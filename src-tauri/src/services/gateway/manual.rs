//! 不串接的金流：收銀員自己抄授權碼。
//!
//! # 這不是「還沒做好」
//!
//! 多數台灣小店的信用卡是走**另一台實體刷卡機**的（銀行或收單機構租的那台）：
//! 收銀員刷完卡，把終端機吐出來的授權碼抄進 POS。那不是暫時的過渡狀態 ——
//! 對很多店來說那大概永遠都會是收信用卡的方式，因為那台機器是銀行給的、
//! 手續費是談好的、壞了有人來修。
//!
//! 所以它是一個正式的實作，而不是 if/else 的另一條分支。其餘所有程式碼
//! 就只需要認識一種形狀：`PaymentGateway`。
//!
//! # 它永遠成功
//!
//! 因為錢已經在另一台機器上收完了。POS 這裡做的只是**記帳**。
//! 它唯一會失敗的情況是收銀員沒有輸入授權碼，而那是驗證，不是金流錯誤。

use crate::error::AppResult;
use crate::services::gateway::{PayOutcome, PayRequest, PaymentGateway, TxStatus};

pub struct ManualGateway;

#[async_trait::async_trait]
impl PaymentGateway for ManualGateway {
    fn provider(&self) -> &'static str {
        "manual"
    }

    /// 不連線。整條非同步路徑都可以跳過 —— 這是離線優先的 POS，
    /// 而「不需要網路」是它最重要的一個性質，能不用就不用。
    fn is_online(&self) -> bool {
        false
    }

    async fn pay(&self, req: &PayRequest) -> AppResult<PayOutcome> {
        Ok(PayOutcome {
            status: TxStatus::Captured,
            // 授權碼由收銀員輸入，寫在 payments.ref_no 上。
            gateway_trade_no: None,
            redirect_url: None,
            raw: None,
            message: format!("已在刷卡機上收 {} 元", req.amount),
        })
    }

    async fn query(&self, _merchant_trade_no: &str) -> AppResult<PayOutcome> {
        // 沒有可以查的對象 —— 真相在那台實體機器的簽單上。
        Ok(PayOutcome {
            status: TxStatus::Captured,
            gateway_trade_no: None,
            redirect_url: None,
            raw: None,
            message: "這一筆沒有串接金流，請看刷卡機的簽單".into(),
        })
    }

    async fn refund(&self, _gateway_trade_no: &str, amount: i64) -> AppResult<PayOutcome> {
        Ok(PayOutcome {
            status: TxStatus::Captured,
            gateway_trade_no: None,
            redirect_url: None,
            raw: None,
            message: format!("請在刷卡機上退 {amount} 元，這裡只記帳"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn manual_never_touches_the_network() {
        let g = ManualGateway;
        assert!(!g.is_online(), "離線優先的 POS 能不連線就不連線");
        let out = g
            .pay(&PayRequest {
                merchant_trade_no: "POS20260907000001".into(),
                amount: 250,
                description: "測試".into(),
                one_time_key: None,
            })
            .await
            .unwrap();
        // 錢已經在另一台機器上收完了，POS 這裡只是記帳。
        assert_eq!(out.status, TxStatus::Captured);
        assert!(out.message.contains("250"));
    }
}
