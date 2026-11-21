//! クリップボードへのコピー・貼り付け

use eframe::egui;

use crate::app::clipboard_worker::{ClipboardJob, ClipboardWorkerHandle};
use crate::app::state::App;

// ======================================================================
// クリップボード
// ======================================================================
impl App {
    /// クリップボード操作用ワーカーを返す。未起動ならここで起動する
    fn clipboard_worker(&mut self) -> &ClipboardWorkerHandle {
        let tx = self.io.tx.clone();
        self.clipboard
            .get_or_insert_with(|| ClipboardWorkerHandle::spawn(tx))
    }

    /// クリップボード操作をワーカーへ送る
    ///
    /// 通常はジョブ内で panic を捕捉するため、送信に失敗するのはワーカー自体の異常終了時のみ
    /// その場合はワーカーを起動し直して再送し、再送も失敗したらステータスに失敗を表示する
    ///
    /// # Arguments
    /// * `job` - ワーカーへ送るクリップボード操作
    fn submit_clipboard_job(&mut self, job: ClipboardJob) {
        let Err(job) = self.clipboard_worker().submit(job) else {
            return;
        };
        self.clipboard = None;
        if self.clipboard_worker().submit(job).is_err() {
            self.status = "クリップボード処理が異常終了しました".into();
        }
    }

    /// トリミング後の画像をバックグラウンドでクリップボードへコピーする
    ///
    /// 完了後は `poll_io` でステータスを更新する
    ///
    /// # Arguments
    /// * `ctx` - egui のコンテキスト (完了時の再描画に使用)
    pub(in crate::app) fn copy_trimmed_to_clipboard(&mut self, ctx: &egui::Context) {
        let Some((image, info)) = self.current_trim_target() else {
            return;
        };
        self.status = "クリップボードへコピー中…".into();
        let ctx = ctx.clone();
        self.submit_clipboard_job(ClipboardJob::Copy { image, info, ctx });
    }

    /// クリップボードの画像をバックグラウンドで読み込む
    ///
    /// 完了後は `poll_io` で元画像として設定する
    /// 読み込み中のファイルがあっても貼り付けを優先し、その結果は破棄する
    ///
    /// # Arguments
    /// * `ctx` - egui のコンテキスト (完了時の再描画に使用)
    pub(in crate::app) fn paste_from_clipboard(&mut self, ctx: &egui::Context) {
        self.io.abort_load();
        let request_id = self.io.load_request_id;
        self.status = "クリップボードから貼り付け中…".into();
        let ctx = ctx.clone();
        self.submit_clipboard_job(ClipboardJob::Paste { request_id, ctx });
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use eframe::egui;

    use crate::app::actions::test_helpers::{poll_io_until, trimmed_image_state};
    use crate::app::clipboard_worker::{ClipboardJob, ClipboardWorkerHandle};
    use crate::app::state::App;

    /// 再計算のデバウンス待ち中は前回の矩形でコピーしないこと
    #[test]
    fn copy_is_ignored_while_trim_is_pending() {
        let ctx = egui::Context::default();
        let mut app = App {
            image: trimmed_image_state(),
            ..App::default()
        };
        app.image.cache_key = Some((0, 0, None));
        app.status = "初期".into();

        // スライダー操作直後 (デバウンス待ち) の状態にする
        app.prefs.tolerance = 1;
        app.schedule_trim(&ctx, true, false, false);
        assert!(!app.has_current_trim());
        assert!(app.current_trim_target().is_none());

        // コピーは開始されずステータスも変わらない
        app.copy_trimmed_to_clipboard(&ctx);
        assert_eq!(app.status, "初期");
        assert!(app.clipboard.is_none());
    }

    /// クリップボードのワーカーが終了していた場合、起動し直して再送すること
    #[test]
    fn submit_clipboard_job_restarts_terminated_worker() {
        let ctx = egui::Context::default();
        let mut app = App {
            status: "初期".into(),
            clipboard: Some(ClipboardWorkerHandle::terminated()),
            ..App::default()
        };

        app.submit_clipboard_job(ClipboardJob::Paste {
            request_id: app.io.load_request_id,
            ctx: ctx.clone(),
        });

        // 失敗扱いにならず、起動し直したワーカーが結果を返す
        assert_ne!(app.status, "クリップボード処理が異常終了しました");
        let done = poll_io_until(&mut app, &ctx, |a| a.status != "初期");
        assert!(done, "再送されたジョブの結果を期待したが: {}", app.status);
    }
}
