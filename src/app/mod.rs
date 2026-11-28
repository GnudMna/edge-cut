//! アプリケーションモジュール
//!
//! tao のウィンドウに wry の `WebView` を載せ、`TypeScript` (`React`) の UI を表示する。
//! 状態と処理は Rust 側 (`state` / `actions` / 各ワーカー) が持ち、UI とは `ipc` のコマンドと
//! 画面状態でやり取りする。イベントループは `event_loop` に置く。
//! 更新プログラム (`EdgeCutUpdater.exe`) の画面は `updater` が同じ UI を使って表示する

mod actions;
mod bundled;
mod clipboard_worker;
mod event_loop;
mod icon;
mod image_convert;
mod ipc;
mod load_worker;
mod protocol;
mod save_dir;
mod settings_store;
mod single_instance;
mod state;
mod trim_worker;
mod updater;
mod waker;

use std::error::Error;

use crate::panic_hook;

pub use updater::run_updater;

// ======================================================================
// アプリケーション起動
// ======================================================================
/// アプリケーションの実行
///
/// # Returns
/// * `Result<(), Box<dyn Error>>` - 正常終了、またはウィンドウ・`WebView` の初期化エラー
///
/// # Errors
/// ウィンドウまたは `WebView` (`WebView2` ランタイム) の初期化に失敗した場合
pub fn run() -> Result<(), Box<dyn Error>> {
    // コンソールの無いリリースビルドでも panic の原因を追えるよう、最初に登録する
    panic_hook::install();

    // 設定の保存・更新プログラムの終了依頼・作業フォルダの掃除が成り立つよう、本体は 1 つに限る。
    // ロックを取れない (別のウィンドウが起動中) 場合は、掃除などを行わずに知らせて終了する
    let _instance = match single_instance::acquire() {
        Ok(Some(guard)) => Some(guard),
        Ok(None) => {
            // 既存のウィンドウを前面に出せれば十分。見つからないとき (起動直後など) だけ知らせる
            if !single_instance::activate_existing_window() {
                rfd::MessageDialog::new()
                    .set_title("EdgeCut")
                    .set_level(rfd::MessageLevel::Info)
                    .set_description("EdgeCut はすでに起動しています。")
                    .show();
            }
            return Ok(());
        }
        // ロックの仕組みが使えない環境では、多重起動を防げなくても起動を妨げない
        Err(err) => {
            eprintln!("多重起動の防止を無効にして起動する: {err}");
            None
        }
    };

    // 過去の更新で残った作業フォルダ (配布物・更新プログラムのコピー) は、起動を遅らせないようバックグラウンドで削除する
    std::thread::spawn(crate::update::cleanup_stale_update_dirs);

    event_loop::run()
}
