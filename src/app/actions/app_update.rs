//! アプリ更新の確認・適用 (Windows)

use std::sync::mpsc::{self, TryRecvError};
use std::thread;

use eframe::egui;

use crate::app::state::{App, UpdateUiPhase, UpdateWorkerMsg};
use crate::update::{UpdateApplyOutcome, UpdateCheckOutcome, apply_update, check_for_update};

// ======================================================================
// アプリ更新
// ======================================================================

impl App {
    /// 更新ダイアログを開き、処理中でなければ更新確認を開始する
    ///
    /// # Arguments
    /// * `ctx` - 完了時の再描画に使う egui コンテキスト
    pub(in crate::app) fn open_update_dialog(&mut self, ctx: &egui::Context) {
        self.update.dialog_open = true;
        self.start_update_check(ctx);
    }

    /// 更新確認をバックグラウンドで開始する
    ///
    /// # Arguments
    /// * `ctx` - 完了時の再描画に使う egui コンテキスト
    pub(in crate::app) fn start_update_check(&mut self, ctx: &egui::Context) {
        if self.update.is_busy() {
            return;
        }

        let (tx, rx) = mpsc::channel();
        self.update.rx = Some(rx);
        self.update.phase = UpdateUiPhase::Checking;
        self.status = "更新を確認しています…".into();

        let ctx_clone = ctx.clone();
        thread::spawn(move || {
            let outcome = check_for_update();
            let _ = tx.send(UpdateWorkerMsg::Check(outcome));
            ctx_clone.request_repaint();
        });
    }

    /// 一括トリミングまたは保存が実行中か
    ///
    /// # Returns
    /// * `bool` - ファイルを書き込むバックグラウンド処理が残っていれば `true`
    pub(in crate::app) fn has_file_work(&self) -> bool {
        self.batch.running || self.io.is_saving()
    }

    /// 更新の適用中で、一括トリミングや保存を始められないか
    ///
    /// 適用中はインストーラー起動後にアプリが終了するため、新しいファイル書き込みを受け付けない
    /// 受け付けない場合はステータスで理由を知らせる
    ///
    /// # Returns
    /// * `bool` - 更新の適用中でファイル書き込みを始められなければ `true`
    pub(in crate::app) fn reject_file_work_while_updating(&mut self) -> bool {
        if !self.update.is_applying() {
            return false;
        }
        self.status = "更新の適用中は一括トリミングと保存を開始できません".into();
        true
    }

    /// 確認済みの更新をダウンロードして MSI を起動する
    ///
    /// 一括トリミングまたは保存の実行中は開始せず、ステータスで理由を知らせる
    ///
    /// # Arguments
    /// * `ctx` - 完了時の再描画に使う egui コンテキスト
    pub(in crate::app) fn start_update_apply(&mut self, ctx: &egui::Context) {
        if self.update.is_busy() {
            return;
        }
        // インストーラー起動後にアプリが終了するため、書き込み中の処理が残る間は適用しない
        if self.has_file_work() {
            self.status = "一括トリミングまたは保存の完了後に更新してください".into();
            return;
        }

        let update = match &self.update.phase {
            UpdateUiPhase::Available(u) => u.clone(),
            _ => return,
        };

        let (tx, rx) = mpsc::channel();
        self.update.rx = Some(rx);
        self.update.phase = UpdateUiPhase::Downloading {
            update: update.clone(),
        };
        self.status = format!("更新をダウンロードしています… (v{})", update.latest_version);

        let ctx_clone = ctx.clone();
        thread::spawn(move || {
            let outcome = apply_update(&update);
            let _ = tx.send(UpdateWorkerMsg::Apply(outcome));
            ctx_clone.request_repaint();
        });
    }

