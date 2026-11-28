//! 更新プログラムの副作用の実装
//!
//! 本体の終了は、本体が保持する標準入力のパイプの終端と、実行ファイルの使用状況で判定する。
//! 終了の依頼は標準出力へ `exit` 行を書いて伝える (`session` と対になる)

use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use super::check::check_for_update;
use super::error::UpdateError;
use super::flow::{AppStop, Ops};
use super::install::{application_folder, install_msi, install_portable, launch_app};
use super::prepare::{is_exe_in_use, prepare_update, remove_work_dir};
use super::session::{ARG_INSTALL_DIR, ARG_LOG_DIR, EXIT_REQUEST_LINE};
use super::types::{
    AvailableUpdate, InstallOutcome, PreparedPackage, UpdateCheckOutcome, UpdatePackageKind,
};
use crate::install_layout::APP_EXE_NAME;

// ======================================================================
// 定数
// ======================================================================
/// `msiexec` の詳細ログのファイル名 (更新のたびに上書きする)
const MSI_LOG_FILE_NAME: &str = "updater-msi.log";
/// 本体の実行ファイルが解放されたかを調べる間隔
const EXE_POLL_INTERVAL: Duration = Duration::from_millis(200);

// ======================================================================
// 引数
// ======================================================================
/// 更新プログラムの引数
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UpdaterArgs {
    /// `EdgeCut.exe` のあるフォルダ
    pub install_dir: PathBuf,
    /// ログの出力先フォルダ
    pub log_dir: PathBuf,
}

impl UpdaterArgs {
    /// 引数を解釈する
    ///
    /// # Arguments
    /// * `args` - プログラム名を除いた引数
    ///
    /// # Returns
    /// * `Option<Self>` - 必要な引数が揃わない、または `EdgeCut.exe` が無い場合は `None`
    pub(crate) fn parse(args: impl IntoIterator<Item = OsString>) -> Option<Self> {
        let (mut install_dir, mut log_dir) = (None, None);
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let slot = match arg.to_str() {
                Some(ARG_INSTALL_DIR) => &mut install_dir,
                Some(ARG_LOG_DIR) => &mut log_dir,
                _ => return None,
            };
            *slot = Some(PathBuf::from(args.next()?));
        }
        let install_dir = install_dir?;
        if !install_dir.join(APP_EXE_NAME).is_file() {
            return None;
        }
        Some(Self {
            install_dir,
            log_dir: log_dir?,
        })
    }
}

// ======================================================================
// 副作用
// ======================================================================
/// 実際のインストール先に対する副作用
pub(crate) struct SystemOps {
    install_dir: PathBuf,
    log_dir: PathBuf,
    kind: UpdatePackageKind,
    /// 維持する MSI のインストール先 (`ensure_updatable` で確定する)
    application_folder: Option<PathBuf>,
    /// 標準入力の終端 (本体の終了) の知らせ
    stdin_closed: Receiver<()>,
}

impl SystemOps {
    /// 引数から組み立て、標準入力の監視を始める
    ///
    /// # Arguments
    /// * `args` - 更新プログラムの引数
    ///
    /// # Returns
    /// * `Self` - 副作用の実装
    pub(crate) fn new(args: &UpdaterArgs) -> Self {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            // 本体以外から起動された場合 (標準入力なし) はすぐに終端になる
            let _ = io::copy(&mut io::stdin().lock(), &mut io::sink());
            let _ = tx.send(());
        });
        Self {
            kind: UpdatePackageKind::for_install_dir(&args.install_dir),
            install_dir: args.install_dir.clone(),
            log_dir: args.log_dir.clone(),
            application_folder: None,
            stdin_closed: rx,
        }
    }

    /// `msiexec` の詳細ログの場所
    fn msi_log_path(&self) -> PathBuf {
        self.log_dir.join(MSI_LOG_FILE_NAME)
    }
}

impl Ops for SystemOps {
    fn check(&self) -> UpdateCheckOutcome {
        check_for_update(self.kind)
    }

    fn ensure_updatable(&mut self) -> Result<(), UpdateError> {
        if self.kind == UpdatePackageKind::Msi {
            self.application_folder = Some(application_folder(&self.install_dir)?);
        }
        Ok(())
    }

    fn prepare(
        &self,
        update: &AvailableUpdate,
        on_progress: &mut dyn FnMut(u64) -> bool,
    ) -> Result<PreparedPackage, UpdateError> {
        prepare_update(update, on_progress)
    }

