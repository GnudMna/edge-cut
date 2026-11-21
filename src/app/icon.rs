//! アイコンを提供
use eframe::egui::IconData;

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
