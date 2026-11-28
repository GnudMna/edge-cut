//! 検証済みの配布物のインストールと、新しい版の起動
//!
//! MSI は `msiexec` で無人インストールし (`msi`)、ポータブル版は実行フォルダのファイルを差し替える (`portable`)。
//! どちらも本体の終了後に更新プログラムから呼ぶ

mod msi;
mod portable;

use std::path::Path;
use std::process::{Command, Stdio};

use super::error::UpdateError;
use crate::install_layout::APP_EXE_NAME;

pub(crate) use msi::{application_folder, install_msi};
pub(crate) use portable::install_portable;

// ======================================================================
// 起動
// ======================================================================
/// 実行フォルダの `EdgeCut.exe` を起動する
///
/// 更新プログラムとのパイプを引き継がせないよう、標準入出力は閉じて起動する
///
/// # Arguments
/// * `install_dir` - `EdgeCut.exe` のあるフォルダ
///
/// # Returns
/// * `Result<(), UpdateError>` - 起動できた場合は `Ok(())`
pub(crate) fn launch_app(install_dir: &Path) -> Result<(), UpdateError> {
    Command::new(install_dir.join(APP_EXE_NAME))
        .current_dir(install_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(drop)
        .map_err(|err| UpdateError::RestartFailed {
            detail: err.to_string(),
        })
}
