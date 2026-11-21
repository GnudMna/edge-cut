//! アプリケーションモジュール
mod actions;
mod bundled;
mod clipboard_worker;
mod fonts;
mod icon;
mod image_convert;
mod load_worker;
mod save_dir;
mod shortcuts;
mod state;
mod theme;
mod title_bar;
mod trim_worker;
mod view;

use eframe::egui;

use crate::{consts, panic_hook};

/// 日本語フォントが見つからなかった場合にステータスバーへ表示するメッセージ
const FONT_MISSING_MESSAGE: &str =
    "日本語フォントが見つからないため、文字が正しく表示されない場合があります";

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
    std::thread::spawn(crate::update::cleanup_stale_update_dirs);

    // 数 MB のフォントファイルの読み込みを、ウィンドウ・GPU の初期化と並行して行う
    let font_loader = std::thread::spawn(fonts::load_system_fonts);

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
            // フォントの追加 (読み込みスレッドが panic した場合は全て未発見として扱う)
            let fonts = font_loader.join().unwrap_or_default();
            let font_registered = fonts::register_system_fonts(&cc.egui_ctx, fonts);
            theme::install(&cc.egui_ctx);

            let mut app = state::App::new(cc.storage);
            if !font_registered {
                app.status = FONT_MISSING_MESSAGE.into();
            }
            Ok(Box::new(app))
        }),
    )
}
