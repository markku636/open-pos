//! 虛擬出單機。
//!
//! 放在這個 crate 而不是 `scripts/` 的決定性理由：**解碼器與編碼器共用同一份
//! `escpos::constants`**，改指令時編譯器會逼你兩邊一起改。不會出現
//! 「模擬器說 OK、真機印出亂碼」這種最難查的落差。
//!
//! 它同時是「讓沒有出單機的人也能貢獻這個專案」的唯一實際做法 ——
//! 一個要有特定硬體才驗得了的開源專案，外部貢獻者是進不來的。
//!
//! ```text
//! fakeprinter --port 9100,9101,9102
//! fakeprinter --port 9100 --simulate paper-out      # 不讀資料，測 deadline
//! fakeprinter --port 9100 --simulate offline        # 接了就斷
//! fakeprinter --port 9100 --simulate slow=200       # 每 KB 慢 200ms
//! fakeprinter --port 9100 --simulate flaky=0.3      # 三成機率印到一半斷線
//! fakeprinter --port 9100 --hex --dump ./jobs       # 存原始位元組供回報 bug
//! ```

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use open_pos::infra::printer::escpos::decode::{self, Op};
use open_pos::infra::printer::escpos::CjkEncoding;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// 要模擬哪一種壞掉的機器。
#[derive(Clone, Debug, Default)]
struct Simulate {
    /// 缺紙：accept 之後完全不讀資料。真機缺紙時就是這樣 ——
    /// 這正是「沒有 deadline 就會靜默死鎖」的來源。
    paper_out: bool,
    /// 關機／離線：accept 之後立刻斷線。
    offline: bool,
    /// 每 KB 慢幾毫秒。便宜機器加一張長長的單就是這種手感。
    slow_ms_per_kb: u64,
    /// 收滿這麼多位元組之後就停住不讀（模擬緩衝區滿）。
    buffer: Option<usize>,
    /// 印到一半斷線的機率（0.0–1.0）。爛網路線與會省電的 AP。
    flaky: f64,
}

impl Simulate {
    fn is_default(&self) -> bool {
        !self.paper_out
            && !self.offline
            && self.slow_ms_per_kb == 0
            && self.buffer.is_none()
            && self.flaky <= 0.0
    }
}

struct Args {
    ports: Vec<u16>,
    encoding: CjkEncoding,
    simulate: Simulate,
    dump: Option<std::path::PathBuf>,
    hex: bool,
    quiet: bool,
}

fn usage() -> ! {
    eprintln!(
        "\
fakeprinter {} —— 虛擬 ESC/POS 出單機

用法：
  fakeprinter [選項]

選項：
  --port <n[,n...]>     監聽的埠，可給多個（預設 9100）
  --encoding <enc>      big5 | gb18030 | utf8（預設 big5）
  --simulate <spec>     paper-out | offline | slow=<ms/KB> | buffer=<bytes> | flaky=<0..1>
                        可以重複給
  --dump <dir>          把每一單的原始位元組另存一份
  --hex                 同時印出十六進位
  --quiet               只印摘要，不印單的內容
  -h, --help            這段說明",
        open_pos::VERSION
    );
    std::process::exit(2)
}

