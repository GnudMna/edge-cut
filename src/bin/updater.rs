//! 更新プログラムのエントリポイント

// リリースビルド時にコンソールウィンドウを表示しない
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

/// 更新プログラムを起動する
///
/// # Returns
/// * `ExitCode` - 更新画面を表示できたら成功
fn main() -> ExitCode {
    edge_cut::app::run_updater()
}
