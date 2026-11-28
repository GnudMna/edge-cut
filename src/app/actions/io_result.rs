//! バックグラウンドの読み込み・保存・クリップボード結果の受信

use std::mem;
use std::path::PathBuf;
use std::sync::Arc;

use image::DynamicImage;

use crate::app::state::{App, IoMsg, SaveKind};
use crate::dpi::Dpi;

// ======================================================================
// 定数
// ======================================================================
/// クリップボードから貼り付けた画像の元ファイル名 (拡張子なしのため `Original` 形式では PNG)
const CLIPBOARD_SOURCE_NAME: &str = "clipboard";

/// 複数フレームのファイルを読み込んだときにステータスへ追記する補足
const EXTRA_FRAMES_NOTE: &str = " (先頭の 1 フレームのみ読み込み)";

// ======================================================================
// 画像 I/O の結果受信
// ======================================================================
impl App {
    /// 読み込み済みの画像を元画像として設定し、プレビュートリミングを開始する
    ///
    /// # Arguments
    /// * `img` - 元画像
    /// * `icc_profile` - 元画像の ICC プロファイル (保存時に埋め込む)
    /// * `dpi` - 元画像の解像度 (保存時に書き込む)
    /// * `status` - 設定するステータスメッセージ
    /// * `source_name` - 出力ファイル名・形式の決定に使う元画像のファイル名
    pub(super) fn set_original_image(
        &mut self,
        img: DynamicImage,
        icc_profile: Option<Vec<u8>>,
        dpi: Option<Dpi>,
        status: String,
        source_name: PathBuf,
    ) {
        self.image.prepare_for_load();
        self.status = status;
        self.image.original = Some(Arc::new(img));
        self.image.icc_profile = icc_profile.map(Arc::from);
        self.image.preview_fill = None;
        self.image.dpi = dpi;
        self.image.source_name = Some(source_name);
        // ファイルから読み込んだ場合は呼び出し側が元のフォルダを設定する
        self.image.source_dir = None;
        // ファイルから読み込んだ場合は呼び出し側が判定済みの形式を設定する
        self.image.source_format = None;
        self.background.reset_to_override(self.prefs.bg_override);
        self.schedule_trim(false, true, true);
    }

