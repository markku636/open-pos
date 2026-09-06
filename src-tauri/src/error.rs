//! 統一錯誤型別。
//!
//! 形狀比照 db-kit：`kind()` / `code()` / `message()` + 手寫 `Serialize`，
//! 序列化成 `{kind, code, message, retryable}`（additive —— 前端只讀 message 也不會壞）。
//!
//! 與 db-kit 的差異：多了 `http_status()`。這是「同一個錯誤型別同時服務兩個 transport」
//! 的關鍵 —— Tauri 的 invoke 失敗會 reject 出這個物件，axum 則靠 `IntoResponse`
//! 轉成非 2xx + `{"error": ...}`，兩邊產出同形，前端才能只有一份 api.ts。

use serde::ser::{Serialize, SerializeStruct, Serializer};

/// # Display 為什麼不掛英文分類前綴
///
/// `kind()` 與 `code()` 已經帶了機器可讀的分類，而 `message()` 是**唯一會被
/// 直接展示給使用者**的東西。在每一句中文訊息前面掛一個 `not found:` 只會
/// 干擾閱讀 —— 尤其開機失敗與權限不足這兩類，讀的人是不懂電腦的店家。
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    NotFound(String),

    /// 使用者輸入不合法（金額為負、品項不存在、數量為 0）。
    #[error("{0}")]
    Validation(String),

    /// 樂觀鎖版本不符 / 狀態機不允許（單已結帳還想加點）。前端應重讀後重試。
    #[error("{0}")]
    Conflict(String),

    #[error("尚未登入或帳號已停用")]
    Unauthorized,

    #[error("{0}")]
    Forbidden(String),

    #[error("資料庫錯誤：{0}")]
    Db(String),

    #[error("檔案存取錯誤：{0}")]
    Storage(String),

    /// 開機期安全檢查失敗（網路磁碟、雲端同步資料夾、已有另一個實例在跑）。
    /// 這一類一律是「拒絕啟動」而非「降級執行」—— 見 guard.rs 的理由。
    ///
    /// Display 刻意不加前綴：這是唯一會被**直接展示給不懂電腦的店家**的錯誤，
    /// 訊息本身就是完整的一段說明（含「該怎麼辦」），前面掛一個英文分類只會干擾閱讀。
    /// 機器可讀的分類仍由 kind() / code() 提供。
    #[error("{0}")]
    Startup(String),

    #[error("出單機錯誤：{0}")]
    Printer(String),

    #[error("電子發票錯誤：{0}")]
    Fiscal(String),

    #[error("{0}")]
    Unsupported(String),

    #[error("逾時（{0} 毫秒）")]
    Timeout(u64),

    #[error("內部錯誤：{0}")]
    Internal(String),
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "not_found",
            Self::Validation(_) => "validation",
            Self::Conflict(_) => "conflict",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden(_) => "forbidden",
            Self::Db(_) => "database",
            Self::Storage(_) => "storage",
            Self::Startup(_) => "startup",
            Self::Printer(_) => "printer",
            Self::Fiscal(_) => "fiscal",
            Self::Unsupported(_) => "unsupported",
            Self::Timeout(_) => "timeout",
            Self::Internal(_) => "internal",
        }
    }

    /// 機器可讀、與顯示語言無關。前端據此分支，不要去比對 message。
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "ERR_NOT_FOUND",
            Self::Validation(_) => "ERR_VALIDATION",
            Self::Conflict(_) => "ERR_CONFLICT",
            Self::Unauthorized => "ERR_UNAUTHORIZED",
            Self::Forbidden(_) => "ERR_FORBIDDEN",
            Self::Db(_) => "ERR_DB",
            Self::Storage(_) => "ERR_STORAGE",
            Self::Startup(_) => "ERR_STARTUP",
            Self::Printer(_) => "ERR_PRINTER",
            Self::Fiscal(_) => "ERR_FISCAL",
            Self::Unsupported(_) => "ERR_UNSUPPORTED",
            Self::Timeout(_) => "ERR_TIMEOUT",
            Self::Internal(_) => "ERR_INTERNAL",
        }
    }

    pub fn message(&self) -> String {
        self.to_string()
    }

    /// 讓同一個錯誤型別能服務 axum。
    pub fn http_status(&self) -> u16 {
        match self {
            Self::NotFound(_) => 404,
            Self::Validation(_) => 400,
            // 409：前端據此自動重讀 + 重試（樂觀鎖衝突）。
            Self::Conflict(_) => 409,
            Self::Unauthorized => 401,
            Self::Forbidden(_) => 403,
            Self::Unsupported(_) => 501,
            Self::Timeout(_) => 504,
            _ => 500,
        }
    }

    /// 前端是否應自動重試（暫時性錯誤）。
    pub fn retryable(&self) -> bool {
        matches!(self, Self::Timeout(_) | Self::Db(_) | Self::Printer(_))
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut st = s.serialize_struct("AppError", 4)?;
        st.serialize_field("kind", self.kind())?;
        st.serialize_field("code", self.code())?;
        st.serialize_field("message", &self.message())?;
        st.serialize_field("retryable", &self.retryable())?;
        st.end()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        Self::Db(e.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        Self::Storage(e.to_string())
    }
}

#[cfg(feature = "server")]
impl axum::response::IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let status = axum::http::StatusCode::from_u16(self.http_status())
            .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        (status, axum::Json(serde_json::json!({ "error": self }))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conflict_maps_to_409_and_is_not_retryable() {
        // 樂觀鎖衝突要讓前端「重讀後重試」，而不是盲目自動重送 —— 盲目重送會覆蓋別人的修改。
        let e = AppError::Conflict("rev mismatch".into());
        assert_eq!(e.http_status(), 409);
        assert!(!e.retryable());
    }

    #[test]
    fn serializes_with_stable_shape() {
        let json = serde_json::to_value(AppError::NotFound("order".into())).unwrap();
        assert_eq!(json["kind"], "not_found");
        assert_eq!(json["code"], "ERR_NOT_FOUND");
        assert_eq!(json["message"], "order");
        assert_eq!(json["retryable"], false);
    }
}
