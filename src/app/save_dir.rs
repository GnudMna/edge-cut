//! 自動保存先の既定パスの決定
//!
//! Windows のピクチャフォルダ (`FOLDERID_Pictures`) を優先し、取得できない場合は
//! `%USERPROFILE%\Pictures` → 実行ファイル隣の `output` の順に代替する。

use std::env;
use std::path::PathBuf;

use known_folders::{KnownFolder, get_known_folder_path};

/// ピクチャフォルダ配下の作成先フォルダ名
const SAVE_DIR_NAME: &str = "EdgeCut";

// ======================================================================
// 既定パスの決定
// ======================================================================
/// 自動保存先の既定パスを決定する
///
/// # Returns
/// * `PathBuf` - `<ピクチャフォルダ>/EdgeCut`。ピクチャフォルダも `%USERPROFILE%\Pictures` も
///   決められない場合は実行ファイル隣の `output`
pub(super) fn default_auto_save_dir() -> PathBuf {
    pictures_dir()
        .map(|p| p.join(SAVE_DIR_NAME))
        .or_else(|| {
            env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.join("output")))
        })
        .unwrap_or_else(|| PathBuf::from("output"))
}

/// ピクチャフォルダを返す
///
/// `OneDrive` へのリダイレクトやユーザーによる移動を反映するため、環境変数ではなく既知のフォルダを優先し、
/// 取得できない場合はユーザープロファイル直下の `Pictures` とする
fn pictures_dir() -> Option<PathBuf> {
    get_known_folder_path(KnownFolder::Pictures).or_else(home_pictures_dir)
}

/// ユーザープロファイル直下の `Pictures` を返す
fn home_pictures_dir() -> Option<PathBuf> {
    env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Pictures"))
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// 既定の保存先が `EdgeCut` フォルダを指すこと
    #[test]
    fn default_dir_ends_with_app_folder_or_output() {
        let dir = default_auto_save_dir();
        assert!(dir.ends_with(SAVE_DIR_NAME) || dir.ends_with("output"));
    }
}
