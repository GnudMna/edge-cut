//! アプリケーションモジュール
mod actions;
mod clipboard_worker;
mod fonts;
mod icon;
mod image_convert;
mod load_worker;
mod shortcuts;
mod state;
mod trim_worker;
mod view;

use crate::consts;

use eframe::egui;

// ======================================================================
// アプリケーション起動
// ======================================================================
/// アプリケーションの実行
///
/// # Returns
/// * `eframe::Result` - ネイティブアプリの実行結果
///
/// # Errors
/// ネイティブウィンドウの初期化またはイベントループの実行に失敗した場合
pub fn run() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(consts::APP_NAME)
            .with_inner_size(consts::DEFAULT_WINDOW_SIZE)
            .with_min_inner_size(consts::MIN_WINDOW_SIZE)
            .with_drag_and_drop(true)
            .with_icon(icon::load_icon()),
        ..Default::default()
    };
    eframe::run_native(
        consts::APP_NAME,
        options,
        Box::new(|cc| {
            // フォントの追加
            fonts::register_japanese_font(&cc.egui_ctx);

            Ok(Box::new(state::App::new(cc.storage)))
        }),
    )
}
