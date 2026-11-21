//! プレビュートリミングのスケジュールと背景色の設定

use std::sync::Arc;
use std::sync::mpsc::TryRecvError;
use std::time::{Duration, Instant};

use eframe::egui;
use image::DynamicImage;

use crate::app::state::App;
use crate::app::trim_worker::TrimJob;
use crate::consts::TRIM_DEBOUNCE_MS;
use crate::trim::TrimInfo;

// ======================================================================
// トリミング
// ======================================================================
impl App {
    /// プレビュートリミングをスケジュールする
    ///
    /// # Arguments
    /// * `ctx` - egui のコンテキスト (再描画リクエストに使用)
    /// * `debounce` - `true` のとき短い待機後に実行 (スライダー連続操作向け)
    /// * `refresh_orig_tex` - `true` のとき元画像テクスチャもバックグラウンドで再生成する
    /// * `auto_save_after` - `true` のとき完了後に自動保存を試みる
    pub(in crate::app) fn schedule_trim(
        &mut self,
        ctx: &egui::Context,
        debounce: bool,
        refresh_orig_tex: bool,
        auto_save_after: bool,
    ) {
        if self.image.original.is_none() {
            return;
        }

        let key = (
            self.prefs.tolerance,
            self.prefs.padding,
            self.prefs.bg_override,
        );
        // 処理中のジョブは別の値で動いている可能性があるため、完了済みの結果と一致し、
        // かつ処理中・デバウンス待ちのジョブがない場合のみスキップする
        if self.image.info.is_some()
            && self.image.cache_key == Some(key)
            && !self.trim_worker.is_busy()
            && !refresh_orig_tex
        {
            if auto_save_after {
                self.auto_save_if_enabled(ctx);
            }
            return;
        }

        self.trim_worker.refresh_orig_tex |= refresh_orig_tex;
        self.trim_worker.auto_save_after |= auto_save_after;
        self.trim_worker.request_id = self.trim_worker.request_id.wrapping_add(1);

        if debounce {
            self.trim_worker.debounce_until =
                Some(Instant::now() + Duration::from_millis(TRIM_DEBOUNCE_MS));
        } else {
            self.trim_worker.debounce_until = None;
            self.start_trim_worker(ctx);
        }
        ctx.request_repaint();
    }

    /// デバウンス待ちとバックグラウンド結果を毎フレーム処理する
    ///
    /// ジョブが panic した場合やワーカーが終了した場合は失敗として処理中の状態を解除し、
    /// 古い結果で保存・コピーしないようトリミング結果も破棄する
    pub(in crate::app) fn poll_trim_work(&mut self, ctx: &egui::Context) {
        if let Some(until) = self.trim_worker.debounce_until {
            if Instant::now() >= until {
                self.trim_worker.debounce_until = None;
                self.start_trim_worker(ctx);
            } else {
                ctx.request_repaint();
            }
        }

        loop {
            let result = match self.trim_worker.result_rx.try_recv() {
                Ok(result) => result,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // 通常はジョブ内で panic を捕捉するため、ここに来るのはワーカー自体の異常終了
                    self.trim_worker.restart();
                    self.fail_trim("トリミング処理が異常終了しました".into());
                    break;
                }
            };
            if result.request_id != self.trim_worker.request_id {
                continue;
            }

            let preview = match result.outcome {
                Ok(preview) => preview,
                Err(msg) => {
                    self.trim_worker.in_flight = false;
                    self.fail_trim(format!("トリミング中に内部エラーが発生: {msg}"));
                    continue;
                }
            };
            if let Some(orig) = preview.orig_color {
                self.image.orig_tex =
                    Some(ctx.load_texture("orig", orig, egui::TextureOptions::LINEAR));
                self.trim_worker.refresh_orig_tex = false;
            }
            self.image.info = Some(preview.info);
            self.background.bg_color = Some(preview.background);
            self.image.cache_key = Some(preview.params);
            self.trim_worker.in_flight = false;

            if self.trim_worker.auto_save_after {
                self.trim_worker.auto_save_after = false;
                self.auto_save_if_enabled(ctx);
            }
        }
    }

    /// プレビュートリミングの失敗を反映する
    ///
    /// 前回の結果は現在の設定と一致しないため破棄し、設定を変えずに再実行してもスキップされないようにする
    ///
    /// # Arguments
    /// * `message` - ステータスバーに表示するメッセージ
    fn fail_trim(&mut self, message: String) {
        self.image.info = None;
        self.image.cache_key = None;
        self.trim_worker.auto_save_after = false;
        self.status = message;
    }

    /// 保存・コピー対象の元画像とトリミング矩形を返す
    ///
    /// 再計算のデバウンス待ち・処理中は `image.info` に前回の結果が残っているため対象にしない
    ///
    /// # Returns
    /// * `Option<(Arc<DynamicImage>, TrimInfo)>` - 現在の設定でトリミング済みなら元画像と矩形、
    ///   画像未読み込み・トリミング未完了・再計算中なら `None`
    pub(super) fn current_trim_target(&self) -> Option<(Arc<DynamicImage>, TrimInfo)> {
        if self.trim_worker.is_busy() {
            return None;
        }
        self.image.trim_target()
    }

    /// 現在の設定でのトリミング結果があり、保存・コピーできるかどうか
    pub(in crate::app) fn has_current_trim(&self) -> bool {
        self.image.info.is_some() && !self.trim_worker.is_busy()
    }

    fn start_trim_worker(&mut self, ctx: &egui::Context) {
        let Some(original) = self.image.original.clone() else {
            return;
        };

        let request_id = self.trim_worker.request_id;
        let tolerance = self.prefs.tolerance;
        let padding = self.prefs.padding;
        let bg_override = self.prefs.bg_override;
        // 結果が後続の要求で破棄されてもテクスチャを作り直せるよう、
        // フラグは送信時ではなくテクスチャを反映した時点で下ろす
        let refresh_orig_tex = self.trim_worker.refresh_orig_tex;
        self.trim_worker.in_flight = true;

        self.trim_worker.handle.submit(TrimJob {
            request_id,
            original,
            tolerance,
            padding,
            bg_override,
            refresh_orig_tex,
            ctx: ctx.clone(),
        });
    }
}

