//! headless server 進入點。
//!
//! `cargo run --bin open-posd --no-default-features --features server`
//! 編出來的執行檔完全不連 Tauri / WebKit —— 這是把 tauri 藏在 gui feature 後的直接紅利，
//! 也是 M0 的驗收條件之一。

use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "open-posd", version, about = "open-pos headless server")]
struct Args {
    /// 資料根目錄。預設走平台標準路徑（與 GUI 共用同一份資料）。
    #[arg(long, env = "OPEN_POS_DATA_DIR")]
    data_dir: Option<PathBuf>,

    /// 允許把資料放在雲端同步資料夾（預設拒絕，理由見 guard.rs）。
    #[arg(long)]
    allow_cloud_sync: bool,

    /// 區網服務的連接埠。
    #[arg(long, default_value_t = open_pos::lan::DEFAULT_PORT)]
    port: u16,

    /// 靜態頁面目錄（KDS 與掃碼點餐）。預設找執行檔旁邊的 dist/。
    #[arg(long)]
    ui_dir: Option<PathBuf>,
}

/// 找 dist/：先看執行檔旁邊（正式安裝的樣子），再往上找 repo 根（開發時的樣子）。
fn default_ui_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    for up in [0usize, 1, 2, 3] {
        let mut base = dir.to_path_buf();
        for _ in 0..up {
            base = base.parent()?.to_path_buf();
        }
        let candidate = base.join("dist");
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    open_pos::init_tracing(None);

    // 刻意不讓 main 回傳 Result。
    //
    // `fn main() -> Result<_, E>` 失敗時，錯誤是用 **Debug** 格式印出來的，
    // 多行訊息會被壓成一行、換行變成字面上的跳脫字元，完全不能讀。
    // 而開機期的錯誤訊息正是最需要被讀懂的一種 —— 它要告訴一個不懂電腦的店家
    // 現在該怎麼辦（「把資料放回本機磁碟」「已經有一個在跑了」）。
    // 所以這裡自己印 Display 格式，並明確給非零離開碼。
    let rt = match open_pos::boot(open_pos::BootOptions {
        data_dir: args.data_dir,
        allow_cloud_sync: args.allow_cloud_sync,
        max_readers: None,
    })
    .await
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("open-pos 無法啟動：\n\n{e}");
            std::process::exit(1);
        }
    };

    println!("open-posd {} 已啟動", open_pos::VERSION);
    println!("資料目錄：{}", rt.ctx.layout.root.display());

    let ui_dir = args.ui_dir.or_else(default_ui_dir);
    let lan = match open_pos::lan::spawn(rt.ctx.clone(), args.port, ui_dir) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("open-pos 無法啟動：\n\n{e}");
            std::process::exit(1);
        }
    };
    // 刻意不印 lan.addr —— 它是 0.0.0.0（監聽所有介面），不是一個連得上的位址。
    // 印出來只會讓使用者照著貼進瀏覽器然後失敗。
    println!("區網服務：連接埠 {}", lan.addr.port());
    println!("本機測試：http://127.0.0.1:{}", lan.addr.port());
    match open_pos::lan::lan_base_url(args.port) {
        // 這一行就是桌卡 QR 要印的位址。IP 一變全店桌卡就失效，
        // 所以安裝精靈會要求店家在路由器設固定 IP / DHCP 保留。
        Some(url) => println!("平板與手機請連：{url}"),
        None => println!("找不到可用的區網位址 —— 請確認這台電腦已連上店內網路（有線或 Wi-Fi）。"),
    }
    println!("按 Ctrl+C 結束。");

    // 等中斷訊號，然後優雅關閉：讓進行中的請求做完、把連線池收乾淨。
    let _ = tokio::signal::ctrl_c().await;
    println!("\n收到中斷訊號，正在關閉…");
    lan.shutdown().await;
    rt.ctx.db.close().await;
}
