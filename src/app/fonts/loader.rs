//! フォントの読み込みを提供

use std::fs;

use eframe::egui::FontData;

// ======================================================================
// システムフォント候補
// ======================================================================
/// Windows システムフォント候補 (優先順位順)
#[cfg(target_os = "windows")]
const SYSTEM_JP_FONT_PATHS: &[&str] = &[
    r"C:\Windows\Fonts\msgothic.ttc",
    r"C:\Windows\Fonts\meiryo.ttc",
    r"C:\Windows\Fonts\YuGothR.ttc",
];

/// macOS システムフォント候補 (優先順位順)
#[cfg(target_os = "macos")]
const SYSTEM_JP_FONT_PATHS: &[&str] = &[
    "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
];

/// Linux システムフォント候補 (優先順位順)
#[cfg(target_os = "linux")]
const SYSTEM_JP_FONT_PATHS: &[&str] = &[
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/opentype/ipafont-gothic/ipag.ttf",
    "/usr/share/fonts/truetype/fonts-japanese-gothic.ttf",
    "/usr/share/fonts/truetype/takao-gothic/TakaoGothic.ttf",
];

// ======================================================================
// フォント読み込み
// ======================================================================
/// 日本語システムフォントを読み込む
///
/// 各 OS のシステムフォントフォルダから日本語フォントを検索する
/// 見つからない場合は `None` を返す
///
/// # Returns
/// * `Option<FontData>` - フォントデータ、または未発見の場合 `None`
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
pub fn load_japanese_font() -> Option<FontData> {
    for &path in SYSTEM_JP_FONT_PATHS {
        if let Ok(data) = fs::read(path) {
            return Some(FontData::from_owned(data));
        }
    }
    None
}

/// 日本語システムフォントを読み込む (未対応プラットフォーム)
///
/// # Returns
/// * `Option<FontData>` - 常に `None`
#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
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

    /// `load_japanese_font()` がパニックせずに呼び出せること
    #[test]
    fn load_japanese_font_does_not_panic() {
        let _ = load_japanese_font();
    }

    /// システムフォントが存在する環境では `Some` を返すこと
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    #[test]
    fn load_japanese_font_returns_some_when_system_fonts_available() {
        let result = load_japanese_font();
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

    /// 未対応プラットフォームでは常に `None` を返すこと
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    #[test]
    fn load_japanese_font_returns_none_on_unsupported_platform() {
        assert!(load_japanese_font().is_none());
    }
}
