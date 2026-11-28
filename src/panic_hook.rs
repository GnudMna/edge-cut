//! panic の内容をログファイルへ記録し、UI スレッドの panic ではダイアログで通知する
//!
//! Windows のリリースビルドはコンソールを持たないため、既定のフックだけでは panic が何も表示されずに終了する。
//! ワーカースレッドの panic は `catch_panic` が UI へ通知するため、ダイアログは出さずログだけ残す。

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::panic::{self, PanicHookInfo};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::consts;
use crate::install_layout;
use crate::panic_message::panic_message;

/// ログファイル名
const LOG_FILE_NAME: &str = "panic.log";

/// 退避ログのファイル名に付ける接尾辞
const ROTATED_SUFFIX: &str = ".1";

/// ログファイルの上限サイズ (バイト)。超える場合は退避ログへ移す
const MAX_LOG_BYTES: u64 = 256 * 1024;

// ======================================================================
// フックの登録
// ======================================================================
/// panic フックを登録する
///
/// 既存のフック (標準エラーへの出力) は維持し、その後にログ記録とダイアログ表示を行う
pub(crate) fn install() {
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        default_hook(info);
        report(info);
    }));
}

/// panic の内容をログへ追記し、UI スレッドの場合はダイアログを表示する
///
/// ログの書き込みに失敗しても、フック内でできる対処はないため無視する
///
/// # Arguments
/// * `info` - panic の発生箇所とペイロード
fn report(info: &PanicHookInfo<'_>) {
    let thread = std::thread::current();
    let location = info.location().map(ToString::to_string);
    let message = panic_message(info.payload());
    let text = format_report(thread.name(), location.as_deref(), &message);

    let path = log_path();
    let _ = write_log(&path, &text, MAX_LOG_BYTES);

    // UI スレッドは Rust のメインスレッド (名前は "main")
    if thread.name() == Some("main") {
        show_dialog(location.as_deref(), &message, &path);
    }
}

// ======================================================================
// ログ
// ======================================================================
/// ログファイルのパスを返す
///
/// `%LOCALAPPDATA%\EdgeCut\panic.log` (ポータブル版は `<実行フォルダ>\data\panic.log`)
/// 保存先を決められない場合は一時ディレクトリ配下
///
/// # Returns
/// * `PathBuf` - ログファイルのパス
fn log_path() -> PathBuf {
    install_layout::local_data_dir()
        .unwrap_or_else(|| std::env::temp_dir().join(consts::APP_NAME))
        .join(LOG_FILE_NAME)
}

/// ログ 1 件分の文字列を組み立てる
///
/// # Arguments
/// * `thread_name` - panic したスレッドの名前
/// * `location` - panic の発生箇所 (`file:line:column`)
/// * `message` - panic メッセージ
///
/// # Returns
/// * `String` - 末尾に空行を含むログ文字列
fn format_report(thread_name: Option<&str>, location: Option<&str>, message: &str) -> String {
    let unix_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format!(
        "[unix_time={unix_time}] {} v{}\nthread: {}\nlocation: {}\nmessage: {message}\n\n",
        consts::APP_NAME,
        env!("CARGO_PKG_VERSION"),
        thread_name.unwrap_or("<unnamed>"),
        location.unwrap_or("<unknown>"),
    )
}

/// 退避ログのパスを返す (ログファイル名に `.1` を付けたもの)
///
/// # Arguments
/// * `path` - ログファイルのパス
///
/// # Returns
/// * `PathBuf` - 退避ログのパス
fn rotated_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(ROTATED_SUFFIX);
    PathBuf::from(name)
}

/// ログファイルへ追記する (親ディレクトリは必要に応じて作成する)
///
/// 追記すると `max_bytes` を超える場合は、現在のログを退避ログへ移してから書き込む
/// 退避ログは 1 世代だけ残し、古いものは上書きする
///
/// # Arguments
/// * `path` - ログファイルのパス
/// * `text` - 追記する文字列
/// * `max_bytes` - ログファイルの上限サイズ
///
/// # Errors
/// ディレクトリの作成・ログの退避・ファイルの書き込みのいずれかに失敗した場合
fn write_log(path: &Path, text: &str, max_bytes: u64) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // 空のログは退避しない (1 件が上限を超えても空のログへは書き込む)
    let current = fs::metadata(path).map_or(0, |m| m.len());
    if current > 0 && current + text.len() as u64 > max_bytes {
        fs::rename(path, rotated_path(path))?;
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(text.as_bytes())
}

// ======================================================================
// ダイアログ
// ======================================================================
/// 異常終了することを知らせるダイアログを表示する
///
/// # Arguments
/// * `location` - panic の発生箇所
/// * `message` - panic メッセージ
/// * `log_path` - 詳細を記録したログファイルのパス
fn show_dialog(location: Option<&str>, message: &str, log_path: &Path) {
    let description = format!(
        "予期しないエラーが発生したため、アプリを終了します。\n\n{message}\n{}\n\n詳細は次のファイルに記録しています:\n{}",
        location.unwrap_or_default(),
        log_path.display(),
    );
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title(consts::APP_NAME)
        .set_description(description)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// スレッド名・発生箇所・メッセージをログ文字列に含めること
    #[test]
    fn format_report_contains_all_fields() {
        let text = format_report(Some("main"), Some("src/lib.rs:1:2"), "boom");
        assert!(text.contains("thread: main\n"));
        assert!(text.contains("location: src/lib.rs:1:2\n"));
        assert!(text.contains("message: boom\n"));
        assert!(text.ends_with("\n\n"));

        // 取得できない項目は代替表記になる
        let text = format_report(None, None, "boom");
        assert!(text.contains("thread: <unnamed>\n"));
        assert!(text.contains("location: <unknown>\n"));
    }

    /// 親ディレクトリを作成し、既存の内容を残して追記すること
    #[test]
    fn write_log_creates_directory_and_appends() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join(LOG_FILE_NAME);

        write_log(&path, "first\n", MAX_LOG_BYTES).unwrap();
        write_log(&path, "second\n", MAX_LOG_BYTES).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "first\nsecond\n");
        assert!(!rotated_path(&path).exists());
    }

    /// 上限を超える場合は現在のログを退避し、退避ログを 1 世代だけ残すこと
    #[test]
    fn write_log_rotates_when_over_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LOG_FILE_NAME);
        let rotated = rotated_path(&path);

        // 上限に収まる間は追記される
        write_log(&path, "aaaa\n", 12).unwrap();
        write_log(&path, "bbbb\n", 12).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "aaaa\nbbbb\n");

        // 超える場合は退避してから新しいログへ書く
        write_log(&path, "cccc\n", 12).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "cccc\n");
        assert_eq!(fs::read_to_string(&rotated).unwrap(), "aaaa\nbbbb\n");

        // 退避ログは上書きされ、世代は増えない
        write_log(&path, "dddd\n", 12).unwrap();
        write_log(&path, "eeee\n", 12).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "eeee\n");
        assert_eq!(fs::read_to_string(&rotated).unwrap(), "cccc\ndddd\n");
    }

    /// 1 件が上限を超えても空のログには書き込むこと
    #[test]
    fn write_log_writes_oversized_entry_to_empty_log() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LOG_FILE_NAME);

        write_log(&path, "0123456789\n", 4).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "0123456789\n");
        assert!(!rotated_path(&path).exists());
    }
}
