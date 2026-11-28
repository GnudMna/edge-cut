//! 本体から更新プログラムを起動し、終了の依頼を受け取る
//!
//! 更新プログラムは自身も差し替え対象になるため、`%TEMP%` へコピーして起動する。
//! 更新プログラムの標準出力の `exit` 行を終了の依頼とし、標準入力のパイプは本体が終了するまで保持する
//! (更新プログラムは標準入力の終端で本体の終了を知る)

use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use super::error::UpdateError;
use super::prepare::{UPDATER_SESSION_PREFIX, update_work_root};
use crate::install_layout::UPDATER_EXE_NAME;

// ======================================================================
// 定数
// ======================================================================
/// `EdgeCut.exe` のあるフォルダを渡す引数
pub(crate) const ARG_INSTALL_DIR: &str = "--install-dir";
/// ログの出力先フォルダを渡す引数
pub(crate) const ARG_LOG_DIR: &str = "--log-dir";
/// 更新プログラムが本体へ終了を依頼する行
pub(crate) const EXIT_REQUEST_LINE: &str = "exit";

// ======================================================================
// 更新プログラムとの接続
// ======================================================================
/// 更新プログラムから本体へ届く知らせ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UpdaterEvent {
    /// 本体の終了を依頼された
    ExitRequested,
    /// 更新プログラムが閉じた (終了の依頼の前に閉じた場合は更新されていない)
    Closed,
}

/// 起動中の更新プログラムとの接続
///
/// 破棄すると標準入力のパイプが閉じ、更新プログラムは本体が終了したとみなす
#[derive(Debug)]
pub(crate) struct UpdaterSession {
    /// 本体の終了まで保持する標準入力
    _stdin: ChildStdin,
    /// 標準出力の読み取りスレッドからの知らせ
    rx: Receiver<UpdaterEvent>,
}

impl UpdaterSession {
    /// 届いている知らせを 1 つ取り出す
    ///
    /// # Arguments
    /// * `self` - 接続
    ///
    /// # Returns
    /// * `Option<UpdaterEvent>` - 知らせが無ければ `None`。読み取りスレッドが止まっていれば `Closed`
    pub(crate) fn poll(&self) -> Option<UpdaterEvent> {
        match self.rx.try_recv() {
            Ok(event) => Some(event),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(UpdaterEvent::Closed),
        }
    }
}

/// 更新プログラムを起動する
///
/// 実行フォルダの `EdgeCutUpdater.exe` を `%TEMP%` へコピーして起動する
///
/// # Arguments
/// * `install_dir` - `EdgeCut.exe` のあるフォルダ
/// * `log_dir` - ログの出力先フォルダ
/// * `notify` - 知らせが届いたときに呼ぶ (イベントループを起こす)
///
/// # Returns
/// * `Result<UpdaterSession, UpdateError>` - 起動した更新プログラムとの接続
pub(crate) fn launch_updater(
    install_dir: &Path,
    log_dir: &Path,
    notify: impl Fn() + Send + 'static,
) -> Result<UpdaterSession, UpdateError> {
    let source = install_dir.join(UPDATER_EXE_NAME);
    if !source.is_file() {
        return Err(UpdateError::UpdaterMissing);
    }
    let session_dir = create_session_dir()?;
    let exe = session_dir.join(UPDATER_EXE_NAME);
    fs::copy(&source, &exe).map_err(|_| UpdateError::UpdaterLaunch)?;

    let mut command = Command::new(&exe);
    command
        .args(updater_args(install_dir, log_dir))
        .current_dir(&session_dir);
    spawn_session(command, notify)
}

/// コマンドを標準入出力のパイプ付きで起動し、標準出力の読み取りを始める
fn spawn_session(
    mut command: Command,
    notify: impl Fn() + Send + 'static,
) -> Result<UpdaterSession, UpdateError> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| UpdateError::UpdaterLaunch)?;
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(UpdateError::UpdaterLaunch);
    };

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else {
                break;
            };
            if line.trim() == EXIT_REQUEST_LINE {
                let _ = tx.send(UpdaterEvent::ExitRequested);
                notify();
            }
        }
        let _ = child.wait();
        let _ = tx.send(UpdaterEvent::Closed);
        notify();
    });

    Ok(UpdaterSession { _stdin: stdin, rx })
}

/// 更新プログラムのコピー先フォルダを作る
fn create_session_dir() -> Result<PathBuf, UpdateError> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let name = format!("{UPDATER_SESSION_PREFIX}{}-{nanos}", std::process::id());
    let dir = update_work_root().join(name);
    fs::create_dir_all(&dir).map_err(|_| UpdateError::WorkDirCreate)?;
    Ok(dir)
}

/// 更新プログラムへ渡す引数を組み立てる
fn updater_args(install_dir: &Path, log_dir: &Path) -> Vec<OsString> {
    vec![
        ARG_INSTALL_DIR.into(),
        install_dir.into(),
        ARG_LOG_DIR.into(),
        log_dir.into(),
    ]
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use known_folders::{KnownFolder, get_known_folder_path};

    use super::*;

    /// 知らせが届くまで待つ
    fn wait_event(session: &UpdaterSession) -> UpdaterEvent {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(event) = session.poll() {
                return event;
            }
            assert!(Instant::now() < deadline, "知らせが届かない");
            thread::sleep(Duration::from_millis(20));
        }
    }

    /// 更新プログラムの無いフォルダでは起動せずに失敗すること
    #[test]
    fn launch_updater_fails_without_updater_exe() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert!(matches!(
            launch_updater(tmp.path(), tmp.path(), || {}),
            Err(UpdateError::UpdaterMissing)
        ));
    }

    /// 標準出力の `exit` 行を終了の依頼として受け取り、閉じたことも知らせること
    #[test]
    fn session_reports_exit_request_then_closed() {
        let cmd = get_known_folder_path(KnownFolder::System)
            .expect("system")
            .join("cmd.exe");
        let mut command = Command::new(cmd);
        command.args(["/d", "/c", "echo", EXIT_REQUEST_LINE]);

        let session = spawn_session(command, || {}).expect("spawn");

        assert_eq!(wait_event(&session), UpdaterEvent::ExitRequested);
        assert_eq!(wait_event(&session), UpdaterEvent::Closed);
    }

    /// 引数に実行フォルダとログの場所を渡すこと
    #[test]
    fn updater_args_pass_dirs() {
        let args = updater_args(Path::new(r"C:\app\bin"), Path::new(r"C:\logs"));
        assert_eq!(
            args,
            [ARG_INSTALL_DIR, r"C:\app\bin", ARG_LOG_DIR, r"C:\logs"].map(OsString::from)
        );
    }
}
