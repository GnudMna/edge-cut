#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! アプリケーションのエントリポイント
mod app;
mod consts;
mod fonts;
mod trim;

/// アプリケーションを起動する
///
/// # Returns
/// * `eframe::Result<()>` - 正常終了、またはウィンドウ生成エラー
fn main() -> eframe::Result<()> {
    app::run()
}
