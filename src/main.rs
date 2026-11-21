//! アプリケーションのエントリポイント

// Windowsのサブシステムを使用して、コンソールウィンドウを表示しないようにする
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Windows以外のプラットフォームでビルドしようとした場合、コンパイルエラーを発生させる
#[cfg(not(target_os = "windows"))]
compile_error!("This crate supports only Windows.");

mod app;
mod consts;
mod trim;

/// アプリケーションを起動する
///
/// # Returns
/// * `eframe::Result<()>` - 正常終了、またはウィンドウ生成エラー
fn main() -> eframe::Result<()> {
    app::run()
}
