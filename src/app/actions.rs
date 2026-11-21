//! アプリケーションのアクション (画像操作)

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use super::state::{App, IoMsg, SaveKind};
use super::trim_worker::TrimJob;
use crate::batch_trim::{BatchProgress, BatchTrimParams, BatchTrimResult, run_batch_trim};
use crate::consts::{
    DEFAULT_IMAGE_EXTENSION, SUPPORTED_IMAGE_EXTENSIONS, TRIM_DEBOUNCE_MS,
    is_supported_image_extension,
};
use crate::image_load::{open_image, validate_clipboard_rgba};
use crate::image_output;

use eframe::egui;
use image::DynamicImage;

// ======================================================================
// 定数
// ======================================================================
/// 元画像名を取得できないときの自動保存ファイル名
const AUTO_SAVE_FALLBACK_STEM: &str = "image";

/// クリップボードから貼り付けた画像の自動保存ファイル名
const CLIPBOARD_SOURCE_STEM: &str = "clipboard";

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

    /// パスから画像をバックグラウンドで読み込む
    ///
    /// 完了後は `poll_io` でテクスチャ・ステータスを更新する
    /// 読み込み中も直前の画像は表示したままにする
    ///
    /// # Arguments
    /// * `ctx`  - egui のコンテキスト (完了時の再描画に使用)
    /// * `path` - 読み込む画像ファイルのパス
    pub(super) fn load_image_from_path(&mut self, ctx: &egui::Context, path: &Path) {
        self.io.abort_load();
        let request_id = self.io.load_request_id;
        let file_name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        self.status = format!("読み込み中: {file_name}");

        let path = path.to_path_buf();
        let tx = self.io.tx.clone();
        let ctx_clone = ctx.clone();
        thread::spawn(move || {
            let result = open_image(&path).map_err(|e| e.to_string());
            let _ = tx.send(IoMsg::Loaded {
                request_id,
                file_name,
                result,
            });
            ctx_clone.request_repaint();
        });
    }

    /// 読み込み済みの画像を元画像として設定し、プレビュートリミングを開始する
    ///
    /// # Arguments
    /// * `ctx` - egui のコンテキスト (トリミングのスケジュールに使用)
    /// * `img` - 元画像
    /// * `status` - 設定するステータスメッセージ
    /// * `source_stem` - 自動保存のファイル名に使う元画像名 (拡張子なし)
    fn set_original_image(
        &mut self,
        ctx: &egui::Context,
        img: DynamicImage,
        status: String,
        source_stem: String,
    ) {
        self.image.prepare_for_load();
        self.status = status;
        self.image.original = Some(Arc::new(img));
        self.image.source_stem = Some(source_stem);
        self.background.on_image_loaded();
        self.schedule_trim(ctx, false, true, true);
    }
}

