//! 把位元組寫進檔案的 driver。
//!
//! 三個實際用途，都不是玩具：
//!
//! 1. **沒有出單機的貢獻者也能跑完整條路徑** —— 從點餐、結帳、排版、編碼
//!    一路到「送出去」，只有最後一吋不同。這對開源專案的存活是決定性的。
//! 2. **回報 bug 的附件** —— 店家說「印出來的字是亂碼」時，最有用的東西是
//!    那一份原始位元組，而不是照片。
//! 3. **接別人的系統** —— 有些廚房顯示器與雲端印表機是監看資料夾的。
//!
//! # 為什麼非附加模式要走 `.part` + rename
//!
//! 監看資料夾的程式沒有交握協定，它只是輪詢。直接寫目標檔名，對方會讀到
//! 寫到一半的檔案 —— 症狀是「偶爾有幾張單是壞的」而且無法重現。
//! 先寫暫存檔再 rename，讀取端就只可能看到完整的檔案。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::AsyncWriteExt;

use crate::error::{AppError, AppResult};

use super::{PrinterCaps, PrinterDriver};

pub struct FilePrinter {
    path: PathBuf,
    append: bool,
    caps: PrinterCaps,
    /// 非附加模式時，每一單各自成檔。序號讓同一秒內的多張單不會互相覆蓋 ——
    /// 尖峰時間一秒兩張單是常態。
    seq: AtomicU64,
}

impl FilePrinter {
    pub fn new(path: PathBuf, append: bool, caps: PrinterCaps) -> Self {
        Self {
            path,
            append,
            caps,
            seq: AtomicU64::new(0),
        }
    }

    /// 這一單要寫到哪個檔。
    ///
    /// 附加模式：固定同一個檔（當成一卷紙）。
    /// 非附加模式：`<path>/<stem>-000001.bin`，一單一檔。
    fn target(&self) -> PathBuf {
        if self.append {
            return self.path.clone();
        }
        let n = self.seq.fetch_add(1, Ordering::Relaxed);
        let stem = self
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("job");
        let ext = self
            .path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("bin");
        let dir = self.path.parent().unwrap_or_else(|| Path::new("."));
        dir.join(format!("{stem}-{n:06}.{ext}"))
    }

    async fn ensure_dir(target: &Path) -> AppResult<()> {
        if let Some(dir) = target.parent() {
            if !dir.as_os_str().is_empty() {
                tokio::fs::create_dir_all(dir).await.map_err(|e| {
                    AppError::Printer(format!("建不出資料夾 {}：{e}", dir.display()))
                })?;
            }
        }
        Ok(())
    }
}

#[async_trait]
impl PrinterDriver for FilePrinter {
    fn describe(&self) -> String {
        self.path.display().to_string()
    }

    fn caps(&self) -> PrinterCaps {
        // 檔案沒有錢箱也沒有狀態。**不要**因為「反正寫得進去」就宣稱全部支援 ——
        // caps 是拿來決定版面的，謊報會讓預覽跟真機不一致。
        PrinterCaps {
            drawer: false,
            status_query: false,
            ..self.caps.clone()
        }
    }

    async fn probe(&self) -> AppResult<()> {
        let target = self.target();
        Self::ensure_dir(&target).await?;
        // 只確認資料夾建得出來。**不建立空檔** —— probe 不該留下垃圾。
        Ok(())
    }

    async fn send(&self, bytes: &[u8], budget: Duration) -> AppResult<()> {
        let target = self.target();
        // 網路磁碟上的檔案寫入一樣會卡住（SMB 斷線時可以卡數分鐘），
        // 所以檔案 driver 也要吃 deadline，不能因為「本機檔案很快」就省掉。
        tokio::time::timeout(budget, async {
            Self::ensure_dir(&target).await?;
            let fail =
                |e: std::io::Error| AppError::Printer(format!("寫不進 {}：{e}", target.display()));

            if self.append {
                let mut f = tokio::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&target)
                    .await
                    .map_err(fail)?;
                f.write_all(bytes).await.map_err(fail)?;
                f.flush().await.map_err(fail)?;
                return Ok::<(), AppError>(());
            }

            // 先寫 `.part` 再 rename：監看資料夾的程式只是輪詢，沒有交握協定，
            // 直接寫目標檔名會讓它讀到寫到一半的檔。
            let part = target.with_extension("part");
            let mut f = tokio::fs::File::create(&part).await.map_err(fail)?;
            f.write_all(bytes).await.map_err(fail)?;
            f.flush().await.map_err(fail)?;
            // 出單機的位元組是「已經發生的事」的證據，值得一次 fsync。
            let _ = f.sync_all().await;
            drop(f);
            tokio::fs::rename(&part, &target).await.map_err(fail)?;
            Ok(())
        })
        .await
        .map_err(|_| AppError::Timeout(budget.as_secs()))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::printer::deadline;
    use crate::receipt::PaperWidth;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("open-pos-file-printer-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("job.bin")
    }

    #[tokio::test]
    async fn append_mode_behaves_like_one_long_roll_of_paper() {
        let path = tmp("append");
        let p = FilePrinter::new(
            path.clone(),
            true,
            PrinterCaps::conservative(PaperWidth::Mm58),
        );
        p.send(b"one", deadline::SEND_TEXT).await.unwrap();
        p.send(b"two", deadline::SEND_TEXT).await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"onetwo");
    }

    #[tokio::test]
    async fn each_job_gets_its_own_file_and_no_partial_file_is_left_behind() {
        let path = tmp("split");
        let p = FilePrinter::new(
            path.clone(),
            false,
            PrinterCaps::conservative(PaperWidth::Mm58),
        );
        p.send(b"a", deadline::SEND_TEXT).await.unwrap();
        p.send(b"b", deadline::SEND_TEXT).await.unwrap();

        let dir = path.parent().unwrap();
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, vec!["job-000000.bin", "job-000001.bin"]);
        // `.part` 必須已經被 rename 掉 —— 監看資料夾的程式不該看到半成品。
        assert!(names.iter().all(|n| !n.ends_with(".part")));
    }

    #[tokio::test]
    async fn probe_does_not_leave_an_empty_file() {
        let path = tmp("probe");
        let p = FilePrinter::new(
            path.clone(),
            false,
            PrinterCaps::conservative(PaperWidth::Mm58),
        );
        p.probe().await.unwrap();
        assert!(path.parent().unwrap().exists());
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            0
        );
    }

    #[test]
    fn caps_do_not_overclaim() {
        // caps 是拿來決定版面的。謊報會讓預覽跟真機不一致。
        let p = FilePrinter::new(
            "x.bin".into(),
            true,
            PrinterCaps::conservative(PaperWidth::Mm80),
        );
        assert!(!p.caps().drawer);
        assert!(!p.caps().status_query);
    }
}
