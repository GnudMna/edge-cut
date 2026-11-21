//! アイコンを提供
use eframe::egui::IconData;

// ======================================================================
// アイコン読み込み
// ======================================================================
/// アイコン画像を読み込み
///
/// # Returns
/// * `IconData` - アイコンのデータ
pub fn load_icon() -> IconData {
    let icon_bytes = include_bytes!("../../assets/icons/icon.png");
    let image = image::load_from_memory(icon_bytes).expect("Failed to load icon");
    let image = image.to_rgba8();
    let (width, height) = image.dimensions();
    IconData {
        rgba: image.into_raw(),
        width,
        height,
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// `load_icon()` が有効なアイコンデータを返すことを確認する
    #[test]
    fn load_icon_has_nonzero_dimensions() {
        let icon = load_icon();
        assert!(icon.width > 0);
        assert!(icon.height > 0);
    }

    /// `load_icon()` が RGBA データの長さが幅と高さに基づいて正しいことを確認する
    #[test]
    fn load_icon_rgba_length_matches_dimensions() {
        let icon = load_icon();
        let expected = (icon.width * icon.height * 4) as usize;
        assert_eq!(icon.rgba.len(), expected);
    }

    /// `load_icon()` がパニックせずに呼び出せることを確認する
    #[test]
    fn load_icon_does_not_panic() {
        // include_bytes! で埋め込んだバイト列が有効な PNG であること
        let _ = load_icon();
    }
}
