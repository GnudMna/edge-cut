//! アプリ更新の確認と、更新プログラムの起動
//!
//! 本体は更新の有無を確かめ、更新するときは更新プログラム (`EdgeCutUpdater.exe`) を起動する。
//! ダウンロードからインストールまでは更新プログラムが進め、終了を依頼されたら保存の完了を待って終了する

use std::sync::mpsc::{self, TryRecvError};
use std::thread;

use crate::app::state::{App, UpdateUiPhase, UpdateWorkerMsg};
use crate::install_layout;
use crate::update::{
    UpdateCheckOutcome, UpdatePackageKind, UpdaterEvent, check_for_update, launch_updater,
};

// ======================================================================
// アプリ更新
// ======================================================================

impl App {
    /// 更新ダイアログを開き、処理中でなければ更新確認を開始する
    ///
    /// # Arguments
    pub(in crate::app) fn open_update_dialog(&mut self) {
        self.update.dialog_open = true;
        self.start_update_check();
    }

    /// 更新確認をバックグラウンドで開始する
    ///
    /// 実行フォルダの配布形態 (MSI / ポータブル) に合う配布物で確かめる
    ///
    /// # Arguments
    pub(in crate::app) fn start_update_check(&mut self) {
        if self.update.is_busy() {
            return;
        }

        let (tx, rx) = mpsc::channel();
        self.update.rx = Some(rx);
        self.update.phase = UpdateUiPhase::Checking;
        self.status = "更新を確認しています…".into();

        let waker = self.waker.clone();
        thread::spawn(move || {
            let kind = install_layout::current_exe_dir().map_or(UpdatePackageKind::Msi, |dir| {
                UpdatePackageKind::for_install_dir(&dir)
            });
            let outcome = check_for_update(kind);
            let _ = tx.send(UpdateWorkerMsg::Check(outcome));
            waker.wake();
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
    /// 更新プログラムから終了を依頼された後は、新しいファイル書き込みを受け付けない
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

    /// 更新プログラムを起動する
    ///
    /// 一括トリミングまたは保存の実行中は開始せず、ステータスで理由を知らせる
    ///
    /// # Arguments
    pub(in crate::app) fn start_update_apply(&mut self) {
        if self.update.is_busy() {
            return;
        }
        if self.has_file_work() {
            self.status = "一括トリミングまたは保存の完了後に更新してください".into();
            return;
        }
        let UpdateUiPhase::Available(update) = &self.update.phase else {
            return;
        };
        let update = update.clone();

        let Some(install_dir) = install_layout::current_exe_dir() else {
            self.fail_update("実行ファイルの場所を取得できません".into());
            return;
        };
        let log_dir = install_layout::log_dir().unwrap_or_else(std::env::temp_dir);
        let waker = self.waker.clone();
        match launch_updater(&install_dir, &log_dir, move || waker.wake()) {
            Ok(session) => {
                self.update.session = Some(session);
                self.update.phase = UpdateUiPhase::UpdaterRunning(update);
                self.update.dialog_open = false;
                self.status = "更新プログラムを起動しました".into();
            }
            Err(err) => self.fail_update(err.to_string()),
        }
    }

    /// 更新の失敗を表示する
    fn fail_update(&mut self, message: String) {
        self.status = format!("更新を開始できません: {message}");
        self.update.phase = UpdateUiPhase::Error { message };
    }

    /// 更新確認の結果と更新プログラムからの知らせを取り込む
    ///
    /// 結果の前に送信側が切断された場合 (処理スレッドの panic など) は失敗として扱う
    /// 終了を依頼された後は、書き込み中の処理が残る間はウィンドウを閉じず、完了を待って閉じる
    ///
    /// # Arguments
    pub(in crate::app) fn poll_update(&mut self) {
        self.poll_updater_session();
        if matches!(self.update.phase, UpdateUiPhase::Exiting) {
            if self.has_file_work() {
                self.status = "更新のため、保存の完了後にアプリを終了します".into();
            } else {
                self.close_requested = true;
            }
            return;
        }
        self.poll_update_check();
    }

    /// 更新プログラムからの知らせを取り込む
    fn poll_updater_session(&mut self) {
        let Some(session) = &self.update.session else {
            return;
        };
        let mut events = Vec::new();
        while let Some(event) = session.poll() {
            events.push(event);
            if event == UpdaterEvent::Closed {
                break;
            }
        }
        for event in events {
            match event {
                UpdaterEvent::ExitRequested => {
                    self.update.phase = UpdateUiPhase::Exiting;
                    self.update.dialog_open = true;
                    self.status = "更新のためアプリを終了します".into();
                }
                UpdaterEvent::Closed => self.on_updater_closed(),
            }
        }
    }

    /// 更新プログラムが閉じたときに、更新前の状態へ戻す
    fn on_updater_closed(&mut self) {
        self.update.session = None;
        self.update.phase = match std::mem::take(&mut self.update.phase) {
            UpdateUiPhase::UpdaterRunning(update) => {
                self.status = "更新プログラムを閉じました".into();
                UpdateUiPhase::Available(update)
            }
            UpdateUiPhase::Exiting => {
                let message = "更新プログラムが終了したため、更新を中止しました".to_string();
                self.status.clone_from(&message);
                UpdateUiPhase::Error { message }
            }
            other => other,
        };
    }

    /// 更新確認の結果を取り込む
    fn poll_update_check(&mut self) {
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

        let UpdateWorkerMsg::Check(outcome) = msg;
        match outcome {
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

    use crate::app::state::{App, UpdateUiPhase};
    use crate::update::AvailableUpdate;

    /// 結果の前に送信側が切断されると、更新処理を失敗として扱い再試行できること
    #[test]
    fn poll_update_fails_when_sender_disconnected() {
        let mut app = App::default();
        let (_, rx) = mpsc::channel();
        app.update.rx = Some(rx);
        app.update.phase = UpdateUiPhase::Checking;

        app.poll_update();

        assert!(app.update.rx.is_none());
        assert!(matches!(app.update.phase, UpdateUiPhase::Error { .. }));
        assert!(!app.update.is_busy());
        assert_eq!(app.status, "更新処理が異常終了しました");
    }

    /// 一括トリミングの実行中は更新プログラムを起動しないこと
    #[test]
    fn start_update_apply_is_blocked_while_batch_running() {
        let mut app = App::default();
        app.update.phase = UpdateUiPhase::Available(sample_update());
        app.batch.running = true;

        app.start_update_apply();

        assert!(app.update.session.is_none());
        assert!(matches!(app.update.phase, UpdateUiPhase::Available(_)));
        assert_eq!(
            app.status,
            "一括トリミングまたは保存の完了後に更新してください"
        );
    }

    /// 保存の実行中は更新プログラムを起動しないこと
    #[test]
    fn start_update_apply_is_blocked_while_saving() {
        let mut app = App::default();
        app.update.phase = UpdateUiPhase::Available(sample_update());
        // 解放されるまで終わらない保存スレッドを模す
        let (release_tx, release_rx) = mpsc::channel::<()>();
        app.io.save_handles.push(thread::spawn(move || {
            let _ = release_rx.recv();
        }));

        app.start_update_apply();

        assert!(app.update.session.is_none());
        assert!(matches!(app.update.phase, UpdateUiPhase::Available(_)));

        // App の破棄で保存スレッドの完了を待つため、先に解放する
        drop(release_tx);
    }

    /// 更新プログラムの無い実行フォルダ (テスト実行時) では、起動せずに失敗を表示すること
    #[test]
    fn start_update_apply_reports_missing_updater() {
        let mut app = App::default();
        app.update.phase = UpdateUiPhase::Available(sample_update());

        app.start_update_apply();

        assert!(app.update.session.is_none());
        assert!(matches!(app.update.phase, UpdateUiPhase::Error { .. }));
    }

    /// 終了の依頼後は一括トリミングを開始せず、ダイアログも開かないこと
    #[test]
    fn batch_trim_is_rejected_while_exiting() {
        let mut app = App::default();
        app.update.phase = UpdateUiPhase::Exiting;

        app.batch_trim();

        assert!(!app.io.dialog_open);
        assert!(!app.batch.running);
        assert_eq!(
            app.status,
            "更新の適用中は一括トリミングと保存を開始できません"
        );
    }

    /// ダイアログを開いたまま終了を依頼されても、選択後の一括トリミングを開始しないこと
    #[test]
    fn spawn_batch_trim_is_rejected_while_exiting() {
        let mut app = App::default();
        app.update.phase = UpdateUiPhase::Exiting;

        app.spawn_batch_trim(vec!["a.png".into()], 0, "out".into());

        assert!(!app.batch.running);
        assert!(app.batch.rx.is_none());
    }

    /// 終了を依頼されたとき、書き込み中の処理が残っていればウィンドウを閉じずに待つこと
    #[test]
    fn poll_update_waits_for_file_work_while_exiting() {
        let mut app = App::default();
        app.update.phase = UpdateUiPhase::Exiting;
        app.batch.running = true;

        app.poll_update();

        assert!(!app.close_requested);
        assert_eq!(app.status, "更新のため、保存の完了後にアプリを終了します");

        // 書き込みが終われば閉じる
        app.batch.running = false;
        app.poll_update();
        assert!(app.close_requested);
    }

    /// 終了を依頼する前に更新プログラムが閉じたら、更新ありの状態へ戻ること
    #[test]
    fn updater_closed_restores_available() {
        let mut app = App::default();
        app.update.phase = UpdateUiPhase::UpdaterRunning(sample_update());

        app.on_updater_closed();

        assert!(matches!(app.update.phase, UpdateUiPhase::Available(_)));
        assert!(!app.update.is_busy());
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
