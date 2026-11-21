//! ファイルダイアログの起動と結果の処理

use std::thread;

use eframe::egui;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

use crate::app::state::{App, DialogResult, IoMsg};
use crate::panic_message::catch_panic;

// ======================================================================
// ファイルダイアログ
// ======================================================================
impl App {
    /// ファイルダイアログの親ウィンドウを設定する
    ///
    /// 親を指定したダイアログはモーダルになり、親ウィンドウの裏に隠れない
    ///
    /// # Arguments
    /// * `parent` - ダイアログの親にするウィンドウ (アプリのメインウィンドウ)
    pub(in crate::app) fn set_dialog_parent<W>(&mut self, parent: &W)
    where
        W: HasWindowHandle + HasDisplayHandle,
    {
        self.io.dialog_template = rfd::FileDialog::new().set_parent(parent);
    }

    /// ファイルダイアログを別スレッドで開き、閉じられたときに結果を `IoMsg::DialogClosed` で通知する
    ///
    /// UI スレッドをブロックしないため、ダイアログ表示中も描画とワーカー結果の取り込みが続く
    /// 別のダイアログを表示中は開かない。結果は `poll_io` で処理する
    /// `job` にはメインウィンドウを親に設定したダイアログを渡し、ダイアログがウィンドウの裏に隠れないようにする
    ///
    /// # Arguments
    /// * `ctx` - egui のコンテキスト (完了時の再描画に使用)
    /// * `job` - 渡されたダイアログを開いて結果を返す処理
    ///
    /// # Returns
    /// * `bool` - ダイアログを開始した場合は `true`、別のダイアログを表示中で見送った場合は `false`
    pub(super) fn spawn_dialog<F>(&mut self, ctx: &egui::Context, job: F) -> bool
    where
        F: FnOnce(rfd::FileDialog) -> DialogResult + Send + 'static,
    {
        if self.io.dialog_open {
            self.status = "ファイルダイアログを表示中です".into();
            return false;
        }
        self.io.dialog_open = true;
        let dialog = self.io.dialog_template.clone();
        let tx = self.io.tx.clone();
        let ctx_clone = ctx.clone();
        thread::spawn(move || {
            // panic しても表示中の状態が残らないよう、失敗として結果を通知する
            let result = catch_panic(|| job(dialog)).unwrap_or_else(|msg| {
                DialogResult::Failed(format!("ファイルダイアログでエラーが発生しました: {msg}"))
            });
            let _ = tx.send(IoMsg::DialogClosed(result));
            ctx_clone.request_repaint();
        });
        true
    }

    /// ファイルダイアログの結果に応じて後続の処理を行う
    ///
    /// # Arguments
    /// * `ctx` - egui のコンテキスト
    /// * `result` - ダイアログの結果
    pub(super) fn handle_dialog_result(&mut self, ctx: &egui::Context, result: DialogResult) {
        self.io.dialog_open = false;
        match result {
            DialogResult::OpenImage(path) => {
                if let Some(path) = path {
                    self.load_image_from_path(ctx, &path);
                }
            }
            DialogResult::SaveImage { request, path } => {
                if let Some(path) = path {
                    self.start_manual_save(ctx, request, path);
                }
            }
            DialogResult::BatchInputs(paths) => {
                if let Some(paths) = paths {
                    self.start_batch_trim(ctx, paths, 0);
                }
            }
            DialogResult::BatchOutput {
                paths,
                skipped,
                dir,
            } => {
                if let Some(dir) = dir {
                    self.spawn_batch_trim(ctx, paths, skipped, dir);
                }
            }
            DialogResult::AutoSaveDir(dir) => {
                if let Some(dir) = dir {
                    self.prefs.auto_save_dir = dir;
                }
            }
            DialogResult::Failed(msg) => self.status = msg,
        }
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::thread;
    use std::time::Duration;

    use eframe::egui;

    use crate::app::actions::test_helpers::{poll_io_until, trimmed_image_state};
    use crate::app::state::{App, DialogResult, IoMsg, SaveRequest};

    // ======================================================================
    // ファイルダイアログ結果の受信
    // ======================================================================
    /// 結果を `IoMsg::DialogClosed` として送り、ダイアログ表示中の状態で `poll_io()` する
    fn poll_dialog_result(app: &mut App, ctx: &egui::Context, result: DialogResult) {
        app.io.dialog_open = true;
        app.io.tx.send(IoMsg::DialogClosed(result)).unwrap();
        app.poll_io(ctx);
    }

    /// 自動保存先フォルダの選択結果を設定へ反映し、キャンセル時は変更しないこと
    #[test]
    fn dialog_result_updates_auto_save_dir() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        let original = app.prefs.auto_save_dir.clone();

        // キャンセルでは保存先を変えず、ダイアログ表示中の状態は解除する
        poll_dialog_result(&mut app, &ctx, DialogResult::AutoSaveDir(None));
        assert_eq!(app.prefs.auto_save_dir, original);
        assert!(!app.io.dialog_open);

        // 選択された場合は保存先を更新する
        let chosen = PathBuf::from("chosen_dir");
        poll_dialog_result(
            &mut app,
            &ctx,
            DialogResult::AutoSaveDir(Some(chosen.clone())),
        );
        assert_eq!(app.prefs.auto_save_dir, chosen);
        assert!(!app.io.dialog_open);
    }

