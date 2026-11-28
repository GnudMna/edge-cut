//! ウィンドウのアイコンを提供

use tao::window::Icon;

// ======================================================================
// アイコン読み込み
// ======================================================================
/// 埋め込みの PNG からウィンドウのアイコンを作る
///
/// # Returns
/// * `Option<Icon>` - アイコン。埋め込みの画像を変換できない場合は `None` (既定のアイコンになる)
pub(super) fn load_icon() -> Option<Icon> {
    let (rgba, width, height) = decode_icon()?;
    Icon::from_rgba(rgba, width, height).ok()
}

/// 埋め込みの PNG を RGBA に展開する
///
/// # Returns
/// * `Option<(Vec<u8>, u32, u32)>` - RGBA の画素列・幅・高さ
fn decode_icon() -> Option<(Vec<u8>, u32, u32)> {
    let icon_bytes = include_bytes!("../../assets/icons/icon.png");
    let image = image::load_from_memory(icon_bytes).ok()?.to_rgba8();
    let (width, height) = image.dimensions();
    Some((image.into_raw(), width, height))
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// 埋め込みのアイコンを展開でき、画素数が寸法と一致すること
    #[test]
    fn decode_icon_has_consistent_dimensions() {
        let (rgba, width, height) = decode_icon().expect("埋め込みのアイコンは有効な PNG");
        assert!(width > 0 && height > 0);
        assert_eq!(rgba.len(), (width * height * 4) as usize);
    }

    /// ウィンドウのアイコンを作れること
    #[test]
    fn load_icon_succeeds() {
        assert!(load_icon().is_some());
    }
}
