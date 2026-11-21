//! 一括トリミングのバックグラウンド処理の状態

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Instant;

/// 一括トリミングのバックグラウンド処理
pub(in crate::app) struct BatchState {
    /// バックグラウンドスレッドからの進捗受信チャネル
    pub rx: Option<std::sync::mpsc::Receiver<crate::batch_trim::BatchProgress>>,
    /// 処理が実行中かどうか
    pub running: bool,
    /// キャンセルフラグ。`true` にセットすると次のファイル処理前に停止する
    pub cancel: Arc<AtomicBool>,
    /// 実行中の一括トリミングスレッド。終了時に完了を待つ
    pub handle: Option<JoinHandle<()>>,
    /// 直近バッチのエラー詳細 (ステータスバーは要約のみ、全文はエラー一覧ウィンドウで表示)
    pub last_error_details: Vec<String>,
    /// エラー一覧ウィンドウを表示中かどうか
    pub errors_open: bool,
    /// 対象外として除外した入力ファイル数 (完了時の要約に追記する)
    pub skipped_inputs: usize,
    /// 終了時の待機期限 (`IoState` と共有する)
    pub shutdown_deadline: super::SharedDeadline,
}

impl Default for BatchState {
    fn default() -> Self {
        Self {
            rx: None,
            running: false,
            cancel: Arc::new(AtomicBool::new(false)),
            handle: None,
            last_error_details: Vec::new(),
            errors_open: false,
            skipped_inputs: 0,
            shutdown_deadline: super::SharedDeadline::default(),
        }
    }
}

impl BatchState {
    /// 未着手のファイルを打ち切り、処理中のファイルの保存完了を期限まで待つ
    ///
    /// # Arguments
    /// * `deadline` - 待つのをやめる時刻
    pub(in crate::app) fn shutdown(&mut self, deadline: Instant) {
        if let Some(handle) = self.handle.take() {
            self.cancel.store(true, Ordering::Relaxed);
            super::join_with_timeout(handle, deadline);
        }
    }
}

impl Drop for BatchState {
    /// 共有された期限まで完了を待つ
    fn drop(&mut self) {
        self.shutdown(super::shutdown_deadline(&self.shutdown_deadline));
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// 破棄時にキャンセルフラグを立て、一括トリミングスレッドの終了を待つこと
    #[test]
    fn batch_state_drop_cancels_and_joins_thread() {
        let mut batch = BatchState::default();
        let cancel = Arc::clone(&batch.cancel);
        let finished = Arc::new(AtomicBool::new(false));
        let finished_in_thread = Arc::clone(&finished);
        // キャンセルされるまで処理を続けるスレッドを模す
        batch.handle = Some(std::thread::spawn(move || {
            while !cancel.load(Ordering::Relaxed) {
                std::thread::yield_now();
            }
            finished_in_thread.store(true, Ordering::Relaxed);
        }));

        drop(batch);
        assert!(finished.load(Ordering::Relaxed));
    }
}
