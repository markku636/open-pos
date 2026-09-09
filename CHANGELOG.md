# Changelog

本檔記錄使用者可見的變更。格式參考 [Keep a Changelog](https://keepachangelog.com/zh-TW/1.1.0/)，
版號遵循 [Semantic Versioning](https://semver.org/lang/zh-TW/)。

## [Unreleased]

### Added
- 「關於」對話框的 PayPal 贊助段：四個固定金額（US$5 / 10 / 15 / 25）各一顆按鈕
  加「其他金額」走 PayPal.Me，三種介面語言都有譯文，連結一律以系統瀏覽器開啟。
  刻意不做成一條「隨意」連結——「隨意」把「要不要贊助」變成「該給多少才不失禮」，
  而後者要想，想了就關掉了。連結收在 `src/shared/donate.ts` 單一來源，
  與 README 的贊助段、`.github/FUNDING.yml` 同一組
- 專案骨架：Tauri 2 + Rust + React 18 + Vite 5 + Tailwind 3
- SQLite 雙連線池（writer=1 / reader=N）與 `UnitOfWork` 交易邊界
- 開機安全檢查：單實例檔案鎖、網路磁碟與雲端同步資料夾拒絕啟動、WAL 模式驗證
- 領域模型 50 張表（商品 / 訂單 / 付款 / 出單 / 班別日結 / 人員權限稽核）
- `VACUUM INTO` 備份與還原，含完整性驗證與「不可用新版備份還原到舊版程式」的保護
- 種子資料：26 個權限碼、5 個系統角色、稅別、付款方式、原因碼
- RBAC 權限檢查（權限集合快取、在職狀態不快取）與稽核寫入服務
- 區網 HTTP server：`POST /api/rpc/{name}` 統一入口、`/api/health` 健康檢查、
  KDS 與掃碼點餐頁的靜態服務
- Tauri 桌面視窗（開機失敗時以原生對話框顯示可讀的錯誤）
- **商品維護**：分類 / 品項 / 規格的建立、修改、刪除，含改價稽核
- **定價引擎**：十一步計算順序的純函式實作，含 20000 組隨機訂單的不變量掃描
- **收據版面引擎**：`ReceiptDoc` DSL、東亞寬度排版、純文字 renderer、廚房單與收據版型
- **點餐與結帳**：開單 / 加點 / 退點 / 混合支付結帳，含樂觀鎖、冪等與出單佇列
