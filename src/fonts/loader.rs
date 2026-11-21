//! フォントの読み込みを提供
use eframe::egui::FontData;

/// Windowsシステムフォント候補（優先順位順）
#[cfg(target_os = "windows")]
const SYSTEM_JP_FONT_PATHS: &[&str] = &[
    r"C:\Windows\Fonts\msgothic.ttc",
    r"C:\Windows\Fonts\meiryo.ttc",
    r"C:\Windows\Fonts\YuGothR.ttc",
];

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
        if let Ok(data) = std::fs::read(path) {
            return Some(FontData::from_owned(data));
        }
    }
    None
}

/// 日本語システムフォントを読み込む（非Windowsプラットフォーム）
///
/// # Returns
/// * `Option<FontData>` - 常に `None`（未実装）
#[cfg(not(target_os = "windows"))]
pub fn load_japanese_font() -> Option<FontData> {
    None
}