fn parse_args() -> Args {
    let mut a = Args {
        ports: Vec::new(),
        encoding: CjkEncoding::Big5,
        simulate: Simulate::default(),
        dump: None,
        hex: false,
        quiet: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--port" => {
                for p in it.next().unwrap_or_else(|| usage()).split(',') {
                    match p.trim().parse::<u16>() {
                        Ok(n) => a.ports.push(n),
                        Err(_) => {
                            eprintln!("埠必須是 0–65535 的數字：{p}");
                            std::process::exit(2);
                        }
                    }
                }
            }
            "--encoding" => {
                a.encoding = match it
                    .next()
                    .unwrap_or_else(|| usage())
                    .to_ascii_lowercase()
                    .as_str()
                {
                    "big5" => CjkEncoding::Big5,
                    "gb18030" | "gbk" => CjkEncoding::Gb18030,
                    "utf8" | "utf-8" => CjkEncoding::Utf8,
                    other => {
                        eprintln!("不認得的編碼：{other}");
                        std::process::exit(2);
                    }
                }
            }
            "--simulate" => {
                let spec = it.next().unwrap_or_else(|| usage());
                let (k, v) = spec.split_once('=').unwrap_or((spec.as_str(), ""));
                match k {
                    "paper-out" => a.simulate.paper_out = true,
                    "offline" => a.simulate.offline = true,
                    "slow" => a.simulate.slow_ms_per_kb = v.parse().unwrap_or(50),
                    "buffer" => a.simulate.buffer = Some(v.parse().unwrap_or(4096)),
                    "flaky" => a.simulate.flaky = v.parse().unwrap_or(0.2),
                    other => {
                        eprintln!("不認得的模擬項目：{other}");
                        std::process::exit(2);
                    }
                }
            }
            "--dump" => a.dump = Some(it.next().unwrap_or_else(|| usage()).into()),
            "--hex" => a.hex = true,
            "--quiet" => a.quiet = true,
            "-h" | "--help" => usage(),
            other => {
                eprintln!("不認得的參數：{other}");
                usage()
            }
        }
    }
    if a.ports.is_empty() {
        a.ports.push(9100);
    }
    a
}

/// 夠用的偽隨機。
///
/// 刻意不為了 `flaky` 拉一個 `rand` 進來：正式打包雖然不含 dev-tools，
/// 但相依樹是全 crate 共用的，多一個就是多一個要盯的供應鏈。
fn coin(p: f64) -> bool {
    if p <= 0.0 {
        return false;
    }
    static STATE: AtomicU64 = AtomicU64::new(0);
    let mut s = STATE.load(Ordering::Relaxed);
    if s == 0 {
        s = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x2545_F491_4F6C_DD1D)
            | 1;
    }
    // xorshift64：三行、零相依，對「有時候要斷線」這件事綽綽有餘。
    s ^= s << 13;
    s ^= s >> 7;
    s ^= s << 17;
    STATE.store(s, Ordering::Relaxed);
    ((s >> 11) as f64 / (1u64 << 53) as f64) < p
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args = Arc::new(parse_args());

    let mut listeners = Vec::new();
    for port in &args.ports {
        match TcpListener::bind(("0.0.0.0", *port)).await {
            Ok(l) => {
                println!("虛擬出單機在 {port} 待命");
                listeners.push(l);
            }
            Err(e) => {
                // 埠被佔用是這支工具最常見的失敗，訊息要直接說出解法。
                eprintln!("埠 {port} 綁不起來：{e}");
                eprintln!("（是真的有一台出單機在用這個埠，還是上一個 fakeprinter 沒關掉？）");
                std::process::exit(1);
            }
        }
    }
    if !args.simulate.is_default() {
        println!("模擬設定：{:?}", args.simulate);
    }
    if let Some(d) = &args.dump {
        let _ = std::fs::create_dir_all(d);
        println!("原始位元組會另存到 {}", d.display());
    }
    println!("按 Ctrl+C 結束。\n");

    let seq = Arc::new(AtomicU64::new(1));
    for listener in listeners {
        let args = args.clone();
        let seq = seq.clone();
        tokio::spawn(async move {
            let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
            loop {
                let Ok((stream, peer)) = listener.accept().await else {
                    continue;
                };
                let n = seq.fetch_add(1, Ordering::Relaxed);
                let args = args.clone();
                tokio::spawn(async move {
                    handle(stream, port, peer, n, &args).await;
                });
            }
        });
    }
    let _ = tokio::signal::ctrl_c().await;
    println!("\n再見。");
}

