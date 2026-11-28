//! MSI の無人インストール
//!
//! インストーラーの画面は出さない (`/qn`)。導入先 (`APPLICATIONFOLDER`) は本体の実行フォルダから求めて維持し、
//! 進捗と結果は更新プログラムの画面に表示する

use std::fs;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use known_folders::{KnownFolder, get_known_folder_path};

use super::super::error::UpdateError;
use super::super::types::InstallOutcome;

// ======================================================================
// 定数
// ======================================================================
/// インストール先 (`APPLICATIONFOLDER`) 直下の、実行ファイルを置くフォルダ名 (`wix/main.wxs` と揃える)
const BIN_DIR_NAME: &str = "bin";
/// 成功
const EXIT_SUCCESS: i32 = 0;
/// 別のインストールが進行中
const EXIT_INSTALL_ALREADY_RUNNING: i32 = 1618;
/// 成功 (再起動を開始した)
const EXIT_REBOOT_INITIATED: i32 = 1641;
/// 成功 (再起動が必要)
const EXIT_REBOOT_REQUIRED: i32 = 3010;

// ======================================================================
// インストール先
// ======================================================================
/// 本体の実行フォルダが MSI の導入先か確かめ、`APPLICATIONFOLDER` を返す
///
/// MSI は `<インストール先>\bin\EdgeCut.exe` に置くため、`bin` の親を導入先とする
/// 別のフォルダへ複製した `EdgeCut.exe` から更新すると、導入先だけが新しくなり、
/// 動かしていた方は古いまま残るため拒否する
///
/// # Arguments
/// * `install_dir` - `EdgeCut.exe` のあるフォルダ
///
/// # Returns
/// * `Result<PathBuf, UpdateError>` - インストール先 (`bin` の親フォルダ)
pub(crate) fn application_folder(install_dir: &Path) -> Result<PathBuf, UpdateError> {
    let is_bin = install_dir
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(BIN_DIR_NAME));
    // ドライブ直下は末尾の `\` が `msiexec` の引数で閉じ引用符と紛れるため対象外
    let parent = install_dir
        .parent()
        .filter(|parent| parent.parent().is_some());
    match parent {
        Some(parent) if is_bin => Ok(parent.to_path_buf()),
        _ => Err(UpdateError::InstallLocationMismatch),
    }
}

// ======================================================================
// インストール
// ======================================================================
/// MSI を無人インストールし、終わるまで待つ
///
/// # Arguments
/// * `msi_path` - 検証済みの MSI
/// * `application_folder` - 維持するインストール先
/// * `log_path` - `msiexec` の詳細ログの出力先
///
/// # Returns
/// * `Result<InstallOutcome, UpdateError>` - インストール結果
pub(crate) fn install_msi(
    msi_path: &Path,
    application_folder: &Path,
    log_path: &Path,
) -> Result<InstallOutcome, UpdateError> {
    let msiexec = system_msiexec_path().ok_or(UpdateError::MsiLaunch)?;
    let args = msiexec_args(msi_path, application_folder, log_path)?;
    if let Some(dir) = log_path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let status = Command::new(msiexec)
        .raw_arg(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| UpdateError::MsiLaunch)?;
    outcome_from_exit_code(status.code().unwrap_or(-1))
}

/// システムフォルダ (`System32`) の `msiexec.exe` の絶対パスを返す
///
/// 実行ファイルと同じフォルダの同名ファイルや `PATH` 上の偽物を拾わないよう、
/// 環境変数に依存しない Known Folders API (`FOLDERID_System`) で解決する
///
/// # Returns
/// * `Option<PathBuf>` - 取得できない場合は `None`
fn system_msiexec_path() -> Option<PathBuf> {
    get_known_folder_path(KnownFolder::System).map(|dir| dir.join("msiexec.exe"))
}

/// `msiexec` の引数を組み立てる
///
/// `msiexec` は `"NAME=値"` 形式を解釈しないため、値だけを引用符で囲む
fn msiexec_args(
    msi_path: &Path,
    application_folder: &Path,
    log_path: &Path,
) -> Result<String, UpdateError> {
    let quote = |path: &Path| match path.to_str() {
        Some(path) if !path.contains('"') && !path.ends_with('\\') => Ok(format!("\"{path}\"")),
        _ => Err(UpdateError::PathInvalid),
    };
    Ok(format!(
        "/i {} /qn /norestart APPLICATIONFOLDER={} /L*v {}",
        quote(msi_path)?,
        quote(application_folder)?,
        quote(log_path)?
    ))
}