// ======================================================================
// 背景色
// ======================================================================
impl App {
    /// 背景色を自動検出モードに戻す
    pub(in crate::app) fn reset_bg_to_auto(&mut self, ctx: &egui::Context) {
        if self.prefs.bg_override.is_none() {
            return;
        }
        self.prefs.bg_override = None;
        self.background.pick_mode = false;
        self.image.invalidate_cache();
        self.schedule_trim(ctx, false, false, false);
    }

    /// 背景色を手動で設定する
    pub(in crate::app) fn set_manual_bg(&mut self, ctx: &egui::Context, color: [u8; 4]) {
        self.prefs.bg_override = Some(color);
        self.background.bg_color = Some(color);
        self.background.pick_mode = false;
        self.image.invalidate_cache();
        self.schedule_trim(ctx, false, false, false);
    }

    /// 元画像の指定座標から背景色をスポイトする
    pub(in crate::app) fn pick_bg_from_pixel(&mut self, ctx: &egui::Context, x: u32, y: u32) {
        let Some(ref original) = self.image.original else {
            return;
        };
        let Some(color) = crate::trim::sample_pixel(original, x, y) else {
            self.status = "ピクセル座標が画像の範囲外です".into();
            return;
        };
        self.background.pick_mode = false;
        self.prefs.bg_override = Some(color);
        self.background.bg_color = Some(color);
        self.image.invalidate_cache();
        self.status = format!(
            "背景色を設定: RGBA({}, {}, {}, {})",
            color[0], color[1], color[2], color[3]
        );
        self.schedule_trim(ctx, false, false, false);
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use eframe::egui;

    use crate::app::actions::test_helpers::{
        make_test_image, poll_trim_until_idle, trimmed_image_state,
    };
    use crate::app::state::{App, ImageState, TrimWorkerResult};

    // ======================================================================
    // schedule_trim() / poll_trim_work()
    // ======================================================================

    /// `schedule_trim()` がバックグラウンドでトリミング結果を UI へ反映すること
    #[test]
    fn schedule_trim_applies_result_in_background() {
        let ctx = eframe::egui::Context::default();
        let mut app = App {
            image: ImageState {
                original: Some(Arc::new(make_test_image())),
                ..ImageState::default()
            },
            ..App::default()
        };
        app.schedule_trim(&ctx, false, true, false);

        for _ in 0..50 {
            app.poll_trim_work(&ctx);
            if app.image.info.is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }

        assert!(
            app.image.trim_target().is_some(),
            "トリミング結果が反映されませんでした"
        );
        assert!(app.image.orig_tex.is_some());
        assert!(!app.trim_worker.in_flight);
    }

    /// 読み込み直後のトリミング中にパラメータを変えても元画像テクスチャが失われないこと
    #[test]
    fn schedule_trim_keeps_orig_tex_refresh_when_superseded() {
        let ctx = eframe::egui::Context::default();
        let mut app = App {
            image: ImageState {
                original: Some(Arc::new(make_test_image())),
                ..ImageState::default()
            },
            ..App::default()
        };
        // 読み込み時のテクスチャ再生成付き要求を、結果の受信前に別の要求で置き換える
        app.schedule_trim(&ctx, false, true, false);
        app.prefs.tolerance = 10;
        app.schedule_trim(&ctx, false, false, false);

        for _ in 0..50 {
            app.poll_trim_work(&ctx);
            if app.image.info.is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }

        // 最新の要求の結果でテクスチャが反映され、再生成要求は解除される
        assert!(app.image.info.is_some());
        assert!(app.image.orig_tex.is_some(), "元画像テクスチャが失われた");
        assert!(!app.trim_worker.refresh_orig_tex);
    }

    /// 処理中のジョブと異なる値へ戻した場合、完了済みの結果と一致してもスキップせず再計算すること
    #[test]
    fn schedule_trim_reruns_when_reverted_during_in_flight_job() {
        let ctx = egui::Context::default();
        let mut app = App {
            image: ImageState {
                original: Some(Arc::new(make_test_image())),
                ..ImageState::default()
            },
            ..App::default()
        };
        app.schedule_trim(&ctx, false, true, false);
        assert!(poll_trim_until_idle(&mut app, &ctx));
        assert_eq!(app.image.cache_key, Some((0, 0, None)));

        // tol=1 のジョブの結果が届く前に tol=0 へ戻す
        app.prefs.tolerance = 1;
        app.schedule_trim(&ctx, false, false, false);
        let in_flight_id = app.trim_worker.request_id;
        app.prefs.tolerance = 0;
        app.schedule_trim(&ctx, false, false, false);

        // tol=1 の結果を無効化する新しい要求が出ていること
        assert_ne!(app.trim_worker.request_id, in_flight_id);
        assert!(poll_trim_until_idle(&mut app, &ctx));
        assert_eq!(app.image.cache_key, Some((0, 0, None)));
    }

    /// `cache_key` に受信時点の UI の値ではなく、ジョブで使った値を記録すること
    #[test]
    fn poll_trim_work_records_job_params_as_cache_key() {
        let ctx = egui::Context::default();
        let mut app = App {
            image: ImageState {
                original: Some(Arc::new(make_test_image())),
                ..ImageState::default()
            },
            ..App::default()
        };
        app.prefs.tolerance = 1;
        app.schedule_trim(&ctx, false, true, false);
        // 再スケジュールせずに UI の値だけ変える
        app.prefs.tolerance = 0;

        assert!(poll_trim_until_idle(&mut app, &ctx));
        assert_eq!(app.image.cache_key, Some((1, 0, None)));
    }

    /// ジョブの panic による失敗結果を受け取ると、処理中を解除して古い結果を破棄すること
    #[test]
    fn poll_trim_work_recovers_from_failed_job() {
        let ctx = egui::Context::default();
        let mut app = App {
            image: trimmed_image_state(),
            ..App::default()
        };
        app.image.cache_key = Some((0, 0, None));
        let (tx, rx) = mpsc::channel();
        app.trim_worker.result_rx = rx;
        app.trim_worker.in_flight = true;
        app.trim_worker.auto_save_after = true;
        tx.send(TrimWorkerResult {
            request_id: app.trim_worker.request_id,
            outcome: Err("boom".into()),
        })
        .unwrap();

        app.poll_trim_work(&ctx);

        // 処理中が解除され、古い結果では保存・コピーできない
        assert!(!app.trim_worker.is_busy());
        assert!(!app.trim_worker.auto_save_after);
        assert!(app.image.info.is_none());
        assert!(app.image.cache_key.is_none());
        assert!(!app.has_current_trim());
        assert!(app.status.contains("boom"), "status: {}", app.status);
    }

    /// ワーカーが終了して結果チャネルが切断されると、処理中を解除してワーカーを再起動すること
    #[test]
    fn poll_trim_work_restarts_disconnected_worker() {
        let ctx = egui::Context::default();
        let mut app = App {
            image: trimmed_image_state(),
            ..App::default()
        };
        // 送信側を即座に破棄してワーカーの終了を再現する
        let (_, rx) = mpsc::channel();
        app.trim_worker.result_rx = rx;
        app.trim_worker.in_flight = true;

        app.poll_trim_work(&ctx);
        assert!(!app.trim_worker.is_busy());
        assert!(app.image.info.is_none());
        assert_eq!(app.status, "トリミング処理が異常終了しました");

        // 再起動したワーカーで再びトリミングできる
        app.schedule_trim(&ctx, false, false, false);
        assert!(poll_trim_until_idle(&mut app, &ctx));
        assert!(app.image.info.is_some());
    }
}
