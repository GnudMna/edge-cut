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

    /// 確認済みの更新をダウンロードして MSI を起動する
    ///
    /// # Arguments
    /// * `ctx` - 完了時の再描画に使う egui コンテキスト
    pub(in crate::app) fn start_update_apply(&mut self, ctx: &egui::Context) {
        if self.update.is_busy() {
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
    ///
    /// # Arguments
    /// * `ctx` - MSI 起動後のウィンドウ終了に使う
    pub(in crate::app) fn poll_update(&mut self, ctx: &egui::Context) {
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
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
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

    use eframe::egui;

    use crate::app::state::{App, UpdateUiPhase};

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
}
