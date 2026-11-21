//! アプリケーションのアクション (画像操作)
use super::state::App;
use crate::trim::{to_color_image, trim};

use eframe::egui;
use image::DynamicImage;

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
    pub(super) fn load_image_from_path(&mut self, ctx: &egui::Context, path: &std::path::Path) {
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
                self.do_trim(ctx);
                self.auto_save_if_enabled();
            }
            Err(e) => self.status = format!("読み込みエラー: {e}"),
        }
    }

    /// 現在の許容度設定でトリミングを実行し、テクスチャを更新する
    ///
    /// # Arguments
    /// * `ctx` - egui のコンテキスト (テクスチャ登録に使用)
    pub(super) fn do_trim(&mut self, ctx: &egui::Context) {
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
        }
    }

    /// 自動保存が有効な場合にトリミング後の画像を保存先フォルダへ保存する
    pub(super) fn auto_save_if_enabled(&mut self) {
        if !self.auto_save {
            return;
        }
        let Some(ref img) = self.trimmed else { return };
        if let Err(e) = std::fs::create_dir_all(&self.auto_save_dir) {
            self.status = format!("自動保存フォルダの作成に失敗: {e}");
            return;
        }
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let filename = format!("trimmed_{ts}.png");
        let path = self.auto_save_dir.join(&filename);
        match img.save(&path) {
            Ok(()) => self.status = format!("自動保存: {}", path.display()),
            Err(e) => self.status = format!("自動保存エラー: {e}"),
        }
    }

    /// 画像をすべてクリアして初期状態に戻す
    pub(super) fn clear_image(&mut self) {
        self.original = None;
        self.trimmed = None;
        self.orig_tex = None;
        self.trim_tex = None;
        self.info = None;
        self.status = "「画像を開く」で画像を選択してください".into();
    }

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

    /// クリップボードから画像を読み込む
    ///
    /// # Arguments
    /// * `ctx` - egui のコンテキスト (テクスチャ登録に使用)
    pub(super) fn paste_from_clipboard(&mut self, ctx: &egui::Context) {
        let cb_result = arboard::Clipboard::new()
            .map_err(|e| format!("クリップボードの初期化に失敗しました: {e}"))
            .and_then(|mut cb| {
                cb.get_image()
                    .map_err(|_| "クリップボードに画像がありません".into())
            });

        match cb_result {
            Ok(img_data) => {
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
                        self.do_trim(ctx);
                        self.auto_save_if_enabled();
                    }
                    None => self.status = "クリップボード画像の変換に失敗しました".into(),
                }
            }
            Err(msg) => self.status = msg,
        }
    }
}
