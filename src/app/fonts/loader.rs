//! フォントの読み込みを提供
use std::fs;

use eframe::egui::FontData;

// ======================================================================
// システムフォント候補
// ======================================================================
/// Windowsシステムフォント候補(優先順位順)
#[cfg(target_os = "windows")]
const SYSTEM_JP_FONT_PATHS: &[&str] = &[
    r"C:\Windows\Fonts\msgothic.ttc",
    r"C:\Windows\Fonts\meiryo.ttc",
    r"C:\Windows\Fonts\YuGothR.ttc",
];

// ======================================================================
// フォント読み込み
// ======================================================================
/// 日本語システムフォントを読み込む
///
/// Windowsのシステムフォントフォルダから日本語フォントを検索する。
/// 見つからない場合は `None` を返す。
///
/// # Returns
/// * `Option<FontData>` - フォントデータ、または未発見の場合 `None`
#[cfg(target_os = "windows")]
pub fn load_japanese_font() -> Option<FontData> {
    for &path in SYSTEM_JP_FONT_PATHS {
        if let Ok(data) = fs::read(path) {
            return Some(FontData::from_owned(data));
        }
    }
    None
}

/// 日本語システムフォントを読み込む(非Windowsプラットフォーム)
///
/// # Returns
/// * `Option<FontData>` - 常に `None`(未実装)
#[cfg(not(target_os = "windows"))]
pub fn load_japanese_font() -> Option<FontData> {
    None
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// `load_japanese_font()` がパニックせずに呼び出せることを確認する
    #[test]
    fn load_japanese_font_does_not_panic() {
        let _ = load_japanese_font();
    }

    /// Windows 以外では常に None を返す
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn load_japanese_font_returns_none_on_non_windows() {
        assert!(load_japanese_font().is_none());
    }

    /// Windows ではフォントが見つかれば Some を返す(環境によるため、存在する場合のみテストが成功する)
    #[cfg(target_os = "windows")]
    #[test]
    fn load_japanese_font_returns_some_when_system_fonts_available() {
        // 開発環境には通常 MS Gothic/Meiryo/Yu Gothic のいずれかが存在する
        // フォントが見つからない環境では None となるが、パニックしてはならない
        let result = load_japanese_font();
        // 候補パスをすべて試してどれも読めない場合のみ None
        let any_path_exists = SYSTEM_JP_FONT_PATHS.iter().any(|p| Path::new(p).exists());
        if any_path_exists {
            assert!(
                result.is_some(),
                "フォントファイルが存在するのに読み込めませんでした"
            );
        } else {
            assert!(result.is_none());
        }
    }
}
