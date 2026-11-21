//! アプリケーションのアクション (画像操作)
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use super::state::App;
use crate::trim::{to_color_image, trim};

use eframe::egui;
use image::DynamicImage;
use rayon::prelude::*;

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
            .add_filter(
                "画像",
                &["png", "jpg", "jpeg", "bmp", "gif", "tiff", "webp"],
            )
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
        match image::open(path) {
            Ok(img) => {
                self.orig_tex = Some(ctx.load_texture(
                    "orig",
                    to_color_image(&img),
                    egui::TextureOptions::LINEAR,
                ));
                self.status = format!(
                    "読み込み完了: {}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                );
                self.original = Some(img);
                self.trim_cache_key = None; // 新画像なのでキャッシュを無効化
                self.do_trim(ctx);
                self.auto_save_if_enabled();
            }
            Err(e) => self.status = format!("読み込みエラー: {e}"),
        }
    }
}

// ======================================================================
// トリミング
// ======================================================================
impl App {
    /// 現在の許容度設定でトリミングを実行し、テクスチャを更新する
    ///
    /// # Arguments
    /// * `ctx` - egui のコンテキスト (テクスチャ登録に使用)
    pub(super) fn do_trim(&mut self, ctx: &egui::Context) {
        let key = (self.tolerance, self.padding);
        // 同じ設定で既にトリミング済みなら再実行しない
        if self.trimmed.is_some() && self.trim_cache_key == Some(key) {
            return;
        }
        let result = self
            .original
            .as_ref()
            .map(|img| trim(img, self.tolerance, self.padding));
        if let Some((trimmed, info)) = result {
            self.trim_tex = Some(ctx.load_texture(
                "trim",
                to_color_image(&trimmed),
                egui::TextureOptions::LINEAR,
            ));
            self.info = Some(info);
            self.trimmed = Some(trimmed);
            self.trim_cache_key = Some(key);
        }
    }
}