// ======================================================================
// 画像 I/O の結果受信
// ======================================================================
impl App {
    /// バックグラウンドの読み込み・保存結果を毎フレーム処理する
    ///
    /// # Arguments
    /// * `ctx` - egui のコンテキスト (読み込み完了後のトリミングに使用)
    pub(super) fn poll_io(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.io.rx.try_recv() {
            match msg {
                IoMsg::Loaded {
                    request_id,
                    file_name,
                    result,
                } => {
                    // 後続の読み込み・貼り付け・クリアで無効化された結果は捨てる
                    if request_id != self.io.load_request_id {
                        continue;
                    }
                    let note = self.io.load_note.take().unwrap_or_default();
                    match result {
                        Ok(img) => {
                            let status = format!("読み込み完了: {file_name}{note}");
                            let stem = Path::new(&file_name).file_stem().map_or_else(
                                || AUTO_SAVE_FALLBACK_STEM.into(),
                                |s| s.to_string_lossy().into_owned(),
                            );
                            self.set_original_image(ctx, img, status, stem);
                        }
                        Err(msg) => self.status = format!("{msg}{note}"),
                    }
                }
                IoMsg::Saved { kind, result } => match (kind, result) {
                    (SaveKind::Manual, Ok(path)) => {
                        self.status = format!("保存完了: {}", path.display());
                    }
                    (SaveKind::Auto, Ok(path)) => {
                        // 既存メッセージを上書きせず末尾に追記する
                        self.status = format!("{}  (自動保存: {})", self.status, path.display());
                    }
                    (_, Err(msg)) => self.status = msg,
                },
            }
        }
        self.io.save_handles.retain(|h| !h.is_finished());
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
            self.image.trimmed = Some(Arc::new(result.trimmed));
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
                self.auto_save_if_enabled(ctx);
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
    /// ファイルダイアログを開いてトリミング後の画像をバックグラウンドで保存する
    ///
    /// # Arguments
    /// * `ctx` - egui のコンテキスト (完了時の再描画に使用)
    pub(super) fn save_image(&mut self, ctx: &egui::Context) {
        let Some(img) = self.image.trimmed.clone() else {
            return;
        };
        let Some(mut path) = rfd::FileDialog::new()
            .add_filter("PNG", &["png"])
            .add_filter("JPEG", &["jpg", "jpeg"])
            .save_file()
        else {
            return;
        };
        // 拡張子なしで入力された場合は形式を判定できないため PNG とする
        if path.extension().is_none() {
            path.set_extension(DEFAULT_IMAGE_EXTENSION);
        }
        self.spawn_save(ctx, SaveKind::Manual, move || {
            image_output::save_image(&img, &path).map_err(|e| e.to_string())?;
            Ok(path)
        });
    }

    /// 自動保存が有効な場合にトリミング後の画像を保存先フォルダへバックグラウンドで保存する
    ///
    /// ファイル名は `{元画像名}_trimmed.png` とし、既存ファイルは上書きせず連番で回避する
    /// 成功時は既存のステータスメッセージに追記し、エラー時のみ上書きする
    ///
    /// # Arguments
    /// * `ctx` - egui のコンテキスト (完了時の再描画に使用)
    pub(super) fn auto_save_if_enabled(&mut self, ctx: &egui::Context) {
        if !self.prefs.auto_save {
            return;
        }
        let Some(img) = self.image.trimmed.clone() else {
            return;
        };
        let dir = self.prefs.auto_save_dir.clone();
        let stem = format!(
            "{}_trimmed",
            self.image
                .source_stem
                .as_deref()
                .unwrap_or(AUTO_SAVE_FALLBACK_STEM)
        );
        self.spawn_save(ctx, SaveKind::Auto, move || {
            fs::create_dir_all(&dir).map_err(|e| format!("自動保存フォルダの作成に失敗: {e}"))?;
            let (path, _) =
                image_output::save_image_unique(&img, &dir, &stem, DEFAULT_IMAGE_EXTENSION)
                    .map_err(|e| {
                        let cause = e
                            .source()
                            .map_or_else(|| e.to_string(), ToString::to_string);
                        format!("自動保存エラー: {cause}")
                    })?;
            Ok(path)
        });
    }

    /// 保存処理をワーカースレッドで実行し、完了を `IoMsg::Saved` で通知する
    ///
    /// # Arguments
    /// * `ctx` - egui のコンテキスト (完了時の再描画に使用)
    /// * `kind` - 保存処理の種別
    /// * `job` - 保存先パス、または失敗時のステータスメッセージを返す保存処理
    fn spawn_save<F>(&mut self, ctx: &egui::Context, kind: SaveKind, job: F)
    where
        F: FnOnce() -> Result<PathBuf, String> + Send + 'static,
    {
        let tx = self.io.tx.clone();
        let ctx_clone = ctx.clone();
        let handle = thread::spawn(move || {
            let _ = tx.send(IoMsg::Saved {
                kind,
                result: job(),
            });
            ctx_clone.request_repaint();
        });
        self.io.save_handles.push(handle);
    }
}

// ======================================================================
// クリア
// ======================================================================
impl App {
    /// 画像をすべてクリアして初期状態に戻す
    pub(super) fn clear_image(&mut self) {
        self.trim_worker.abort();
        self.io.abort_load();
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
                // 読み込み中のファイルがあっても貼り付けた画像を優先する
                self.io.abort_load();
                self.set_original_image(
                    ctx,
                    DynamicImage::ImageRgba8(rgba),
                    "クリップボードから貼り付け完了".into(),
                    CLIPBOARD_SOURCE_STEM.into(),
                );
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
        self.start_batch_trim(ctx, paths, 0);
    }

