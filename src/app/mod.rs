//! アプリケーションモジュール
mod actions;
mod icon;
mod state;
mod view;

use crate::consts;
use crate::fonts;

use eframe::egui;

/// アプリケーションの実行
///
/// # Returns
/// * `eframe::Result` - ネイティブアプリの実行結果
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
            fonts::regist_japanese_font(&cc.egui_ctx);

            Ok(Box::new(state::App::new()))
        }),
    )
}
