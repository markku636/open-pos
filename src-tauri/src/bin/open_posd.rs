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
    println!("資料目錄：{}", rt.layout.root.display());
    match rt.db.schema_fingerprint().await {
        Ok(fp) => println!("schema 指紋長度：{}", fp.len()),
        Err(e) => eprintln!("讀取 schema 指紋失敗：{e}"),
    }

    rt.db.close().await;
}