    fn stop_app(&self, timeout: Duration) -> Result<AppStop, UpdateError> {
        let deadline = Instant::now() + timeout;
        // 依頼する前に標準入力が閉じていれば、利用者が本体を閉じている
        if self.stdin_closed.try_recv().is_ok() {
            wait_exe_released(&self.install_dir.join(APP_EXE_NAME), deadline)?;
            return Ok(AppStop::ClosedByUser);
        }

        // 依頼の直前に本体が終了して書けない場合も、続けて終了を確かめる
        let mut stdout = io::stdout().lock();
        let _ = writeln!(stdout, "{EXIT_REQUEST_LINE}").and_then(|()| stdout.flush());
        drop(stdout);

        match self.stdin_closed.recv_timeout(timeout) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {}
            Err(RecvTimeoutError::Timeout) => return Err(UpdateError::ExitTimeout),
        }
        wait_exe_released(&self.install_dir.join(APP_EXE_NAME), deadline)?;
        Ok(AppStop::Requested)
    }

    fn install(&mut self, package: &PreparedPackage) -> Result<InstallOutcome, UpdateError> {
        match package {
            PreparedPackage::Msi { path } => {
                let folder = self
                    .application_folder
                    .as_deref()
                    .ok_or(UpdateError::InstallLocationMismatch)?;
                install_msi(path, folder, &self.msi_log_path())
            }
            PreparedPackage::Portable { source_dir } => {
                install_portable(source_dir, &self.install_dir)?;
                Ok(InstallOutcome::Installed)
            }
        }
    }

    fn start(&self) -> Result<(), UpdateError> {
        launch_app(&self.install_dir)
    }

    fn log_path(&self) -> Option<PathBuf> {
        let path = self.msi_log_path();
        (self.kind == UpdatePackageKind::Msi && path.is_file()).then_some(path)
    }

    fn cleanup(&self, update: &AvailableUpdate) {
        remove_work_dir(&update.latest_version);
    }
}

/// 本体の実行ファイルが解放される (プロセスが消える) まで待つ
///
/// パイプが閉じてもプロセスの後片付けが終わるまでは実行ファイルを差し替えられないため
fn wait_exe_released(exe: &Path, deadline: Instant) -> Result<(), UpdateError> {
    while is_exe_in_use(exe) {
        if Instant::now() >= deadline {
            return Err(UpdateError::ExitTimeout);
        }
        thread::sleep(EXE_POLL_INTERVAL);
    }
    Ok(())
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    /// 実行フォルダとログの場所を受け取ること
    #[test]
    fn parse_reads_dirs() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(tmp.path().join(APP_EXE_NAME), b"x").expect("exe");
        let args = [
            OsString::from(ARG_LOG_DIR),
            OsString::from(r"C:\logs"),
            OsString::from(ARG_INSTALL_DIR),
            tmp.path().into(),
        ];

        assert_eq!(
            UpdaterArgs::parse(args),
            Some(UpdaterArgs {
                install_dir: tmp.path().to_path_buf(),
                log_dir: PathBuf::from(r"C:\logs"),
            })
        );
    }

    /// 引数の不足・不明な引数・本体の無いフォルダを拒否すること
    #[test]
    fn parse_rejects_invalid_args() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = OsString::from(tmp.path());
        let log = OsString::from("logs");
        let cases: [Vec<OsString>; 4] = [
            vec![],
            vec![ARG_INSTALL_DIR.into(), dir.clone()],
            vec![
                ARG_INSTALL_DIR.into(),
                dir.clone(),
                "--other".into(),
                log.clone(),
            ],
            vec![ARG_INSTALL_DIR.into(), dir, ARG_LOG_DIR.into(), log],
        ];
        for args in cases {
            assert_eq!(UpdaterArgs::parse(args.clone()), None, "{args:?}");
        }
    }

    /// 使用中でない実行ファイルは待たずに終わり、使用中のまま期限を過ぎると失敗すること
    #[test]
    fn wait_exe_released_respects_deadline() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let idle = tmp.path().join(APP_EXE_NAME);
        fs::write(&idle, b"x").expect("exe");
        assert_eq!(wait_exe_released(&idle, Instant::now()), Ok(()));

        let running = std::env::current_exe().expect("current exe");
        assert_eq!(
            wait_exe_released(&running, Instant::now()),
            Err(UpdateError::ExitTimeout)
        );
    }
}
