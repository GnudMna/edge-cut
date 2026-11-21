//! アプリケーションのアクション (画像操作)

use std::fs;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::state::App;
use super::trim_worker::TrimJob;
use crate::batch_trim::{BatchProgress, BatchTrimParams, BatchTrimResult, run_batch_trim};
use crate::consts::{DEFAULT_IMAGE_EXTENSION, SUPPORTED_IMAGE_EXTENSIONS, TRIM_DEBOUNCE_MS};
use crate::image_load::{open_image, validate_clipboard_rgba};

use eframe::egui;
use image::DynamicImage;

// ======================================================================
// 画像読み込み
// ======================================================================
impl App {
    /// ファイルダイアログを開いて画像を読み込む
    ///
    /// # Arguments
    /// * `ctx` - egui のコンテキスト (テクスチャ登録に使用)
    pub(super) fn open_image(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("画像", SUPPORTED_IMAGE_EXTENSIONS)
            .pick_file()
        else {
            return;
        };
        self.load_image_from_path(ctx, &path);
    }

    /// パスから画像を読み込み、テクスチャ・ステータスを更新する
    ///
    /// # Arguments
    /// * `ctx`  - egui のコンテキスト (テクスチャ登録に使用)
    /// * `path` - 読み込む画像ファイルのパス
    pub(super) fn load_image_from_path(&mut self, ctx: &egui::Context, path: &Path) {
        match open_image(path) {
            Ok(img) => {
                self.image.prepare_for_load();
                self.status = format!(
                    "読み込み完了: {}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                );
                self.image.original = Some(Arc::new(img));
                self.background.on_image_loaded();
                self.schedule_trim(ctx, false, true, true);
            }
            Err(e) => self.status = e.to_string(),
        }
    }
}

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
    pub(super) fn schedule_trim(
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
            self.background.bg_override,
        );
        if self.image.trimmed.is_some() && self.image.cache_key == Some(key) && !refresh_orig_tex {
            if auto_save_after {
                self.auto_save_if_enabled();
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
    pub(super) fn poll_trim_work(&mut self, ctx: &egui::Context) {
        if let Some(until) = self.trim_worker.debounce_until {
            if Instant::now() >= until {
                self.trim_worker.debounce_until = None;
                self.start_trim_worker(ctx);
            } else {
                ctx.request_repaint();
            }
        }

        loop {
            let msg = self.trim_worker.result_rx.try_recv().ok();
            let Some(result) = msg else {
                break;
            };
            if result.request_id != self.trim_worker.request_id {
                continue;
            }

            if let Some(orig) = result.orig_color {
                self.image.orig_tex =
                    Some(ctx.load_texture("orig", orig, egui::TextureOptions::LINEAR));
            }
            self.image.trim_tex =
                Some(ctx.load_texture("trim", result.trimmed_color, egui::TextureOptions::LINEAR));
            self.image.trimmed = Some(result.trimmed);
            self.image.info = Some(result.info);
            self.background.bg_color = Some(result.background);
            self.image.cache_key = Some((
                self.prefs.tolerance,
                self.prefs.padding,
                self.background.bg_override,
            ));
            self.trim_worker.in_flight = false;

            if self.trim_worker.auto_save_after {
                self.trim_worker.auto_save_after = false;
                self.auto_save_if_enabled();
            }
        }
    }

    fn start_trim_worker(&mut self, ctx: &egui::Context) {
        let Some(original) = self.image.original.clone() else {
            return;
        };

        let request_id = self.trim_worker.request_id;
        let tolerance = self.prefs.tolerance;
        let padding = self.prefs.padding;
        let bg_override = self.background.bg_override;
        let refresh_orig_tex = self.trim_worker.refresh_orig_tex;
        self.trim_worker.refresh_orig_tex = false;
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
    pub(super) fn reset_bg_to_auto(&mut self, ctx: &egui::Context) {
        if self.background.bg_override.is_none() {
            return;
        }
        self.background.bg_override = None;
        self.background.pick_mode = false;
        self.image.invalidate_cache();
        self.schedule_trim(ctx, false, false, false);
    }

    /// 背景色を手動で設定する
    pub(super) fn set_manual_bg(&mut self, ctx: &egui::Context, color: [u8; 4]) {
        self.background.bg_override = Some(color);
        self.background.bg_color = Some(color);
        self.background.pick_mode = false;
        self.image.invalidate_cache();
        self.schedule_trim(ctx, false, false, false);
    }

    /// 元画像の指定座標から背景色をスポイトする
    pub(super) fn pick_bg_from_pixel(&mut self, ctx: &egui::Context, x: u32, y: u32) {
        let Some(ref original) = self.image.original else {
            return;
        };
        let Some(color) = crate::trim::sample_pixel(original, x, y) else {
            self.status = "ピクセル座標が画像の範囲外です".into();
            return;
        };
        self.background.pick_mode = false;
        self.background.bg_override = Some(color);
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
// 保存
// ======================================================================
impl App {
    /// ファイルダイアログを開いてトリミング後の画像を保存する
    pub(super) fn save_image(&mut self) {
        let Some(ref img) = self.image.trimmed else {
            return;
        };
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PNG", &["png"])
            .add_filter("JPEG", &["jpg", "jpeg"])
            .save_file()
        else {
            return;
        };
        match img.save(&path) {
            Ok(()) => self.status = format!("保存完了: {}", path.display()),
            Err(e) => self.status = format!("保存エラー: {e}"),
        }
    }

    /// 自動保存が有効な場合にトリミング後の画像を保存先フォルダへ保存する
    ///
    /// 成功時は既存のステータスメッセージに追記し、エラー時のみ上書きする
    pub(super) fn auto_save_if_enabled(&mut self) {
        if !self.prefs.auto_save {
            return;
        }
        let Some(ref img) = self.image.trimmed else {
            return;
        };
        if let Err(e) = fs::create_dir_all(&self.prefs.auto_save_dir) {
            self.status = format!("自動保存フォルダの作成に失敗: {e}");
            return;
        }
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let filename = format!("trimmed_{ts}.{DEFAULT_IMAGE_EXTENSION}");
        let path = self.prefs.auto_save_dir.join(&filename);
        match img.save(&path) {
            Ok(()) => {
                // 既存メッセージを上書きせず末尾に追記する
                self.status = format!("{}  (自動保存: {})", self.status, path.display());
            }
            Err(e) => self.status = format!("自動保存エラー: {e}"),
        }
    }
}

// ======================================================================
// クリア
// ======================================================================
impl App {
    /// 画像をすべてクリアして初期状態に戻す
    pub(super) fn clear_image(&mut self) {
        self.trim_worker.abort();
        self.image.clear();
        self.background.on_image_cleared();
        self.status = "「画像を開く」で画像を選択してください".into();
    }
}

// ======================================================================
// クリップボード
// ======================================================================
impl App {
    /// クリップボードアクセス用インスタンスを返す。未初期化ならここで作成する
    fn get_clipboard(&mut self) -> Result<&mut arboard::Clipboard, String> {
        if self.clipboard.is_none() {
            self.clipboard = Some(
                arboard::Clipboard::new()
                    .map_err(|e| format!("クリップボードの初期化に失敗しました: {e}"))?,
            );
        }
        Ok(self.clipboard.as_mut().unwrap())
    }

    /// トリミング後の画像をクリップボードへコピーする
    pub(super) fn copy_trimmed_to_clipboard(&mut self) {
        let Some(ref img) = self.image.trimmed else {
            return;
        };
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        let bytes = rgba.into_raw();
        let img_data = arboard::ImageData {
            width: w as usize,
            height: h as usize,
            bytes: bytes.into(),
        };
        match self.get_clipboard() {
            Ok(cb) => match cb.set_image(img_data) {
                Ok(()) => self.status = "クリップボードへコピーしました".into(),
                Err(e) => self.status = format!("コピーエラー: {e}"),
            },
            Err(msg) => self.status = msg,
        }
    }

    /// クリップボードから画像を読み込む
    ///
    /// # Arguments
    /// * `ctx` - egui のコンテキスト (テクスチャ登録に使用)
    pub(super) fn paste_from_clipboard(&mut self, ctx: &egui::Context) {
        let img_data = match self.get_clipboard() {
            Err(msg) => {
                self.status = msg;
                return;
            }
            Ok(cb) => {
                if let Ok(data) = cb.get_image() {
                    data
                } else {
                    self.status = "クリップボードに画像がありません".into();
                    return;
                }
            }
        };
        let Ok(w) = u32::try_from(img_data.width) else {
            self.status = "クリップボードの画像サイズが不正です".into();
            return;
        };
        let Ok(h) = u32::try_from(img_data.height) else {
            self.status = "クリップボードの画像サイズが不正です".into();
            return;
        };
        let bytes: Vec<u8> = img_data.bytes.into_owned();
        if let Err(e) = validate_clipboard_rgba(w, h, bytes.len()) {
            self.status = e.to_string();
            return;
        }
        match image::RgbaImage::from_raw(w, h, bytes) {
            Some(rgba) => {
                let img = DynamicImage::ImageRgba8(rgba);
                self.image.prepare_for_load();
                self.status = "クリップボードから貼り付け完了".into();
                self.image.original = Some(Arc::new(img));
                self.background.on_image_loaded();
                self.schedule_trim(ctx, false, true, true);
            }
            None => self.status = "クリップボード画像の変換に失敗しました".into(),
        }
    }
}

// ======================================================================
// 一括トリミング
// ======================================================================
impl App {
    /// 複数の画像ファイルを選択してまとめてトリミングし、指定フォルダへ保存する
    ///
    /// バックグラウンドスレッドで処理するため UI はブロックしない
    /// 完了後は `poll_batch_result` でステータスを更新する
    ///
    /// # Arguments
    /// * `ctx` - egui のコンテキスト (再描画リクエストに使用)
    pub(super) fn batch_trim(&mut self, ctx: &egui::Context) {
        if self.batch.running {
            return;
        }
        let Some(paths) = rfd::FileDialog::new()
            .add_filter("画像", SUPPORTED_IMAGE_EXTENSIONS)
            .pick_files()
        else {
            return;
        };
        let Some(out_dir) = rfd::FileDialog::new()
            .set_title("保存先フォルダを選択")
            .pick_folder()
        else {
            return;
        };

        let tolerance = self.prefs.tolerance;
        let padding = self.prefs.padding;
        let bg_override = self.background.bg_override;
        let ctx_clone = ctx.clone();
        let total = paths.len();

        self.batch.last_error_details.clear();
        self.batch.cancel.store(false, Ordering::Relaxed);
        let cancel_flag = Arc::clone(&self.batch.cancel);

        let (tx, rx) = mpsc::channel::<BatchProgress>();
        self.batch.rx = Some(rx);
        self.batch.running = true;
        self.status = format!("一括トリミング中... (0/{total})");

        thread::spawn(move || {
            if let Err(e) = fs::create_dir_all(&out_dir) {
                let result =
                    BatchTrimResult::fatal(out_dir, format!("出力フォルダの作成に失敗: {e}"));
                let _ = tx.send(BatchProgress::Finished(result));
                ctx_clone.request_repaint();
                return;
            }

            let result = run_batch_trim(
                &paths,
                &out_dir,
                BatchTrimParams {
                    tolerance,
                    padding,
                    bg_override,
                },
                &cancel_flag,
                |processed, batch_total| {
                    let _ = tx.send(BatchProgress::Working {
                        processed,
                        total: batch_total,
                    });
                    ctx_clone.request_repaint();
                },
            );

            let _ = tx.send(BatchProgress::Finished(result));
            ctx_clone.request_repaint();
        });
    }

    /// 実行中の一括トリミングをキャンセルする
    ///
    /// フラグをセットするのみ。進行中の 1 ファイルが完了したあと、次のファイル処理前に停止する
    pub(super) fn cancel_batch(&self) {
        self.batch.cancel.store(true, Ordering::Relaxed);
    }

    /// バックグラウンドの一括トリミング処理が完了していればステータスを更新する
    ///
    /// 毎フレーム呼び出すことで結果をポーリングする
    /// 進捗はステータスバーに表示し、エラー詳細は `batch.last_error_details` に保持する
    pub(super) fn poll_batch_result(&mut self) {
        if !self.batch.running {
            return;
        }
        let mut done = false;
        loop {
            let msg = self.batch.rx.as_ref().and_then(|rx| rx.try_recv().ok());
            match msg {
                Some(BatchProgress::Finished(result)) => {
                    self.status = result.summary();
                    self.batch.last_error_details = result.errors;
                    done = true;
                    break;
                }
                Some(BatchProgress::Working { processed, total }) => {
                    self.status = format!("一括トリミング中... ({processed}/{total})");
                }
                None => break,
            }
        }
        if done {
            self.batch.running = false;
            self.batch.rx = None;
        }
    }
}

// ======================================================================
// アプリ更新 (Windows)
// ======================================================================
#[cfg(windows)]
impl App {
    /// 更新確認をバックグラウンドで開始する
    ///
    /// # Arguments
    /// * `ctx` - 完了時の再描画に使う egui コンテキスト
    pub(super) fn start_update_check(&mut self, ctx: &egui::Context) {
        if self.update.is_busy() {
            return;
        }

        let (tx, rx) = mpsc::channel();
        self.update.rx = Some(rx);
        self.update.phase = super::state::UpdateUiPhase::Checking;
        self.status = "更新を確認しています…".into();

        let ctx_clone = ctx.clone();
        thread::spawn(move || {
            let outcome = crate::update::check_for_update();
            let _ = tx.send(super::state::UpdateWorkerMsg::Check(outcome));
            ctx_clone.request_repaint();
        });
    }

    /// 確認済みの更新をダウンロードして MSI を起動する
    ///
    /// # Arguments
    /// * `ctx` - 完了時の再描画に使う egui コンテキスト
    pub(super) fn start_update_apply(&mut self, ctx: &egui::Context) {
        if self.update.is_busy() {
            return;
        }

        let update = match &self.update.phase {
            super::state::UpdateUiPhase::Available(u) => u.clone(),
            _ => return,
        };

        let (tx, rx) = mpsc::channel();
        self.update.rx = Some(rx);
        self.update.phase = super::state::UpdateUiPhase::Downloading {
            update: update.clone(),
        };
        self.status = format!("更新をダウンロードしています… (v{})", update.latest_version);

        let ctx_clone = ctx.clone();
        thread::spawn(move || {
            let outcome = crate::update::apply_update(&update);
            let _ = tx.send(super::state::UpdateWorkerMsg::Apply(outcome));
            ctx_clone.request_repaint();
        });
    }

    /// バックグラウンドの更新処理結果をポーリングする
    ///
    /// # Arguments
    /// * `ctx` - MSI 起動後のウィンドウ終了に使う
    pub(super) fn poll_update(&mut self, ctx: &egui::Context) {
        let Some(rx) = self.update.rx.as_ref() else {
            return;
        };
        let Ok(msg) = rx.try_recv() else {
            return;
        };
        self.update.rx = None;

        match msg {
            super::state::UpdateWorkerMsg::Check(outcome) => match outcome {
                crate::update::UpdateCheckOutcome::UpToDate {
                    current_version,
                    latest_version,
                } => {
                    self.status =
                        format!("最新版です (v{current_version}、リモート v{latest_version})");
                    self.update.phase = super::state::UpdateUiPhase::UpToDate { current_version };
                }
                crate::update::UpdateCheckOutcome::Available(update) => {
                    self.status = format!(
                        "更新があります: v{} → v{}",
                        update.current_version, update.latest_version
                    );
                    self.update.phase = super::state::UpdateUiPhase::Available(update);
                }
                crate::update::UpdateCheckOutcome::Error { message } => {
                    self.status = format!("更新確認に失敗: {message}");
                    self.update.phase = super::state::UpdateUiPhase::Error { message };
                }
            },
            super::state::UpdateWorkerMsg::Apply(outcome) => match outcome {
                crate::update::UpdateApplyOutcome::MsiStarted => {
                    self.status = "インストーラーを起動しました。アプリを終了します".into();
                    self.update.phase = super::state::UpdateUiPhase::MsiStarted;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                crate::update::UpdateApplyOutcome::Error { message } => {
                    self.status = format!("更新の適用に失敗: {message}");
                    self.update.phase = super::state::UpdateUiPhase::Error { message };
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
    use std::env;
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::super::state::{App, AppPreferences, ImageState};

    use image::{DynamicImage, RgbaImage};

    // ======================================================================
    // テスト用のヘルパー関数
    // ======================================================================
    /// テスト用の単色 4×4 RGBA 画像を生成する
    fn make_test_image() -> DynamicImage {
        let buf = vec![200u8; 4 * 4 * 4];
        DynamicImage::ImageRgba8(RgbaImage::from_raw(4, 4, buf).unwrap())
    }

    // ======================================================================
    // clear_image()
    // ======================================================================
    /// `clear_image()` が元画像を None に設定することを確認する
    #[test]
    fn clear_image_sets_original_to_none() {
        let mut app = App {
            image: ImageState {
                original: Some(Arc::new(make_test_image())),
                ..ImageState::default()
            },
            ..App::default()
        };
        app.clear_image();
        assert!(app.image.original.is_none());
    }

    /// `clear_image()` がトリミング後の画像を None に設定することを確認する
    #[test]
    fn clear_image_sets_trimmed_to_none() {
        let mut app = App {
            image: ImageState {
                trimmed: Some(make_test_image()),
                ..ImageState::default()
            },
            ..App::default()
        };
        app.clear_image();
        assert!(app.image.trimmed.is_none());
    }

    /// `clear_image()` がテクスチャを None に設定することを確認する
    #[test]
    fn clear_image_sets_info_to_none() {
        let mut app = App {
            image: ImageState {
                info: Some(crate::trim::TrimInfo {
                    orig_w: 10,
                    orig_h: 10,
                    trim_w: 8,
                    trim_h: 8,
                    offset_x: 1,
                    offset_y: 1,
                }),
                ..ImageState::default()
            },
            ..App::default()
        };
        app.clear_image();
        assert!(app.image.info.is_none());
    }

    /// `clear_image()` がステータスメッセージを初期状態にリセットすることを確認する
    #[test]
    fn clear_image_resets_status_message() {
        let mut app = App {
            status: "何か別のメッセージ".into(),
            ..App::default()
        };
        app.clear_image();
        assert!(!app.status.is_empty());
        // デフォルトメッセージに戻っていることを確認
        assert!(app.status.contains("画像を開く"));
    }

    /// `clear_image()` が画像がない状態でもパニックせずに動作することを確認する
    #[test]
    fn clear_image_from_initial_state_does_not_panic() {
        let mut app = App::default();
        // 既に空の状態でも安全に動作すること
        app.clear_image();
        assert!(app.image.original.is_none());
        assert!(app.image.trimmed.is_none());
    }

    // ======================================================================
    // auto_save_if_enabled()
    // ======================================================================
    /// `auto_save_if_enabled()` が自動保存が無効な場合に何もしないことを確認する
    #[test]
    fn auto_save_skips_when_disabled() {
        let mut app = App {
            prefs: AppPreferences {
                auto_save: false,
                ..AppPreferences::default()
            },
            ..App::default()
        };
        let status_before = app.status.clone();
        app.auto_save_if_enabled();
        assert_eq!(app.status, status_before);
    }

    /// `auto_save_if_enabled()` がトリミング後の画像がない場合に何もしないことを確認する
    #[test]
    fn auto_save_skips_when_no_trimmed_image() {
        let mut app = App {
            prefs: AppPreferences {
                auto_save: true,
                ..AppPreferences::default()
            },
            ..App::default()
        };
        let status_before = app.status.clone();
        app.auto_save_if_enabled();
        assert_eq!(app.status, status_before);
    }

    /// `auto_save_if_enabled()` が自動保存が有効でトリミング後の画像がある場合に PNG ファイルを保存することを確認する
    #[test]
    fn auto_save_writes_png_file_to_dir() {
        let tmp = env::temp_dir().join(format!(
            "edge_cut_test_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_millis())
        ));
        let mut app = App {
            prefs: AppPreferences {
                auto_save: true,
                auto_save_dir: tmp.clone(),
                ..AppPreferences::default()
            },
            image: ImageState {
                trimmed: Some(make_test_image()),
                ..ImageState::default()
            },
            ..App::default()
        };
        app.auto_save_if_enabled();

        // ステータスが自動保存パスを含むこと
        assert!(app.status.contains("自動保存"), "status = {}", app.status);

        // 実際にファイルが生成されていること
        let saved_files: Vec<_> = fs::read_dir(&tmp)
            .map(|rd| rd.filter_map(std::result::Result::ok).collect())
            .unwrap_or_default();
        assert!(
            !saved_files.is_empty(),
            "自動保存フォルダにファイルが存在しない"
        );

        // クリーンアップ
        let _ = fs::remove_dir_all(&tmp);
    }

    /// `auto_save_if_enabled()` がディレクトリ作成に失敗した場合にエラーステータスを設定することを確認する
    #[test]
    fn auto_save_error_status_when_dir_creation_fails() {
        // 既存ファイルをディレクトリとして扱おうとすると create_dir_all が失敗する
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let existing_file = env::temp_dir().join(format!("ec_blocker_{ts}.bin"));
        fs::write(&existing_file, b"x").expect("テスト用ファイルの作成に失敗");

        // ファイルのパス配下にサブディレクトリを作ろうとするので失敗する
        let mut app = App {
            prefs: AppPreferences {
                auto_save: true,
                auto_save_dir: existing_file.join("subdir_cannot_exist"),
                ..AppPreferences::default()
            },
            image: ImageState {
                trimmed: Some(make_test_image()),
                ..ImageState::default()
            },
            ..App::default()
        };
        app.auto_save_if_enabled();

        let _ = fs::remove_file(&existing_file);

        assert!(
            app.status.contains("失敗") || app.status.contains("エラー"),
            "エラーメッセージを期待したが: {}",
            app.status
        );
    }

    // ======================================================================
    // load_image_from_path (エラーブランチ)
    // ======================================================================
    /// `load_image_from_path()` が存在しないファイルを指定した場合にエラーステータスを設定することを確認する
    #[test]
    fn load_image_from_path_sets_error_status_for_missing_file() {
        let ctx = eframe::egui::Context::default();
        let mut app = App::default();
        let bad_path = Path::new("/nonexistent/path/image.png");
        app.load_image_from_path(&ctx, bad_path);
        assert!(
            app.status.contains("読み込みエラー"),
            "status = {}",
            app.status
        );
    }

    /// `load_image_from_path()` がエラー時に元画像を設定しないことを確認する
    #[test]
    fn load_image_from_path_does_not_set_original_on_error() {
        let ctx = eframe::egui::Context::default();
        let mut app = App::default();
        let bad_path = Path::new("/nonexistent/path/image.png");
        app.load_image_from_path(&ctx, bad_path);
        assert!(app.image.original.is_none());
    }

    // ======================================================================
    // schedule_trim() / poll_trim_work()
    // ======================================================================
    /// `schedule_trim()` がバックグラウンドでトリミング結果を UI へ反映することを確認する
    #[test]
    fn schedule_trim_applies_result_in_background() {
        use std::thread;
        use std::time::Duration;

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
            if app.image.trimmed.is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }

        assert!(
            app.image.trimmed.is_some(),
            "トリミング結果が反映されませんでした"
        );
        assert!(app.image.trim_tex.is_some());
        assert!(app.image.orig_tex.is_some());
        assert!(!app.trim_worker.in_flight);
    }
}
