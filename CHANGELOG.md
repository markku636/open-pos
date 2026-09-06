# Changelog

本檔記錄使用者可見的變更。格式參考 [Keep a Changelog](https://keepachangelog.com/zh-TW/1.1.0/)，
版號遵循 [Semantic Versioning](https://semver.org/lang/zh-TW/)。

## [Unreleased]

### Added
- 專案骨架：Tauri 2 + Rust + React 18 + Vite 5 + Tailwind 3
- SQLite 雙連線池（writer=1 / reader=N）與 `UnitOfWork` 交易邊界
- 開機安全檢查：單實例檔案鎖、網路磁碟與雲端同步資料夾拒絕啟動、WAL 模式驗證
- 領域模型 50 張表（商品 / 訂單 / 付款 / 出單 / 班別日結 / 人員權限稽核）
- `VACUUM INTO` 備份與還原，含完整性驗證與「不可用新版備份還原到舊版程式」的保護
- 種子資料：26 個權限碼、5 個系統角色、稅別、付款方式、原因碼