    /// 画像選択のキャンセルでは何も読み込まず、表示中の状態を解除すること
    #[test]
    fn dialog_result_open_image_cancel_does_nothing() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        let status_before = app.status.clone();
        let request_id = app.io.load_request_id;

        poll_dialog_result(&mut app, &ctx, DialogResult::OpenImage(None));

        assert!(!app.io.dialog_open);
        assert_eq!(app.status, status_before);
        assert_eq!(app.io.load_request_id, request_id);
    }

    /// ダイアログ処理の失敗をステータスへ表示し、表示中の状態を解除すること
    #[test]
    fn dialog_result_failed_reports_status() {
        let ctx = egui::Context::default();
        let mut app = App::default();

        poll_dialog_result(
            &mut app,
            &ctx,
            DialogResult::Failed("ダイアログ失敗".into()),
        );

        assert_eq!(app.status, "ダイアログ失敗");
        assert!(!app.io.dialog_open);
    }

    /// 保存先が選ばれると、ダイアログを開いた時点の内容で保存し、拡張子なしなら補うこと
    #[test]
    fn dialog_result_save_image_writes_file() {
        let ctx = egui::Context::default();
        let tmp = tempfile::tempdir().unwrap();
        let mut app = App::default();
        let state = trimmed_image_state();
        let request = SaveRequest {
            original: state.original.clone().unwrap(),
            info: state.info.unwrap(),
            icc_profile: None,
            dpi: None,
            ext: "png".into(),
        };

        // 拡張子なしの保存先は設定の形式の拡張子を補う
        let path = tmp.path().join("out");
        poll_dialog_result(
            &mut app,
            &ctx,
            DialogResult::SaveImage {
                request,
                path: Some(path),
            },
        );

        let done = poll_io_until(&mut app, &ctx, |a| a.status.contains("保存完了"));
        assert!(done, "status = {}", app.status);
        assert!(tmp.path().join("out.png").is_file());
    }

    /// 保存先のキャンセルでは保存を開始しないこと
    #[test]
    fn dialog_result_save_image_cancel_does_not_save() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        let state = trimmed_image_state();
        let request = SaveRequest {
            original: state.original.clone().unwrap(),
            info: state.info.unwrap(),
            icc_profile: None,
            dpi: None,
            ext: "png".into(),
        };

        poll_dialog_result(
            &mut app,
            &ctx,
            DialogResult::SaveImage {
                request,
                path: None,
            },
        );

        assert!(!app.io.is_saving());
        assert!(!app.io.dialog_open);
    }

    /// ダイアログ表示中は新しいダイアログを開かないこと
    #[test]
    fn dialog_is_not_opened_twice() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.io.dialog_open = true;

        app.open_image(&ctx);

        // 追加のダイアログスレッドが結果を送らず、表示中のままであること
        thread::sleep(Duration::from_millis(50));
        assert!(app.io.rx.try_recv().is_err());
        assert!(app.io.dialog_open);
        assert!(
            app.status.contains("ダイアログを表示中"),
            "status = {}",
            app.status
        );
    }
}