async fn handle(
    mut stream: tokio::net::TcpStream,
    port: u16,
    peer: std::net::SocketAddr,
    n: u64,
    args: &Args,
) {
    let sim = &args.simulate;

    if sim.offline {
        println!("#{n} @{port} ← {peer}：離線模擬，直接斷線");
        return;
    }
    if sim.paper_out {
        println!("#{n} @{port} ← {peer}：缺紙模擬，收下連線但不讀資料");
        // 什麼都不做，只是把連線握著 —— 真機缺紙時就是這樣。
        // 對面沒有 deadline 的話，它會在這裡永遠等下去。
        tokio::time::sleep(Duration::from_secs(3600)).await;
        return;
    }

    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = match stream.read(&mut chunk).await {
            Ok(0) => break,
            Ok(k) => k,
            Err(e) => {
                println!("#{n} @{port}：讀取中斷（{e}）");
                break;
            }
        };

        // 狀態查詢要在**串流當中**就回，不能等收完 —— 呼叫端正在等這兩個位元組。
        if let Some(reply) = status_reply(&chunk[..read], sim) {
            let _ = stream.write_all(&reply).await;
            let _ = stream.flush().await;
        }

        buf.extend_from_slice(&chunk[..read]);

        if sim.slow_ms_per_kb > 0 {
            let ms = sim.slow_ms_per_kb * read as u64 / 1024;
            if ms > 0 {
                tokio::time::sleep(Duration::from_millis(ms)).await;
            }
        }
        if let Some(limit) = sim.buffer {
            if buf.len() >= limit {
                println!("#{n} @{port}：緩衝區滿（{limit} bytes），停止讀取");
                tokio::time::sleep(Duration::from_secs(3600)).await;
                return;
            }
        }
        if coin(sim.flaky) {
            println!("#{n} @{port}：★ 模擬斷線（已收 {} bytes）", buf.len());
            return;
        }
    }

    report(&buf, port, peer, n, args);
}

/// 看看這一段位元組裡有沒有 `DLE EOT n`，有的話回一個狀態位元組。
///
/// bit4 恆為 1、bit0/1 恆為 0，是 ESC/POS 的「這確實是狀態位元組」標記。
fn status_reply(bytes: &[u8], sim: &Simulate) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 2 < bytes.len() {
        if bytes[i] == 0x10 && bytes[i + 1] == 0x04 {
            let kind = bytes[i + 2];
            let mut b = 0b0001_0010u8;
            match kind {
                // DLE EOT 1：印表機狀態，bit3 = 離線
                1 if sim.offline => b |= 0b0000_1000,
                // DLE EOT 4：紙張狀態，bit5/6 = 沒紙
                4 if sim.paper_out => b |= 0b0110_0000,
                _ => {}
            }
            out.push(b);
            i += 3;
        } else {
            i += 1;
        }
    }
    (!out.is_empty()).then_some(out)
}

fn report(bytes: &[u8], port: u16, peer: std::net::SocketAddr, n: u64, args: &Args) {
    let ops = decode::decode(bytes, args.encoding);
    let unknown = ops.iter().filter(|o| matches!(o, Op::Unknown(_))).count();

    println!(
        "──── #{n} @{port} ← {peer}　{} bytes　{} 個指令{}",
        bytes.len(),
        ops.len(),
        if unknown > 0 {
            // 解不出來要講出來。安靜跳過正是「模擬器說沒問題、真機印出亂碼」的來源。
            format!("　★ {unknown} 段解不出來")
        } else {
            String::new()
        }
    );

    if let Some(dir) = &args.dump {
        let path = dir.join(format!("job-{n:06}-{port}.bin"));
        match std::fs::write(&path, bytes) {
            Ok(()) => println!("（原始位元組 → {}）", path.display()),
            Err(e) => eprintln!("（存不進 {}：{e}）", path.display()),
        }
    }

    if !args.quiet {
        print!("{}", decode::render_human(&ops));
        // raster 模式下位元組完全看不出對錯，把圖組回來才驗得了對齊與糊字。
        for op in &ops {
            if let Op::Raster {
                width,
                height,
                data,
            } = op
            {
                print!("{}", decode::render_raster_ascii(*width, *height, data));
            }
        }
    }
    if args.hex {
        println!("{}", decode::hex(bytes));
    }
    println!();
}