/// `msiexec` の終了コードをインストール結果へ変換する
fn outcome_from_exit_code(code: i32) -> Result<InstallOutcome, UpdateError> {
    match code {
        EXIT_SUCCESS => Ok(InstallOutcome::Installed),
        EXIT_REBOOT_REQUIRED | EXIT_REBOOT_INITIATED => Ok(InstallOutcome::RebootRequired),
        EXIT_INSTALL_ALREADY_RUNNING => Err(UpdateError::MsiBusy),
        code => Err(UpdateError::MsiFailed { code }),
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// 実行フォルダの名前が `bin` の場合だけ、その親を導入先とすること
    #[test]
    fn application_folder_requires_bin_dir() {
        assert_eq!(
            application_folder(Path::new(r"D:\My Apps\edge-cut\bin")),
            Ok(PathBuf::from(r"D:\My Apps\edge-cut"))
        );
        assert_eq!(
            application_folder(Path::new(r"C:\Users\a\AppData\Local\edge-cut\BIN")),
            Ok(PathBuf::from(r"C:\Users\a\AppData\Local\edge-cut"))
        );
        // `cargo run` の出力先や複製先は拒否する
        for dir in [r"C:\src\edge-cut\target\debug", r"C:\copied", r"C:\bin"] {
            assert_eq!(
                application_folder(Path::new(dir)),
                Err(UpdateError::InstallLocationMismatch),
                "{dir}"
            );
        }
    }

    /// `WiX` の定義どおり、`bin` に本体と更新プログラムを置くこと
    #[test]
    fn wix_layout_matches_install_layout() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("wix/main.wxs");
        let wxs = fs::read_to_string(path).expect("main.wxs");
        assert!(wxs.contains(&format!("<Directory Id='Bin' Name='{BIN_DIR_NAME}'>")));
        for exe in [
            crate::install_layout::APP_EXE_NAME,
            crate::install_layout::UPDATER_EXE_NAME,
        ] {
            assert!(wxs.contains(&format!("Name='{exe}'")), "{exe}");
        }
        assert!(wxs.contains("<Directory Id='APPLICATIONFOLDER'"));
    }

    /// 空白を含むパスでも、値だけを引用符で囲むこと
    #[test]
    fn msiexec_args_quote_only_values() {
        let args = msiexec_args(
            Path::new(r"C:\Temp dir\app.msi"),
            Path::new(r"D:\My Apps\edge-cut"),
            Path::new(r"C:\logs\updater-msi.log"),
        );
        assert_eq!(
            args.as_deref(),
            Ok(concat!(
                r#"/i "C:\Temp dir\app.msi" /qn /norestart "#,
                r#"APPLICATIONFOLDER="D:\My Apps\edge-cut" "#,
                r#"/L*v "C:\logs\updater-msi.log""#
            ))
        );
    }

    /// 引用符や末尾の区切りを含むパスを拒否すること
    #[test]
    fn msiexec_args_reject_unsafe_path() {
        let log = Path::new("log");
        assert_eq!(
            msiexec_args(Path::new("a\" /x b.msi"), Path::new("app"), log),
            Err(UpdateError::PathInvalid)
        );
        assert_eq!(
            msiexec_args(Path::new("a.msi"), Path::new(r"D:\app\"), log),
            Err(UpdateError::PathInvalid)
        );
    }

    /// 終了コードから成功・再起動待ち・失敗を区別すること
    #[test]
    fn exit_codes_map_to_outcome() {
        assert_eq!(outcome_from_exit_code(0), Ok(InstallOutcome::Installed));
        for code in [3010, 1641] {
            assert_eq!(
                outcome_from_exit_code(code),
                Ok(InstallOutcome::RebootRequired)
            );
        }
        assert_eq!(outcome_from_exit_code(1618), Err(UpdateError::MsiBusy));
        assert_eq!(
            outcome_from_exit_code(1603),
            Err(UpdateError::MsiFailed { code: 1603 })
        );
    }

    /// `msiexec` をシステムフォルダの絶対パスで解決できるようにすること
    #[test]
    fn system_msiexec_path_is_absolute_and_exists() {
        let path = system_msiexec_path().expect("msiexec path");
        assert!(path.is_absolute());
        assert!(path.is_file(), "{}", path.display());
    }
}
