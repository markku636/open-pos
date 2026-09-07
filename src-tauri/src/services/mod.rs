//! 應用層 use case。
//!
//! **這是兩個 transport（Tauri command 與 axum RPC）唯一的入口。**
//! 交易邊界在這一層：一次 use case = 一個 UnitOfWork = 一個 BEGIN IMMEDIATE。
//!
//! 兩條硬規則：
//! * 交易內嚴禁任何外部 I/O（印表機 TCP、檔案投遞、HTTP）。需要的一律寫進 outbox。
//! * 一個 use case 只取一次時間戳（`Stamp`），往下傳 —— 同一筆交易的多張表
//!   必須拿到完全相同的時間，否則報表對不起來。

pub mod analytics;
pub mod app;
pub mod audit;
pub mod backup;
pub mod backup_worker;
pub mod demo;
pub mod diagnostics;
pub mod dining;
pub mod gateway;
pub mod kds;
pub mod locale;
pub mod menu;
/// 區網連線狀態。只有帶 server feature 時才有網路可言。
#[cfg(feature = "server")]
pub mod network;
pub mod order;
pub mod print_worker;
pub mod printer;
pub mod rbac;
pub mod reason;
pub mod refund;
pub mod report_export;
pub mod sales;
pub mod seed;
pub mod sequence;
pub mod shift;
pub mod table;
pub mod xlsx;
