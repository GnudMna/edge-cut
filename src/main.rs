//! アプリケーションのエントリポイント

// リリースビルド時にコンソールウィンドウを表示しない (Windows)
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

/// アプリケーションを起動する
///
/// # Returns
/// * `eframe::Result<()>` - 正常終了、またはウィンドウ生成エラー
fn main() -> eframe::Result<()> {
    edge_cut::app::run()
}