    /// 保存先フォルダを選択させ、指定ファイルの一括トリミングをバックグラウンドで開始する
    ///
    /// # Arguments
    /// * `ctx` - egui のコンテキスト (再描画リクエストに使用)
    /// * `paths` - トリミング対象の画像ファイル
    /// * `skipped` - 呼び出し側で対象外として除外した件数 (完了時の要約に追記する)
    fn start_batch_trim(&mut self, ctx: &egui::Context, paths: Vec<PathBuf>, skipped: usize) {
        if self.batch.running {
            return;
        }
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
        self.batch.skipped_inputs = skipped;
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
                    let note = skipped_note(self.batch.skipped_inputs);
                    self.status = format!("{}{note}", result.summary());
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
// ドラッグ & ドロップ
// ======================================================================
impl App {
    /// ドロップされたファイルを件数に応じて処理する
    ///
    /// 対応外の拡張子・フォルダは除外する。対象が 1 件ならプレビューで開き、
    /// 2 件以上なら保存先フォルダを選択させて一括トリミングする。
    ///
    /// # Arguments
    /// * `ctx` - egui のコンテキスト
    /// * `paths` - ドロップされたパス
    pub(super) fn handle_dropped_paths(&mut self, ctx: &egui::Context, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        let (images, skipped) = partition_supported_images(paths);
        match images.as_slice() {
            [] => {
                self.status = format!("対応していないファイルです ({skipped} 件)");
            }
            [path] => {
                self.load_image_from_path(ctx, path);
                if skipped > 0 {
                    let note = skipped_note(skipped);
                    self.status.push_str(&note);
                    // 読み込み完了でステータスが上書きされても除外件数を残す
                    self.io.load_note = Some(note);
                }
            }
            _ if self.batch.running => {
                self.status =
                    "一括トリミングの実行中は複数ファイルのドロップを受け付けません".into();
            }
            _ => self.start_batch_trim(ctx, images, skipped),
        }
    }
}

/// パスを対応画像ファイルとそれ以外に振り分ける
///
/// 拡張子の大文字・小文字は区別しない。フォルダは拡張子に関係なく対象外とする
///
/// # Arguments
/// * `paths` - 振り分けるパス
///
/// # Returns
/// * `(Vec<PathBuf>, usize)` - 対応画像ファイル (入力順を維持) と除外した件数
fn partition_supported_images(paths: Vec<PathBuf>) -> (Vec<PathBuf>, usize) {
    let total = paths.len();
    let images: Vec<PathBuf> = paths
        .into_iter()
        .filter(|p| {
            !p.is_dir()
                && p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| is_supported_image_extension(&e.to_ascii_lowercase()))
        })
        .collect();
    let skipped = total - images.len();
    (images, skipped)
}

/// 除外件数をステータスへ追記する補足文を返す (0 件なら空文字列)
fn skipped_note(skipped: usize) -> String {
    if skipped == 0 {
        String::new()
    } else {
        format!(" (対応外の {skipped} 件を除外)")
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
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::super::state::{App, AppPreferences, ImageState};
    use super::{partition_supported_images, skipped_note};

    use eframe::egui;
    use image::{DynamicImage, RgbaImage};

    // ======================================================================
    // テスト用のヘルパー関数
    // ======================================================================
    /// テスト用の単色 4×4 RGBA 画像を生成する
    fn make_test_image() -> DynamicImage {
        let buf = vec![200u8; 4 * 4 * 4];
        DynamicImage::ImageRgba8(RgbaImage::from_raw(4, 4, buf).unwrap())
    }

    /// 条件を満たすまで `poll_io()` を繰り返す。タイムアウト時は `false` を返す
    fn poll_io_until(app: &mut App, ctx: &egui::Context, cond: impl Fn(&App) -> bool) -> bool {
        for _ in 0..500 {
            app.poll_io(ctx);
            if cond(app) {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        false
    }

    /// テスト用の一時ディレクトリパスを生成する
    fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
        env::temp_dir().join(format!(
            "{prefix}_{}_{:?}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
            thread::current().id()
        ))
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
                trimmed: Some(Arc::new(make_test_image())),
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
        let ctx = egui::Context::default();
        let mut app = App {
            prefs: AppPreferences {
                auto_save: false,
                ..AppPreferences::default()
            },
            image: ImageState {
                trimmed: Some(Arc::new(make_test_image())),
                ..ImageState::default()
            },
            ..App::default()
        };
        let status_before = app.status.clone();
        app.auto_save_if_enabled(&ctx);
        // 保存スレッドを起動しないこと
        assert!(!app.io.is_saving());
        assert_eq!(app.status, status_before);
    }

    /// `auto_save_if_enabled()` がトリミング後の画像がない場合に何もしないことを確認する
    #[test]
    fn auto_save_skips_when_no_trimmed_image() {
        let ctx = egui::Context::default();
        let mut app = App {
            prefs: AppPreferences {
                auto_save: true,
                ..AppPreferences::default()
            },
            ..App::default()
        };
        let status_before = app.status.clone();
        app.auto_save_if_enabled(&ctx);
        assert!(!app.io.is_saving());
        assert_eq!(app.status, status_before);
    }

    /// `auto_save_if_enabled()` が自動保存が有効でトリミング後の画像がある場合に PNG ファイルを保存することを確認する
    #[test]
    fn auto_save_writes_png_file_to_dir() {
        let ctx = egui::Context::default();
        let tmp = unique_temp_dir("edge_cut_test");
        let mut app = App {
            prefs: AppPreferences {
                auto_save: true,
                auto_save_dir: tmp.clone(),
                ..AppPreferences::default()
            },
            image: ImageState {
                trimmed: Some(Arc::new(make_test_image())),
                ..ImageState::default()
            },
            ..App::default()
        };
        app.auto_save_if_enabled(&ctx);

        // ステータスが自動保存パスを含むこと
        let done = poll_io_until(&mut app, &ctx, |a| a.status.contains("自動保存"));
        assert!(done, "status = {}", app.status);

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

    /// 自動保存が元画像名でファイルを作成し、2 回目は上書きせず連番を付けることを確認する
    #[test]
    fn auto_save_uses_source_name_and_avoids_overwrite() {
        let ctx = egui::Context::default();
        let tmp = unique_temp_dir("edge_cut_auto_name");
        let mut app = App {
            prefs: AppPreferences {
                auto_save: true,
                auto_save_dir: tmp.clone(),
                ..AppPreferences::default()
            },
            image: ImageState {
                trimmed: Some(Arc::new(make_test_image())),
                source_stem: Some("photo".into()),
                ..ImageState::default()
            },
            ..App::default()
        };

        // 1 回目は `{元画像名}_trimmed.png`
        app.auto_save_if_enabled(&ctx);
        let first = tmp.join("photo_trimmed.png");
        assert!(poll_io_until(&mut app, &ctx, |_| first.is_file()));

        // 2 回目は既存ファイルを残したまま連番で保存する
        app.auto_save_if_enabled(&ctx);
        let second = tmp.join("photo_trimmed_2.png");
        assert!(poll_io_until(&mut app, &ctx, |_| second.is_file()));
        assert!(first.is_file());

        let _ = fs::remove_dir_all(&tmp);
    }

    /// `auto_save_if_enabled()` がディレクトリ作成に失敗した場合にエラーステータスを設定することを確認する
    #[test]
    fn auto_save_error_status_when_dir_creation_fails() {
        let ctx = egui::Context::default();
        // 既存ファイルをディレクトリとして扱おうとすると create_dir_all が失敗する
        let existing_file = unique_temp_dir("ec_blocker").with_extension("bin");
        fs::write(&existing_file, b"x").expect("テスト用ファイルの作成に失敗");

        // ファイルのパス配下にサブディレクトリを作ろうとするので失敗する
        let mut app = App {
            prefs: AppPreferences {
                auto_save: true,
                auto_save_dir: existing_file.join("subdir_cannot_exist"),
                ..AppPreferences::default()
            },
            image: ImageState {
                trimmed: Some(Arc::new(make_test_image())),
                ..ImageState::default()
            },
            ..App::default()
        };
        app.auto_save_if_enabled(&ctx);
        let done = poll_io_until(&mut app, &ctx, |a| {
            a.status.contains("失敗") || a.status.contains("エラー")
        });

        let _ = fs::remove_file(&existing_file);

        assert!(done, "エラーメッセージを期待したが: {}", app.status);
    }

    // ======================================================================
    // load_image_from_path() / poll_io()
    // ======================================================================
    /// `load_image_from_path()` が存在しないファイルを指定した場合にエラーステータスを設定することを確認する
    #[test]
    fn load_image_from_path_sets_error_status_for_missing_file() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        let bad_path = Path::new("/nonexistent/path/image.png");
        app.load_image_from_path(&ctx, bad_path);
        let done = poll_io_until(&mut app, &ctx, |a| a.status.contains("読み込みエラー"));
        assert!(done, "status = {}", app.status);
    }

    /// `load_image_from_path()` がエラー時に元画像を設定しないことを確認する
    #[test]
    fn load_image_from_path_does_not_set_original_on_error() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        let bad_path = Path::new("/nonexistent/path/image.png");
        app.load_image_from_path(&ctx, bad_path);
        poll_io_until(&mut app, &ctx, |a| a.status.contains("読み込みエラー"));
        assert!(app.image.original.is_none());
    }

    /// `load_image_from_path()` がバックグラウンドで読み込んだ画像を元画像に設定することを確認する
    #[test]
    fn load_image_from_path_sets_original_in_background() {
        let ctx = egui::Context::default();
        let tmp = unique_temp_dir("edge_cut_load");
        fs::create_dir_all(&tmp).unwrap();
        let path = tmp.join("input.png");
        make_test_image().save(&path).unwrap();

        let mut app = App::default();
        app.load_image_from_path(&ctx, &path);
        // 呼び出し直後は読み込み中であること
        assert!(app.status.contains("読み込み中"), "status = {}", app.status);

        let done = poll_io_until(&mut app, &ctx, |a| a.image.original.is_some());
        let _ = fs::remove_dir_all(&tmp);

        assert!(done, "元画像が設定されなかった: {}", app.status);
        assert!(
            app.status.contains("読み込み完了"),
            "status = {}",
            app.status
        );
    }

    /// 読み込み中に `clear_image()` した場合、遅れて届いた読み込み結果を無視することを確認する
    #[test]
    fn clear_image_discards_pending_load_result() {
        let ctx = egui::Context::default();
        let tmp = unique_temp_dir("edge_cut_stale");
        fs::create_dir_all(&tmp).unwrap();
        let path = tmp.join("input.png");
        make_test_image().save(&path).unwrap();

        let mut app = App::default();
        app.load_image_from_path(&ctx, &path);
        app.clear_image();

        // 読み込みスレッドの完了を待ってから結果を受信する
        thread::sleep(Duration::from_millis(300));
        app.poll_io(&ctx);
        let _ = fs::remove_dir_all(&tmp);

        assert!(app.image.original.is_none());
        assert!(app.status.contains("画像を開く"), "status = {}", app.status);
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

    // ======================================================================
    // handle_dropped_paths() / partition_supported_images()
    // ======================================================================
    /// 対応拡張子のファイルのみを入力順のまま残し、除外件数を返すことを確認する
    #[test]
    fn partition_supported_images_filters_by_extension() {
        let paths = vec![
            PathBuf::from("b.PNG"),
            PathBuf::from("note.txt"),
            PathBuf::from("a.jpeg"),
            PathBuf::from("no_ext"),
        ];
        let (images, skipped) = partition_supported_images(paths);
        // 大文字の拡張子も対応形式として扱う
        assert_eq!(
            images,
            vec![PathBuf::from("b.PNG"), PathBuf::from("a.jpeg")]
        );
        assert_eq!(skipped, 2);
    }

    /// 画像の拡張子を持つフォルダを除外することを確認する
    #[test]
    fn partition_supported_images_skips_directories() {
        let tmp = unique_temp_dir("edge_cut_dir").with_extension("png");
        fs::create_dir_all(&tmp).unwrap();
        let (images, skipped) = partition_supported_images(vec![tmp.clone()]);
        let _ = fs::remove_dir_all(&tmp);

        assert!(images.is_empty());
        assert_eq!(skipped, 1);
    }

    /// 除外件数が 0 のときのみ補足文が空になることを確認する
    #[test]
    fn skipped_note_is_empty_only_for_zero() {
        assert!(skipped_note(0).is_empty());
        assert!(skipped_note(3).contains("3 件"));
    }

    /// 対応ファイルがない場合に件数をステータスへ表示し、読み込みを行わないことを確認する
    #[test]
    fn handle_dropped_paths_reports_unsupported_only() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.handle_dropped_paths(&ctx, vec![PathBuf::from("a.txt"), PathBuf::from("b.pdf")]);

        assert!(
            app.status.contains("対応していない") && app.status.contains("2 件"),
            "status = {}",
            app.status
        );
        assert!(app.image.original.is_none());
    }

    /// 空のドロップではステータスを変更しないことを確認する
    #[test]
    fn handle_dropped_paths_ignores_empty() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        let status_before = app.status.clone();
        app.handle_dropped_paths(&ctx, Vec::new());
        assert_eq!(app.status, status_before);
    }

    /// 対応画像が 1 件ならプレビューで開き、完了後のステータスにも除外件数を残すことを確認する
    #[test]
    fn handle_dropped_paths_loads_single_image_with_note() {
        let ctx = egui::Context::default();
        let tmp = unique_temp_dir("edge_cut_drop");
        fs::create_dir_all(&tmp).unwrap();
        let path = tmp.join("input.png");
        make_test_image().save(&path).unwrap();

        let mut app = App::default();
        app.handle_dropped_paths(&ctx, vec![tmp.join("readme.txt"), path]);
        // 読み込み中の時点で除外件数を表示する
        assert!(app.status.contains("読み込み中"), "status = {}", app.status);
        assert!(app.status.contains("除外"), "status = {}", app.status);

        let done = poll_io_until(&mut app, &ctx, |a| a.image.original.is_some());
        let _ = fs::remove_dir_all(&tmp);

        // 読み込み完了で上書きされても補足は残る
        assert!(done, "元画像が設定されなかった: {}", app.status);
        assert!(
            app.status.contains("読み込み完了") && app.status.contains("対応外の 1 件"),
            "status = {}",
            app.status
        );
    }

    /// 一括トリミングの実行中は複数ファイルのドロップを受け付けないことを確認する
    #[test]
    fn handle_dropped_paths_rejects_batch_while_running() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.batch.running = true;
        app.handle_dropped_paths(&ctx, vec![PathBuf::from("a.png"), PathBuf::from("b.png")]);
        assert!(
            app.status.contains("受け付けません"),
            "status = {}",
            app.status
        );
    }
}
