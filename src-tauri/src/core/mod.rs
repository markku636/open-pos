//! 純業務層：零 I/O、零 async、零 SQL、零 Tauri。
//!
//! 這一層的所有東西都能用假資料在 microsecond 內跑幾十萬次 property test。
//! 它是「錢算對了沒」的唯一真相，也是換資料庫後端時**完全不需要動**的部分。

pub mod business_date;
pub mod clock;
pub mod ids;
pub mod money;

pub use business_date::BusinessDate;
pub use clock::Stamp;
pub use ids::Id;
pub use money::{Micros, Money, RoundingPolicy};
