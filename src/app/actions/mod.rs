//! アプリケーションのアクション (画像操作)
//!
//! `App` への操作を責務ごとのサブモジュールに分けて実装する。
//! ファイルダイアログ・読み込み・トリミング・保存・クリップボード・一括処理・ドロップ・更新を扱う。

mod app_update;
mod batch;
mod clipboard;
mod dialog;
mod drop;
mod io_result;
mod load;
mod save;
#[cfg(test)]
pub(super) mod test_helpers;
mod trim;
