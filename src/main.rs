//! アプリケーションのエントリポイント

// リリースビルド時にコンソールウィンドウを表示しない
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// アプリケーションを起動する
///
/// # Returns
/// * `eframe::Result<()>` - 正常終了、またはウィンドウ生成エラー
fn main() -> eframe::Result<()> {
    edge_cut::app::run()
}
