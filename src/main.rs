//! アプリケーションのエントリポイント

// リリースビルド時にコンソールウィンドウを表示しない (Windows)
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod app;
mod consts;
mod image_load;
mod image_output;
mod trim;

/// アプリケーションを起動する
///
/// # Returns
/// * `eframe::Result<()>` - 正常終了、またはウィンドウ生成エラー
fn main() -> eframe::Result<()> {
    app::run()
}
