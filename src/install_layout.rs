//! 配布形態 (MSI / ポータブル) ごとの実行ファイルとデータの配置
//!
//! 実行ファイルの隣に `portable` マーカーがあればポータブル版とし、設定などのデータを
//! 実行フォルダの `data` に置く。それ以外 (MSI 版・`cargo run`) は利用者のプロファイルに置く

use std::path::{Path, PathBuf};

use known_folders::{KnownFolder, get_known_folder_path};

use crate::consts::APP_NAME;

// ======================================================================
// 定数
// ======================================================================
/// 本体の実行ファイル名
pub(crate) const APP_EXE_NAME: &str = "EdgeCut.exe";
/// 更新プログラムの実行ファイル名
pub(crate) const UPDATER_EXE_NAME: &str = "EdgeCutUpdater.exe";
/// ポータブル版を示すマーカーファイル名 (実行ファイルの隣に置く)
pub(crate) const PORTABLE_MARKER_FILE: &str = "portable";
/// ポータブル版のデータフォルダ名 (実行フォルダ直下。更新で上書きしない)
pub(crate) const PORTABLE_DATA_DIR_NAME: &str = "data";

// ======================================================================
// 判定
// ======================================================================
/// 実行ファイルのあるフォルダを返す
///
/// # Returns
/// * `Option<PathBuf>` - 実行ファイルのパスを取得できない場合は `None`
pub(crate) fn current_exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(Path::to_path_buf)
}

/// フォルダがポータブル版の実行フォルダか
///
/// # Arguments
/// * `dir` - 実行ファイルのあるフォルダ
///
/// # Returns
/// * `bool` - `portable` マーカーがあれば `true`
pub(crate) fn is_portable_dir(dir: &Path) -> bool {
    dir.join(PORTABLE_MARKER_FILE).is_file()
}

/// ポータブル版で動いていれば、データフォルダ (`<実行フォルダ>\data`) を返す
///
/// # Returns
/// * `Option<PathBuf>` - ポータブル版でなければ `None`
pub(crate) fn portable_data_dir() -> Option<PathBuf> {
    current_exe_dir()
        .filter(|dir| is_portable_dir(dir))
        .map(|dir| dir.join(PORTABLE_DATA_DIR_NAME))
}

// ======================================================================
// データの保存先
// ======================================================================
/// 端末ごとのデータ (`WebView2` のユーザーデータ・ログ) を置くフォルダを返す
///
/// ポータブル版は `<実行フォルダ>\data`、それ以外は `%LOCALAPPDATA%\EdgeCut`
///
/// # Returns
/// * `Option<PathBuf>` - 既知のフォルダを取得できない場合は `None`
pub(crate) fn local_data_dir() -> Option<PathBuf> {
    portable_data_dir()
        .or_else(|| Some(get_known_folder_path(KnownFolder::LocalAppData)?.join(APP_NAME)))
}

/// 設定を置くフォルダを返す
///
/// ポータブル版は `<実行フォルダ>\data`、それ以外は `%APPDATA%\EdgeCut`
///
/// # Returns
/// * `Option<PathBuf>` - 既知のフォルダを取得できない場合は `None`
pub(crate) fn settings_dir() -> Option<PathBuf> {
    portable_data_dir()
        .or_else(|| Some(get_known_folder_path(KnownFolder::RoamingAppData)?.join(APP_NAME)))
}

/// 更新ログ (`msiexec` の詳細ログなど) を置くフォルダを返す
///
/// # Returns
/// * `Option<PathBuf>` - 既知のフォルダを取得できない場合は `None`
pub(crate) fn log_dir() -> Option<PathBuf> {
    Some(local_data_dir()?.join("logs"))
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    /// `portable` マーカーの有無でポータブル版を判定すること
    #[test]
    fn portable_dir_follows_marker() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert!(!is_portable_dir(tmp.path()));

        fs::write(tmp.path().join(PORTABLE_MARKER_FILE), b"").expect("marker");
        assert!(is_portable_dir(tmp.path()));
    }

    /// テスト実行ファイルの隣にマーカーは無いため、利用者のプロファイルを使うこと
    #[test]
    fn data_dirs_use_profile_outside_portable() {
        assert!(portable_data_dir().is_none());
        let local = local_data_dir().expect("local");
        assert!(local.ends_with(APP_NAME));
        assert!(log_dir().expect("log").starts_with(&local));
        assert!(settings_dir().expect("settings").ends_with(APP_NAME));
    }
}
