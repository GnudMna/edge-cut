//! アプリケーションモジュール
mod actions;
mod clipboard_worker;
mod fonts;
mod icon;
mod image_convert;
mod load_worker;
mod save_dir;
mod shortcuts;
mod state;
#[cfg(windows)]
mod title_bar;
mod trim_worker;
mod view;

use eframe::egui;

use crate::{consts, panic_hook};

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
    // コンソールの無いリリースビルドでも panic の原因を追えるよう、最初に登録する
    panic_hook::install();

    // 過去の更新で残った MSI は、起動を遅らせないようバックグラウンドで削除する
    #[cfg(windows)]
    std::thread::spawn(crate::update::cleanup_stale_update_dirs);

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
