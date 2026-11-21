//! バックグラウンドの読み込み・保存・クリップボード結果の受信

use std::mem;
use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui;
use image::DynamicImage;

use crate::app::state::{App, IoMsg, SaveKind};

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
                IoMsg::PastedFiles { request_id, paths } => {
                    // 後続の読み込み・貼り付け・クリアで無効化された結果は捨てる
                    if request_id != self.io.load_request_id {
                        continue;
                    }
                    // ドロップと同じく、1 件ならプレビュー、複数またはフォルダなら一括トリミング
                    self.handle_dropped_paths(ctx, paths);
                }
                IoMsg::DropScanned { request_id, result } => {
                    // 後続のドロップで無効化された結果は捨てる
                    if request_id != self.io.drop_request_id {
                        continue;
                    }
                    match result {
                        Ok(scanned) => self.apply_scanned_drop(ctx, scanned),
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

    use eframe::egui;

    use super::CLIPBOARD_SOURCE_NAME;
    use crate::app::actions::test_helpers::{make_test_image, poll_io_until};
    use crate::app::state::{App, IoMsg};

    // ======================================================================
    // クリップボード結果の受信
    // ======================================================================
    /// 有効な貼り付け結果を受信すると元画像として設定すること
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

    /// コピーされた画像ファイル 1 件を受信すると、ファイルとして読み込むこと
    #[test]
    fn poll_io_loads_single_pasted_file() {
        let ctx = egui::Context::default();
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("input.png");
        make_test_image().save(&path).unwrap();

        let mut app = App::default();
        let request_id = app.io.load_request_id;
        app.io
            .tx
            .send(IoMsg::PastedFiles {
                request_id,
                paths: vec![path],
            })
            .unwrap();
        app.poll_io(&ctx);

        let done = poll_io_until(&mut app, &ctx, |a| a.image.original.is_some());
        assert!(done, "元画像が設定されなかった: {}", app.status);
        // 元ファイル名を保持し、クリップボード画像の名前にはならない
        assert_eq!(
            app.image.source_name.as_deref(),
            Some(Path::new("input.png"))
        );
    }

    /// 対応画像でないファイルを受信した場合は読み込まず、ステータスで通知すること
    #[test]
    fn poll_io_reports_unsupported_pasted_file() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        let request_id = app.io.load_request_id;
        app.io
            .tx
            .send(IoMsg::PastedFiles {
                request_id,
                paths: vec!["note.txt".into()],
            })
            .unwrap();

        // フォルダの展開はワーカースレッドで行うため、結果が届くまで待つ
        let done = poll_io_until(&mut app, &ctx, |a| a.status.contains("対応していない"));
        assert!(done, "status = {}", app.status);
        assert!(app.image.original.is_none());
    }

    /// 貼り付け後に `clear_image()` した場合、遅れて届いたファイルの貼り付け結果を無視すること
    #[test]
    fn clear_image_discards_pending_pasted_files() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        let request_id = app.io.load_request_id;
        app.clear_image();
        app.io
            .tx
            .send(IoMsg::PastedFiles {
                request_id,
                paths: vec!["a.png".into()],
            })
            .unwrap();
        app.poll_io(&ctx);

        assert!(app.status.contains("画像を開く"), "status = {}", app.status);
    }

    /// 貼り付け後に `clear_image()` した場合、遅れて届いた貼り付け結果を無視すること
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

    /// コピー結果に応じてステータスを更新すること
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