// ======================================================================
// 保存
// ======================================================================
impl App {
    /// ファイルダイアログを開いてトリミング後の画像を保存する
    pub(super) fn save_image(&mut self) {
        let Some(ref img) = self.trimmed else { return };
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
    /// 成功時は既存のステータスメッセージに追記し、エラー時のみ上書きする。
    pub(super) fn auto_save_if_enabled(&mut self) {
        if !self.auto_save {
            return;
        }
        let Some(ref img) = self.trimmed else { return };
        if let Err(e) = fs::create_dir_all(&self.auto_save_dir) {
            self.status = format!("自動保存フォルダの作成に失敗: {e}");
            return;
        }
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let filename = format!("trimmed_{ts}.png");
        let path = self.auto_save_dir.join(&filename);
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
        self.original = None;
        self.trimmed = None;
        self.orig_tex = None;
        self.trim_tex = None;
        self.info = None;
        self.trim_cache_key = None;
        self.status = "「画像を開く」で画像を選択してください".into();
    }
}

// ======================================================================
// クリップボード
// ======================================================================
impl App {
    /// クリップボードアクセス用インスタンスを返す。未初期化ならここで作成する。
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
        let Some(ref img) = self.trimmed else { return };
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
            Ok(cb) => match cb.get_image() {
                Ok(data) => data,
                Err(_) => {
                    self.status = "クリップボードに画像がありません".into();
                    return;
                }
            },
        };
        let (w, h) = (img_data.width as u32, img_data.height as u32);
        let bytes: Vec<u8> = img_data.bytes.into_owned();
        match image::RgbaImage::from_raw(w, h, bytes) {
            Some(rgba) => {
                let img = DynamicImage::ImageRgba8(rgba);
                self.orig_tex = Some(ctx.load_texture(
                    "orig",
                    to_color_image(&img),
                    egui::TextureOptions::LINEAR,
                ));
                self.status = "クリップボードから貼り付け完了".into();
                self.original = Some(img);
                self.trim_cache_key = None; // 新画像なのでキャッシュを無効化
                self.do_trim(ctx);
                self.auto_save_if_enabled();
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
    /// バックグラウンドスレッドで処理するため UI はブロックしない。
    /// 完了後は `poll_batch_result` でステータスを更新する。
    ///
    /// # Arguments
    /// * `ctx` - egui のコンテキスト (再描画リクエストに使用)
    pub(super) fn batch_trim(&mut self, ctx: &egui::Context) {
        if self.batch_running {
            return;
        }
        let Some(paths) = rfd::FileDialog::new()
            .add_filter(
                "画像",
                &["png", "jpg", "jpeg", "bmp", "gif", "tiff", "webp"],
            )
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

        let tolerance = self.tolerance;
        let padding = self.padding;
        let ctx_clone = ctx.clone();
        let total = paths.len();

        // 新バッチのキャンセルフラグをリセットし共有する
        self.batch_cancel.store(false, Ordering::Relaxed);
        let cancel_flag = Arc::clone(&self.batch_cancel);

        let (tx, rx) = mpsc::channel::<(bool, String)>();
        self.batch_rx = Some(rx);
        self.batch_running = true;
        self.status = format!("一括トリミング中... ({total} ファイル)");

        thread::spawn(move || {
            if let Err(e) = fs::create_dir_all(&out_dir) {
                let _ = tx.send((true, format!("出力フォルダの作成に失敗: {e}")));
                ctx_clone.request_repaint();
                return;
            }

            let ok = AtomicUsize::new(0);
            let processed = AtomicUsize::new(0);
            let err_msgs = Mutex::new(Vec::new());

            paths.par_iter().for_each(|path| {
                if cancel_flag.load(Ordering::Relaxed) {
                    return;
                }

                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();

                match image::open(path) {
                    Ok(img) => {
                        let (trimmed, _) = trim(&img, tolerance, padding);
                        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                        let out_path = out_dir.join(format!("{stem}_trimmed.png"));
                        match trimmed.save(&out_path) {
                            Ok(()) => {
                                ok.fetch_add(1, Ordering::Relaxed);
                            }
                            Err(e) => {
                                err_msgs
                                    .lock()
                                    .unwrap()
                                    .push(format!("保存エラー {name}: {e}"));
                            }
                        }
                    }
                    Err(e) => {
                        err_msgs
                            .lock()
                            .unwrap()
                            .push(format!("読み込みエラー {name}: {e}"));
                    }
                }

                let p = processed.fetch_add(1, Ordering::Relaxed) + 1;
                let _ = tx.send((false, format!("一括トリミング中... ({p}/{total})")));
                ctx_clone.request_repaint();
            });

            let ok_count = ok.into_inner();

            // キャンセルされた場合の報告
            if cancel_flag.load(Ordering::Relaxed) {
                let _ = tx.send((
                    true,
                    format!("一括トリミングをキャンセルしました ({ok_count}/{total} 保存済み)"),
                ));
                ctx_clone.request_repaint();
                return;
            }

            let errs = err_msgs.into_inner().unwrap();
            let error_count = errs.len();
            let msg = if error_count == 0 {
                format!(
                    "一括トリミング完了: {ok_count}/{total} ファイルを保存 → {}",
                    out_dir.display()
                )
            } else {
                format!(
                    "一括トリミング完了: {ok_count}/{total} 成功、{error_count} 件のエラー → {}\n{}",
                    out_dir.display(),
                    errs.join("\n"),
                )
            };
            let _ = tx.send((true, msg));
            ctx_clone.request_repaint();
        });
    }

    /// 実行中の一括トリミングをキャンセルする
    ///
    /// フラグをセットするのみ。スレッドは次のファイル処理前に停止しチャネルに完了メッセージを送る。
    pub(super) fn cancel_batch(&self) {
        self.batch_cancel.store(true, Ordering::Relaxed);
    }

    /// バックグラウンドの一括トリミング処理が完了していればステータスを更新する
    ///
    /// 毎フレーム呼び出すことで結果をポーリングする。
    /// 進捗メッセージ (`is_final = false`) はステータスに表示し処理を継続する。
    /// 完了メッセージ (`is_final = true`) を受信すると `batch_running` をリセットする。
    pub(super) fn poll_batch_result(&mut self) {
        if !self.batch_running {
            return;
        }
        let mut done = false;
        loop {
            let msg = self.batch_rx.as_ref().and_then(|rx| rx.try_recv().ok());
            match msg {
                Some((true, text)) => {
                    self.status = text;
                    done = true;
                    break;
                }
                Some((false, text)) => {
                    self.status = text;
                }
                None => break,
            }
        }
        if done {
            self.batch_running = false;
            self.batch_rx = None;
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
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::super::state::App;

    use image::{DynamicImage, RgbaImage};

    // ------------------------------------------------------------------
    // テスト用のヘルパー関数
    // ------------------------------------------------------------------
    /// テスト用の単色 4×4 RGBA 画像を生成する
    fn make_test_image() -> DynamicImage {
        let buf = vec![200u8; 4 * 4 * 4];
        DynamicImage::ImageRgba8(RgbaImage::from_raw(4, 4, buf).unwrap())
    }

    // ------------------------------------------------------------------
    // clear_image()
    // ------------------------------------------------------------------
    /// `clear_image()` が元画像を None に設定することを確認する
    #[test]
    fn clear_image_sets_original_to_none() {
        let mut app = App::default();
        app.original = Some(make_test_image());
        app.clear_image();
        assert!(app.original.is_none());
    }

    /// `clear_image()` がトリミング後の画像を None に設定することを確認する
    #[test]
    fn clear_image_sets_trimmed_to_none() {
        let mut app = App::default();
        app.trimmed = Some(make_test_image());
        app.clear_image();
        assert!(app.trimmed.is_none());
    }

    /// `clear_image()` がテクスチャを None に設定することを確認する
    #[test]
    fn clear_image_sets_info_to_none() {
        let mut app = App::default();
        // trim を経由せず info を直接注入
        app.info = Some(crate::trim::TrimInfo {
            orig_w: 10,
            orig_h: 10,
            trim_w: 8,
            trim_h: 8,
            offset_x: 1,
            offset_y: 1,
        });
        app.clear_image();
        assert!(app.info.is_none());
    }

    /// `clear_image()` がステータスメッセージを初期状態にリセットすることを確認する
    #[test]
    fn clear_image_resets_status_message() {
        let mut app = App::default();
        app.status = "何か別のメッセージ".into();
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
        assert!(app.original.is_none());
        assert!(app.trimmed.is_none());
    }

    // ------------------------------------------------------------------
    // auto_save_if_enabled()
    // ------------------------------------------------------------------
    /// `auto_save_if_enabled()` が自動保存が無効な場合に何もしないことを確認する
    #[test]
    fn auto_save_skips_when_disabled() {
        let mut app = App::default();
        app.auto_save = false;
        let status_before = app.status.clone();
        app.auto_save_if_enabled();
        assert_eq!(app.status, status_before);
    }

    /// `auto_save_if_enabled()` がトリミング後の画像がない場合に何もしないことを確認する
    #[test]
    fn auto_save_skips_when_no_trimmed_image() {
        let mut app = App::default();
        app.auto_save = true;
        app.trimmed = None;
        let status_before = app.status.clone();
        app.auto_save_if_enabled();
        assert_eq!(app.status, status_before);
    }

    /// `auto_save_if_enabled()` が自動保存が有効でトリミング後の画像がある場合に PNG ファイルを保存することを確認する
    #[test]
    fn auto_save_writes_png_file_to_dir() {
        let mut app = App::default();
        let tmp = env::temp_dir().join(format!(
            "edge_cut_test_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        ));
        app.auto_save = true;
        app.auto_save_dir = tmp.clone();
        app.trimmed = Some(make_test_image());
        app.auto_save_if_enabled();

        // ステータスが自動保存パスを含むこと
        assert!(app.status.contains("自動保存"), "status = {}", app.status);

        // 実際にファイルが生成されていること
        let saved_files: Vec<_> = fs::read_dir(&tmp)
            .map(|rd| rd.filter_map(|e| e.ok()).collect())
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
        let mut app = App::default();
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let existing_file = env::temp_dir().join(format!("ec_blocker_{ts}.bin"));
        fs::write(&existing_file, b"x").expect("テスト用ファイルの作成に失敗");

        // ファイルのパス配下にサブディレクトリを作ろうとするので失敗する
        app.auto_save = true;
        app.auto_save_dir = existing_file.join("subdir_cannot_exist");
        app.trimmed = Some(make_test_image());
        app.auto_save_if_enabled();

        let _ = fs::remove_file(&existing_file);

        assert!(
            app.status.contains("失敗") || app.status.contains("エラー"),
            "エラーメッセージを期待したが: {}",
            app.status
        );
    }

    // ------------------------------------------------------------------
    // load_image_from_path(エラーブランチ)
    // ------------------------------------------------------------------
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
        assert!(app.original.is_none());
    }
}
