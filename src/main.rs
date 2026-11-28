//! アプリケーションのエントリポイント

// リリースビルド時にコンソールウィンドウを表示しない
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

/// アプリケーションを起動する
///
/// 起動に失敗した場合 (`WebView2` ランタイムが無いなど) はダイアログで知らせる
///
/// # Returns
/// * `ExitCode` - 正常終了なら成功、起動に失敗したら失敗
fn main() -> ExitCode {
    match edge_cut::app::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            rfd::MessageDialog::new()
                .set_title("EdgeCut")
                .set_level(rfd::MessageLevel::Error)
                .set_description(startup_error_message(&e.to_string()))
                .show();
            ExitCode::FAILURE
        }
    }
}

/// 起動に失敗したときに表示する文面を作る
///
/// `WebView2` ランタイムが見つからない場合だけ、導入の確認を案内する
///
/// # Arguments
/// * `detail` - 起動エラーの詳細
///
/// # Returns
/// * `String` - ダイアログに表示する文面
fn startup_error_message(detail: &str) -> String {
    let hint = if wry::webview_version().is_err() {
        "\n\nMicrosoft Edge WebView2 ランタイムがインストールされているか確認してください。"
    } else {
        ""
    };
    format!("EdgeCut を起動できませんでした。{hint}\n\n詳細: {detail}")
}
