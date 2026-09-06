//! 開機期安全檢查。
//!
//! 這個模組存在的理由是一句話：**「絕不可以把資料庫放在網路磁碟」「絕不可以跑兩個實例」
//! 這兩件事，用 README 上的一行字是擋不住的。**
//!
//! 兩個具體失敗情境（都不會有任何錯誤訊息，所以必須事前擋）：
//!
//! 1. 老闆點了桌面捷徑沒看到視窗（其實在背景），又點一次。兩份 outbox worker 掃到
//!    同一批工作 → 同一張單印兩次、同一張發票上傳兩次、發票字軌被兩邊各自推進造成
//!    跳號或重號。SQLite 完全不會抱怨 —— 本機檔案系統上 WAL 本來就支援多程序併發。
//!    所以要守的不變量不是「SQLite 不能多程序」，而是
//!    **「單一 outbox / 列印佇列 / 發票配號 leader」**。
//!
//! 2. 老闆看到「資料庫就是一個檔案」，覺得放 NAS 比較保險，或把資料夾丟進雲端同步。
//!    SQLite 官方明言 WAL 無法跨網路檔案系統（-shm 需要跨主機共享記憶體，SMB 給不了）；
//!    雲端同步引擎則會各自獨立複製 db / -wal / -shm 三個檔。兩者的結果都是資料損毀。

use std::fs::{File, OpenOptions};
use std::path::Path;

use crate::error::{AppError, AppResult};

/// 常見的雲端同步資料夾特徵字串（小寫比對）。
const CLOUD_SYNC_MARKERS: &[&str] = &[
    "onedrive",
    "dropbox",
    "google drive",
    "googledrive",
    "icloud",
    "creative cloud",
];

/// 持有單實例鎖。**必須讓它活到 process 結束** —— drop 掉鎖就沒了。
pub struct InstanceLock {
    _file: File,
}

/// 對 lock_path 取 OS 獨佔鎖。第二個實例會拿不到而直接失敗。
///
/// 用 try_lock_exclusive 而不是阻塞版：我們要的是「立刻告訴使用者已經有一個在跑」，
/// 不是安靜地等到對方結束。
pub fn acquire_instance_lock(lock_path: &Path) -> AppResult<InstanceLock> {
    use fs4::fs_std::FileExt;

    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::Storage(format!("建立資料目錄失敗：{e}")))?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)
        .map_err(|e| AppError::Storage(format!("開啟鎖定檔失敗 {}：{e}", lock_path.display())))?;

    match file.try_lock_exclusive() {
        Ok(true) => Ok(InstanceLock { _file: file }),
        Ok(false) | Err(_) => Err(AppError::Startup(format!(
            "本機已有另一個 open-pos 正在使用這份資料（{}）。\n\
             請切換到已開啟的視窗；若確定它已經關閉，請等候數秒後重試。",
            lock_path.display()
        ))),
    }
}

/// 資料目錄是否安全。不安全就**拒絕啟動**，並在訊息裡直接給出正確做法。
///
/// 刻意分成「硬拒絕」（網路磁碟）與「需確認」（雲端同步資料夾）兩級：
/// 前者一定壞，後者是使用者可能有理由這麼做。
pub fn check_data_dir_safety(dir: &Path, allow_cloud_sync: bool) -> AppResult<()> {
    if is_network_path(dir) {
        return Err(AppError::Startup(format!(
            "資料目錄位於網路磁碟：{}\n\n\
             SQLite 的 WAL 模式無法在網路檔案系統上安全運作（會靜默損毀資料）。\n\
             正確做法：把資料放在這台電腦的本機磁碟，其他裝置改用瀏覽器連進來\n\
             （廚房平板開 KDS 頁、顧客手機掃桌卡 QR），而不是共用同一個資料庫檔案。",
            dir.display()
        )));
    }
    if !allow_cloud_sync {
        if let Some(marker) = cloud_sync_marker(dir) {
            return Err(AppError::Startup(format!(
                "資料目錄疑似位於雲端同步資料夾（偵測到 {marker}）：{}\n\n\
                 同步引擎會各自獨立複製 pos.db / -wal / -shm 三個檔案，造成資料庫損毀。\n\
                 正確做法：把資料放在非同步的本機資料夾，備份改用內建的「備份到 USB」功能。\n\
                 若你確定要繼續，請在設定中開啟「允許雲端同步資料夾」。",
                dir.display()
            )));
        }
    }
    Ok(())
}

/// 路徑是否為 UNC 或網路磁碟機。
pub fn is_network_path(dir: &Path) -> bool {
    let s = dir.to_string_lossy();
    if s.starts_with("\\\\") || s.starts_with("//") {
        return true;
    }
    #[cfg(windows)]
    {
        windows_drive_is_remote(&s)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
fn windows_drive_is_remote(path: &str) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
    // windows-sys 0.61 沒有把這個常數匯出到 FileSystem 模組。它的值由 Win32 定義且永不改變，
    // 直接寫死比為了一個整數去追 crate 的模組搬遷位置划算。
    const DRIVE_REMOTE: u32 = 4;

    let bytes = path.as_bytes();
    if bytes.len() < 2 || bytes[1] != 0x3A {
        return false;
    }
    let root = format!("{}:\\", bytes[0] as char);
    let wide: Vec<u16> = std::ffi::OsStr::new(&root)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: wide 是以 NUL 結尾的合法 UTF-16 字串，生命週期涵蓋這次呼叫。
    unsafe { GetDriveTypeW(wide.as_ptr()) == DRIVE_REMOTE }
}

/// 路徑中命中的雲端同步特徵字串。
pub fn cloud_sync_marker(dir: &Path) -> Option<&'static str> {
    let lower = dir.to_string_lossy().to_lowercase();
    CLOUD_SYNC_MARKERS
        .iter()
        .find(|m| lower.contains(*m))
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn unc_path_is_rejected() {
        assert!(is_network_path(Path::new(r"\\nas\share\pos")));
        assert!(is_network_path(Path::new("//nas/share/pos")));
        assert!(!is_network_path(Path::new(r"C:\ProgramData\open-pos")));
    }

    #[test]
    fn cloud_sync_folder_is_detected_case_insensitively() {
        assert_eq!(
            cloud_sync_marker(Path::new(r"C:\Users\a\OneDrive\pos")),
            Some("onedrive")
        );
        assert_eq!(
            cloud_sync_marker(Path::new(r"C:\Users\a\Google Drive\pos")),
            Some("google drive")
        );
        assert!(cloud_sync_marker(Path::new(r"C:\pos-data")).is_none());
    }

    #[test]
    fn cloud_sync_can_be_explicitly_allowed() {
        let p = PathBuf::from(r"C:\Users\a\Dropbox\pos");
        assert!(check_data_dir_safety(&p, false).is_err());
        assert!(check_data_dir_safety(&p, true).is_ok());
    }

    #[test]
    fn second_instance_is_refused() {
        let dir = std::env::temp_dir().join(format!("openpos_lock_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let lock = dir.join("pos.lock");

        let first = acquire_instance_lock(&lock).expect("第一個實例應該拿得到鎖");
        let second = acquire_instance_lock(&lock);
        assert!(second.is_err(), "第二個實例必須被拒絕");

        drop(first);
        let third = acquire_instance_lock(&lock);
        assert!(third.is_ok(), "鎖釋放後應可重新取得");
        drop(third);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