    /// バックグラウンドの読み込み・保存結果を毎フレーム処理する
    ///
    /// # Arguments
    pub(in crate::app) fn poll_io(&mut self) {
        while let Ok(msg) = self.io.rx.try_recv() {
            match msg {
                IoMsg::Loaded {
                    request_id,
                    file_name,
                    source_dir,
                    result,
                } => {
                    // 後続の読み込み・貼り付け・クリアで無効化された結果は捨てる
                    if request_id != self.io.load_request_id {
                        continue;
                    }
                    self.io.loading = false;
                    let note = self.io.load_note.take().unwrap_or_default();
                    match result {
                        Ok(loaded) => {
                            let frames = if loaded.has_extra_frames {
                                EXTRA_FRAMES_NOTE
                            } else {
                                ""
                            };
                            let status = format!("読み込み完了: {file_name}{frames}{note}");
                            self.set_original_image(
                                loaded.image,
                                loaded.icc_profile,
                                loaded.dpi,
                                status,
                                file_name.into(),
                            );
                            self.image.source_dir = source_dir;
                            self.image.source_format = loaded.source_format;
                        }
                        Err(msg) => self.status = format!("{msg}{note}"),
                    }
                }
                IoMsg::Saved { kind, result } => match (kind, result) {
                    (SaveKind::Manual, Ok(path)) => {
                        self.status = format!("保存完了: {}", path.display());
                    }
                    (SaveKind::Auto(_), Ok(path)) => {
                        // 既存メッセージを上書きせず末尾に追記する。前回の追記が残っていれば置き換える
                        let base = match self.io.auto_save_status.take() {
                            Some((base, shown)) if shown == self.status => base,
                            _ => mem::take(&mut self.status),
                        };
                        self.status = format!("{base}  (自動保存: {})", path.display());
                        self.io.auto_save_status = Some((base, self.status.clone()));
                    }
                    (SaveKind::Auto(key), Err(msg)) => {
                        // 失敗した内容は保存し直せるように記録を消す
                        // 後続の保存が別の内容で始まっている場合は、その記録を残す
                        if self.io.last_auto_save.as_ref() == Some(&key) {
                            self.io.last_auto_save = None;
                        }
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
                        Ok(img) => {
                            // 貼り付けを優先し、読み込み中のファイルの結果は破棄する
                            self.io.abort_load();
                            // クリップボード画像は ICC プロファイルも解像度も持たない
                            self.set_original_image(
                                img,
                                None,
                                None,
                                "クリップボードから貼り付け完了".into(),
                                CLIPBOARD_SOURCE_NAME.into(),
                            );
                        }
                        Err(msg) => self.status = msg,
                    }
                }
                IoMsg::PastedFiles { request_id, paths } => {
                    // 後続の読み込み・貼り付け・クリアで無効化された結果は捨てる
                    if request_id != self.io.load_request_id {
                        continue;
                    }
                    // ドロップと同じく、1 件ならプレビュー、複数またはフォルダなら一括トリミング
                    self.handle_dropped_paths(paths);
                }
                IoMsg::DropScanned { request_id, result } => {
                    // 後続のドロップで無効化された結果は捨てる
                    if request_id != self.io.drop_request_id {
                        continue;
                    }
                    match result {
                        Ok(scanned) => self.apply_scanned_drop(scanned),
                        Err(msg) => self.status = msg,
                    }
                }
                IoMsg::DialogClosed(result) => self.handle_dialog_result(result),
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
    use crate::app::actions::test_helpers::{make_test_image, poll_io_until};
    use crate::app::state::{App, IoMsg};
    use crate::image_load::LoadedImage;

    // ======================================================================
    // 読み込み結果の受信
    // ======================================================================
    /// 読み込み完了のステータスに、複数フレームのときだけ先頭フレームのみである旨を追記すること
    #[test]
    fn poll_io_notes_first_frame_only_for_multi_frame_files() {
        for (has_extra_frames, expected) in [(true, true), (false, false)] {
            let mut app = App::default();
            let request_id = app.io.load_request_id;
            app.io
                .tx
                .send(IoMsg::Loaded {
                    request_id,
                    file_name: "anim.gif".into(),
                    source_dir: None,
                    result: Ok(LoadedImage {
                        image: make_test_image(),
                        icc_profile: None,
                        dpi: None,
                        has_extra_frames,
                        source_format: None,
                    }),
                })
                .unwrap();
            app.poll_io();

            assert!(
                app.status.contains("読み込み完了: anim.gif"),
                "{}",
                app.status
            );
            assert_eq!(app.status.contains("先頭の 1 フレームのみ"), expected);
        }
    }

    // ======================================================================
    // クリップボード結果の受信
    // ======================================================================
    /// 有効な貼り付け結果を受信すると元画像として設定すること
    #[test]
    fn poll_io_applies_pasted_image() {
        let mut app = App::default();
        let request_id = app.io.load_request_id;
        app.io
            .tx
            .send(IoMsg::Pasted {
                request_id,
                result: Ok(make_test_image()),
            })
            .unwrap();
        app.poll_io();

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

    /// 貼り付けが失敗した場合は、読み込み中のファイルの結果を破棄しないこと
    #[test]
    fn failed_paste_keeps_pending_load() {
        let mut app = App::default();
        let load_id = app.io.load_request_id;

        // 貼り付け失敗の後に、読み込み中だったファイルの結果が届く
        app.io
            .tx
            .send(IoMsg::Pasted {
                request_id: load_id,
                result: Err("クリップボードに画像がありません".into()),
            })
            .unwrap();
        app.io
            .tx
            .send(IoMsg::Loaded {
                request_id: load_id,
                file_name: "a.png".into(),
                source_dir: None,
                result: Ok(LoadedImage {
                    image: make_test_image(),
                    icc_profile: None,
                    dpi: None,
                    has_extra_frames: false,
                    source_format: None,
                }),
            })
            .unwrap();
        app.poll_io();

        assert_eq!(app.io.load_request_id, load_id);
        assert!(app.image.original.is_some());
        assert!(
            app.status.contains("読み込み完了"),
            "status = {}",
            app.status
        );
    }

    /// 貼り付けが成功した場合は、読み込み中のファイルの結果を破棄すること
    #[test]
    fn successful_paste_discards_pending_load() {
        let mut app = App::default();
        let load_id = app.io.load_request_id;
        app.io
            .tx
            .send(IoMsg::Pasted {
                request_id: load_id,
                result: Ok(make_test_image()),
            })
            .unwrap();
        app.io
            .tx
            .send(IoMsg::Loaded {
                request_id: load_id,
                file_name: "a.png".into(),
                source_dir: None,
                result: Err("読み込み失敗".into()),
            })
            .unwrap();
        app.poll_io();

        assert_ne!(app.io.load_request_id, load_id);
        assert!(
            app.status.contains("貼り付け完了"),
            "status = {}",
            app.status
        );
    }

    /// コピーされた画像ファイル 1 件を受信すると、ファイルとして読み込むこと
    #[test]
    fn poll_io_loads_single_pasted_file() {
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
        app.poll_io();

        let done = poll_io_until(&mut app, |a| a.image.original.is_some());
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
        let done = poll_io_until(&mut app, |a| a.status.contains("対応していない"));
        assert!(done, "status = {}", app.status);
        assert!(app.image.original.is_none());
    }

    /// 貼り付け後に `clear_image()` した場合、遅れて届いたファイルの貼り付け結果を無視すること
    #[test]
    fn clear_image_discards_pending_pasted_files() {
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
        app.poll_io();

        assert!(app.status.contains("画像を開く"), "status = {}", app.status);
    }

    /// 貼り付け後に `clear_image()` した場合、遅れて届いた貼り付け結果を無視すること
    #[test]
    fn clear_image_discards_pending_paste_result() {
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
        app.poll_io();

        assert!(app.image.original.is_none());
        assert!(app.status.contains("画像を開く"), "status = {}", app.status);
    }

    /// コピー結果に応じてステータスを更新すること
    #[test]
    fn poll_io_reports_copy_result() {
        let mut app = App::default();

        // 成功時は完了メッセージ
        app.io.tx.send(IoMsg::Copied { result: Ok(()) }).unwrap();
        app.poll_io();
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
        app.poll_io();
        assert_eq!(app.status, "コピーエラー: test");
    }
}