    /// バックグラウンドの更新処理結果をポーリングする
    ///
    /// 結果の前に送信側が切断された場合 (処理スレッドの panic など) は失敗として扱う
    /// MSI 起動後は、書き込み中の処理が残る間はウィンドウを閉じず、完了を待って閉じる
    ///
    /// # Arguments
    /// * `ctx` - MSI 起動後のウィンドウ終了に使う
    pub(in crate::app) fn poll_update(&mut self, ctx: &egui::Context) {
        if matches!(self.update.phase, UpdateUiPhase::MsiStarted) {
            if self.has_file_work() {
                self.status =
                    "インストーラーを起動しました。保存の完了後にアプリを終了します".into();
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        }
        let Some(rx) = self.update.rx.as_ref() else {
            return;
        };
        let msg = match rx.try_recv() {
            Ok(msg) => msg,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                self.update.rx = None;
                self.status = "更新処理が異常終了しました".into();
                self.update.phase = UpdateUiPhase::Error {
                    message: self.status.clone(),
                };
                return;
            }
        };
        self.update.rx = None;

        match msg {
            UpdateWorkerMsg::Check(outcome) => match outcome {
                UpdateCheckOutcome::UpToDate {
                    current_version,
                    latest_version,
                } => {
                    self.status =
                        format!("最新版です (v{current_version}、リモート v{latest_version})");
                    self.update.phase = UpdateUiPhase::UpToDate { current_version };
                }
                UpdateCheckOutcome::Available(update) => {
                    self.status = format!(
                        "更新があります: v{} → v{}",
                        update.current_version, update.latest_version
                    );
                    self.update.phase = UpdateUiPhase::Available(update);
                }
                UpdateCheckOutcome::Error { message } => {
                    self.status = format!("更新確認に失敗: {message}");
                    self.update.phase = UpdateUiPhase::Error { message };
                }
            },
            UpdateWorkerMsg::Apply(outcome) => match outcome {
                UpdateApplyOutcome::MsiStarted => {
                    self.status = "インストーラーを起動しました。アプリを終了します".into();
                    self.update.phase = UpdateUiPhase::MsiStarted;
                    // 書き込み中の処理が残っていれば、次回以降の `poll_update` で完了を待って閉じる
                    if !self.has_file_work() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
                UpdateApplyOutcome::Error { message } => {
                    self.status = format!("更新の適用に失敗: {message}");
                    self.update.phase = UpdateUiPhase::Error { message };
                }
            },
        }
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::thread;

    use eframe::egui;

    use crate::app::state::{App, UpdateUiPhase};
    use crate::update::AvailableUpdate;

    /// 結果の前に送信側が切断されると、更新処理を失敗として扱い再試行できること
    #[test]
    fn poll_update_fails_when_sender_disconnected() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        let (_, rx) = mpsc::channel();
        app.update.rx = Some(rx);
        app.update.phase = UpdateUiPhase::Checking;

        app.poll_update(&ctx);

        assert!(app.update.rx.is_none());
        assert!(matches!(app.update.phase, UpdateUiPhase::Error { .. }));
        assert!(!app.update.is_busy());
        assert_eq!(app.status, "更新処理が異常終了しました");
    }

    /// 一括トリミングの実行中は更新の適用を開始しないこと
    #[test]
    fn start_update_apply_is_blocked_while_batch_running() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.update.phase = UpdateUiPhase::Available(sample_update());
        app.batch.running = true;

        app.start_update_apply(&ctx);

        assert!(app.update.rx.is_none());
        assert!(matches!(app.update.phase, UpdateUiPhase::Available(_)));
        assert_eq!(
            app.status,
            "一括トリミングまたは保存の完了後に更新してください"
        );
    }

    /// 保存の実行中は更新の適用を開始しないこと
    #[test]
    fn start_update_apply_is_blocked_while_saving() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.update.phase = UpdateUiPhase::Available(sample_update());
        // 解放されるまで終わらない保存スレッドを模す
        let (release_tx, release_rx) = mpsc::channel::<()>();
        app.io.save_handles.push(thread::spawn(move || {
            let _ = release_rx.recv();
        }));

        app.start_update_apply(&ctx);

        assert!(app.update.rx.is_none());
        assert!(matches!(app.update.phase, UpdateUiPhase::Available(_)));

        // App の破棄で保存スレッドの完了を待つため、先に解放する
        drop(release_tx);
    }

    /// ダウンロード中は一括トリミングを開始せず、ダイアログも開かないこと
    #[test]
    fn batch_trim_is_rejected_while_downloading() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.update.phase = UpdateUiPhase::Downloading {
            update: sample_update(),
        };

        app.batch_trim(&ctx);

        assert!(!app.io.dialog_open);
        assert!(!app.batch.running);
        assert_eq!(
            app.status,
            "更新の適用中は一括トリミングと保存を開始できません"
        );
    }

    /// ダイアログを開いたまま適用が始まっても、選択後の一括トリミングを開始しないこと
    #[test]
    fn spawn_batch_trim_is_rejected_while_downloading() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.update.phase = UpdateUiPhase::Downloading {
            update: sample_update(),
        };

        app.spawn_batch_trim(&ctx, vec!["a.png".into()], 0, "out".into());

        assert!(!app.batch.running);
        assert!(app.batch.rx.is_none());
    }

    /// MSI 起動時に書き込み中の処理が残っていれば、ウィンドウを閉じずに待つこと
    #[test]
    fn poll_update_waits_for_file_work_after_msi_started() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.update.phase = UpdateUiPhase::MsiStarted;
        app.batch.running = true;

        app.poll_update(&ctx);

        assert!(matches!(app.update.phase, UpdateUiPhase::MsiStarted));
        assert_eq!(
            app.status,
            "インストーラーを起動しました。保存の完了後にアプリを終了します"
        );
    }

    /// 更新確認で得られる更新情報を模す
    fn sample_update() -> AvailableUpdate {
        AvailableUpdate {
            current_version: "1.0.0".into(),
            latest_version: "1.1.0".into(),
            tag: "v1.1.0".into(),
            asset_name: "EdgeCut.msi".into(),
            size: 1024,
            sha256: "0".repeat(64),
        }
    }
}
