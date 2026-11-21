//! 自動保存先の既定パスの決定
//!
//! OS のピクチャフォルダ (Windows の `FOLDERID_Pictures`・Linux の XDG `XDG_PICTURES_DIR`) を
//! 優先し、取得できない場合は `~/Pictures` → 実行ファイル隣の `output` の順に代替する。

use std::env;
#[cfg(target_os = "linux")]
use std::fs;
#[cfg(any(target_os = "linux", test))]
use std::path::Path;
use std::path::PathBuf;

#[cfg(windows)]
use known_folders::{KnownFolder, get_known_folder_path};

/// ピクチャフォルダ配下の作成先フォルダ名
const SAVE_DIR_NAME: &str = "EdgeCut";

// ======================================================================
// 既定パスの決定
// ======================================================================
/// 自動保存先の既定パスを決定する
///
/// # Returns
/// * `PathBuf` - `<ピクチャフォルダ>/EdgeCut`。ピクチャフォルダも `~/Pictures` も決められない場合は
///   実行ファイル隣の `output`
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
/// OS の設定を優先し、取得できない場合はホーム直下の `Pictures` とする
fn pictures_dir() -> Option<PathBuf> {
    os_pictures_dir().or_else(home_pictures_dir)
}

/// ホーム直下の `Pictures` を返す
fn home_pictures_dir() -> Option<PathBuf> {
    env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(|h| PathBuf::from(h).join("Pictures"))
}

// ======================================================================
// OS ごとのピクチャフォルダ
// ======================================================================
/// Windows のピクチャフォルダを返す
///
/// `OneDrive` へのリダイレクトやユーザーによる移動を反映するため、環境変数ではなく既知のフォルダから取得する
#[cfg(windows)]
fn os_pictures_dir() -> Option<PathBuf> {
    get_known_folder_path(KnownFolder::Pictures)
}

/// Linux のピクチャフォルダを XDG のユーザーディレクトリ設定から返す
///
/// `$XDG_CONFIG_HOME/user-dirs.dirs` (未設定時は `~/.config/user-dirs.dirs`) の
/// `XDG_PICTURES_DIR` を読む。日本語環境では `~/画像` などになる
#[cfg(target_os = "linux")]
fn os_pictures_dir() -> Option<PathBuf> {
    let home = PathBuf::from(env::var_os("HOME")?);
    let config_home = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".config"));
    let text = fs::read_to_string(config_home.join("user-dirs.dirs")).ok()?;
    parse_xdg_pictures_dir(&text, &home)
}

/// ピクチャフォルダの OS 設定を持たないプラットフォームでは `None` を返す
#[cfg(not(any(windows, target_os = "linux")))]
fn os_pictures_dir() -> Option<PathBuf> {
    None
}

// ======================================================================
// XDG user-dirs.dirs の解釈
// ======================================================================
/// `user-dirs.dirs` の内容から `XDG_PICTURES_DIR` を取り出す
///
/// 値は `"$HOME/..."` または `"/..."` の形式のみ受け付ける。同じキーが複数ある場合は
/// シェルが読み込む挙動に合わせて最後の定義を使う。
///
/// # Arguments
/// * `text` - `user-dirs.dirs` の内容
/// * `home` - `$HOME` に展開するパス
///
/// # Returns
/// * `Option<PathBuf>` - ピクチャフォルダ。未定義・書式不正・ホーム自身を指す場合 (無効化の表現) は `None`
#[cfg(any(target_os = "linux", test))]
fn parse_xdg_pictures_dir(text: &str, home: &Path) -> Option<PathBuf> {
    let value = text
        .lines()
        .rev()
        .find_map(|line| line.trim().strip_prefix("XDG_PICTURES_DIR="))?
        .trim()
        .strip_prefix('"')?
        .strip_suffix('"')?;

    let path = if let Some(rest) = value.strip_prefix("$HOME") {
        if rest.is_empty() {
            home.to_path_buf()
        } else {
            home.join(unescape(rest.strip_prefix('/')?))
        }
    } else if value.starts_with('/') {
        PathBuf::from(unescape(value))
    } else {
        return None;
    };

    (path != home).then_some(path)
}

/// ダブルクォート内のバックスラッシュエスケープを解除する
#[cfg(any(target_os = "linux", test))]
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            out.extend(chars.next());
        } else {
            out.push(c);
        }
    }
    out
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        PathBuf::from("/home/user")
    }

    /// `$HOME` 配下の日本語フォルダ名を解釈できること
    #[test]
    fn parses_home_relative_localized_dir() {
        let text = "XDG_DESKTOP_DIR=\"$HOME/デスクトップ\"\nXDG_PICTURES_DIR=\"$HOME/画像\"\n";
        assert_eq!(
            parse_xdg_pictures_dir(text, &home()),
            Some(home().join("画像"))
        );
    }

    /// 絶対パスの指定をそのまま返すこと
    #[test]
    fn parses_absolute_dir() {
        let text = "XDG_PICTURES_DIR=\"/mnt/data/Pictures\"\n";
        assert_eq!(
            parse_xdg_pictures_dir(text, &home()),
            Some(PathBuf::from("/mnt/data/Pictures"))
        );
    }

    /// コメント行を無視し、同じキーが複数ある場合は最後の定義を使うこと
    #[test]
    fn ignores_comments_and_uses_last_definition() {
        let text = "# XDG_PICTURES_DIR=\"$HOME/old\"\n\
                    XDG_PICTURES_DIR=\"$HOME/first\"\n\
                    XDG_PICTURES_DIR=\"$HOME/second\"\n";
        assert_eq!(
            parse_xdg_pictures_dir(text, &home()),
            Some(home().join("second"))
        );
    }

    /// バックスラッシュでエスケープされた文字を解除すること
    #[test]
    fn unescapes_backslashes() {
        let text = "XDG_PICTURES_DIR=\"$HOME/My \\\"Pics\\\"\"\n";
        assert_eq!(
            parse_xdg_pictures_dir(text, &home()),
            Some(home().join("My \"Pics\""))
        );
    }

    /// ホーム自身を指す設定 (ディレクトリの無効化) は採用しないこと
    #[test]
    fn rejects_home_itself() {
        assert_eq!(
            parse_xdg_pictures_dir("XDG_PICTURES_DIR=\"$HOME/\"\n", &home()),
            None
        );
        assert_eq!(
            parse_xdg_pictures_dir("XDG_PICTURES_DIR=\"$HOME\"\n", &home()),
            None
        );
    }

    /// 未定義・引用符なし・相対パスなど解釈できない値は `None` にすること
    #[test]
    fn rejects_missing_or_malformed_values() {
        assert_eq!(parse_xdg_pictures_dir("", &home()), None);
        assert_eq!(
            parse_xdg_pictures_dir("XDG_DESKTOP_DIR=\"$HOME/Desktop\"\n", &home()),
            None
        );
        assert_eq!(
            parse_xdg_pictures_dir("XDG_PICTURES_DIR=$HOME/Pictures\n", &home()),
            None
        );
        assert_eq!(
            parse_xdg_pictures_dir("XDG_PICTURES_DIR=\"Pictures\"\n", &home()),
            None
        );
        assert_eq!(
            parse_xdg_pictures_dir("XDG_PICTURES_DIR=\"$HOMEX/Pictures\"\n", &home()),
            None
        );
    }

    /// 既定の保存先が `EdgeCut` フォルダを指すこと
    #[test]
    fn default_dir_ends_with_app_folder_or_output() {
        let dir = default_auto_save_dir();
        assert!(dir.ends_with(SAVE_DIR_NAME) || dir.ends_with("output"));
    }
}
