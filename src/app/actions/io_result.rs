//! バックグラウンドの読み込み・保存・クリップボード結果の受信

use std::mem;
use std::path::PathBuf;
use std::sync::Arc;

use crate::app::state::{App, IoMsg, SaveKind};

use eframe::egui;
use image::DynamicImage;

// ======================================================================
// 定数
// ======================================================================
/// クリップボードから貼り付けた画像の元ファイル名 (拡張子なしのため `Original` 形式では PNG)
const CLIPBOARD_SOURCE_NAME: &str = "clipboard";

// ======================================================================
// 画像 I/O の結果受信
// ======================================================================
impl App {
    /// 読み込み済みの画像を元画像として設定し、プレビュートリミングを開始する
    ///
    /// # Arguments
    /// * `ctx` - egui のコンテキスト (トリミングのスケジュールに使用)
    /// * `img` - 元画像
    /// * `icc_profile` - 元画像の ICC プロファイル (保存時に埋め込む)
    /// * `status` - 設定するステータスメッセージ
    /// * `source_name` - 出力ファイル名・形式の決定に使う元画像のファイル名
    pub(super) fn set_original_image(
        &mut self,
        ctx: &egui::Context,
        img: DynamicImage,
        icc_profile: Option<Vec<u8>>,
        status: String,
        source_name: PathBuf,
    ) {
        self.image.prepare_for_load();
        self.status = status;
        self.image.original = Some(Arc::new(img));
        self.image.icc_profile = icc_profile.map(Arc::from);
        self.image.source_name = Some(source_name);
        self.background.reset_to_override(self.prefs.bg_override);
        self.schedule_trim(ctx, false, true, true);
    }

    /// バックグラウンドの読み込み・保存結果を毎フレーム処理する
    ///
    /// # Arguments
    /// * `ctx` - egui のコンテキスト (読み込み完了後のトリミングに使用)
    pub(in crate::app) fn poll_io(&mut self, ctx: &egui::Context) {
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
                        Ok(loaded) => {
                            let status = format!("読み込み完了: {file_name}{note}");
                            self.set_original_image(
                                ctx,
                                loaded.image,
                                loaded.icc_profile,
                                status,
                                file_name.into(),
                            );
                        }
                        Err(msg) => self.status = format!("{msg}{note}"),
                    }
                }
                IoMsg::Saved { kind, result } => match (kind, result) {
                    (SaveKind::Manual, Ok(path)) => {
                        self.status = format!("保存完了: {}", path.display());
                    }
                    (SaveKind::Auto, Ok(path)) => {
                        // 既存メッセージを上書きせず末尾に追記する。前回の追記が残っていれば置き換える
                        let base = match self.io.auto_save_status.take() {
                            Some((base, shown)) if shown == self.status => base,
                            _ => mem::take(&mut self.status),
                        };
                        self.status = format!("{base}  (自動保存: {})", path.display());
                        self.io.auto_save_status = Some((base, self.status.clone()));
                    }
                    (SaveKind::Auto, Err(msg)) => {
                        // 失敗した内容は再度の有効化で保存し直せるようにする
                        self.io.last_auto_save = None;
                        self.status = msg;
                    }
                    (SaveKind::Manual, Err(msg)) => self.status = msg,
                },
                IoMsg::Copied { result } => match result {
                    Ok(()) => self.status = "クリップボードへコピーしました".into(),
                    Err(msg) => self.status = msg,
                },
                IoMsg::Pasted { request_id, result } => {
                    // 後続の読み込み・貼り付け・クリアで無効化された結果は捨てる
                    if request_id != self.io.load_request_id {
                        continue;
                    }
                    match result {
                        // クリップボード画像は ICC プロファイルを持たない
                        Ok(img) => self.set_original_image(
                            ctx,
                            img,
                            None,
                            "クリップボードから貼り付け完了".into(),
                            CLIPBOARD_SOURCE_NAME.into(),
                        ),
                        Err(msg) => self.status = msg,
                    }
                }
                IoMsg::DialogClosed(result) => self.handle_dialog_result(ctx, result),
            }
        }
        self.io.save_handles.retain(|h| !h.is_finished());
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::CLIPBOARD_SOURCE_NAME;
    use crate::app::actions::test_helpers::make_test_image;
    use crate::app::state::{App, IoMsg};

    use eframe::egui;

    // ======================================================================
    // クリップボード結果の受信
    // ======================================================================
    /// 有効な貼り付け結果を受信すると元画像として設定することを確認する
    #[test]
    fn poll_io_applies_pasted_image() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        let request_id = app.io.load_request_id;
        app.io
            .tx
            .send(IoMsg::Pasted {
                request_id,
                result: Ok(make_test_image()),
            })
            .unwrap();
        app.poll_io(&ctx);

        assert!(app.image.original.is_some());
        assert_eq!(
            app.image.source_name.as_deref(),
            Some(Path::new(CLIPBOARD_SOURCE_NAME))
        );
        assert!(
            app.status.contains("貼り付け完了"),
            "status = {}",
            app.status
        );
    }

    /// 貼り付け後に `clear_image()` した場合、遅れて届いた貼り付け結果を無視することを確認する
    #[test]
    fn clear_image_discards_pending_paste_result() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        let request_id = app.io.load_request_id;
        app.clear_image();
        app.io
            .tx
            .send(IoMsg::Pasted {
                request_id,
                result: Ok(make_test_image()),
            })
            .unwrap();
        app.poll_io(&ctx);

        assert!(app.image.original.is_none());
        assert!(app.status.contains("画像を開く"), "status = {}", app.status);
    }

    /// コピー結果に応じてステータスを更新することを確認する
    #[test]
    fn poll_io_reports_copy_result() {
        let ctx = egui::Context::default();
        let mut app = App::default();

        // 成功時は完了メッセージ
        app.io.tx.send(IoMsg::Copied { result: Ok(()) }).unwrap();
        app.poll_io(&ctx);
        assert!(
            app.status.contains("コピーしました"),
            "status = {}",
            app.status
        );

        // 失敗時はワーカーのメッセージをそのまま表示
        app.io
            .tx
            .send(IoMsg::Copied {
                result: Err("コピーエラー: test".into()),
            })
            .unwrap();
        app.poll_io(&ctx);
        assert_eq!(app.status, "コピーエラー: test");
    }
}
