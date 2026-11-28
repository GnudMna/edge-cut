//! 画像の読み込みとクリア

use std::path::Path;

use crate::app::load_worker::{LoadJob, LoadWorkerHandle};
use crate::app::state::{App, DialogResult};
use crate::consts::SUPPORTED_IMAGE_EXTENSIONS;

// ======================================================================
// 画像読み込み
// ======================================================================
impl App {
    /// ファイルダイアログを開いて画像を読み込む
    ///
    /// ダイアログは別スレッドで開き、選択後は `poll_io` 経由で読み込みを開始する
    ///
    /// # Arguments
    pub(in crate::app) fn open_image(&mut self) {
        self.spawn_dialog(|dialog| {
            let path = dialog
                .add_filter("画像", SUPPORTED_IMAGE_EXTENSIONS)
                .pick_file();
            DialogResult::OpenImage(path)
        });
    }

    /// パスから画像をバックグラウンドで読み込む
    ///
    /// 完了後は `poll_io` でテクスチャ・ステータスを更新する
    /// 読み込み中も直前の画像は表示したままにする
    ///
    /// # Arguments
    /// * `path` - 読み込む画像ファイルのパス
    pub(super) fn load_image_from_path(&mut self, path: &Path) {
        self.io.abort_load();
        let request_id = self.io.load_request_id;
        self.io.loading = true;
        let file_name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        self.status = format!("読み込み中: {file_name}");

        let job = LoadJob {
            request_id,
            path: path.to_path_buf(),
            file_name,
            // 親が空パス (相対のファイル名のみ) の場合はフォルダを特定できない
            source_dir: path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map(Path::to_path_buf),
            waker: self.waker.clone(),
        };
        self.load_worker().submit(job);
    }

    /// 画像読み込み用ワーカーを返す。未起動ならここで起動する
    fn load_worker(&mut self) -> &LoadWorkerHandle {
        let tx = self.io.tx.clone();
        self.loader
            .get_or_insert_with(|| LoadWorkerHandle::spawn(tx))
    }
}

// ======================================================================
// クリア
// ======================================================================
impl App {
    /// 画像をすべてクリアして初期状態に戻す
    pub(in crate::app) fn clear_image(&mut self) {
        self.trim_worker.abort();
        self.io.abort_load();
        self.image.clear();
        self.background.reset_to_override(self.prefs.bg_override);
        self.status = "「画像を開く」で画像を選択してください".into();
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use crate::app::actions::test_helpers::{make_test_image, poll_io_until, trimmed_image_state};
    use crate::app::state::{App, ImageState};

    // ======================================================================
    // clear_image()
    // ======================================================================
    /// `clear_image()` が元画像を None に設定すること
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

    /// `clear_image()` 後は保存・コピー対象がなくなること
    #[test]
    fn clear_image_clears_trim_target() {
        let mut app = App {
            image: trimmed_image_state(),
            ..App::default()
        };
        assert!(app.image.trim_target().is_some());
        app.clear_image();
        assert!(app.image.trim_target().is_none());
    }

    /// `clear_image()` がテクスチャを None に設定すること
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
                    extend_left: 0,
                    extend_top: 0,
                    extend_right: 0,
                    extend_bottom: 0,
                    fill: [255, 255, 255, 255],
                }),
                ..ImageState::default()
            },
            ..App::default()
        };
        app.clear_image();
        assert!(app.image.info.is_none());
    }

    /// `clear_image()` がステータスメッセージを初期状態にリセットすること
    #[test]
    fn clear_image_resets_status_message() {
        let mut app = App {
            status: "何か別のメッセージ".into(),
            ..App::default()
        };
        app.clear_image();
        assert_ne!(app.status, "");
        // デフォルトメッセージに戻っていることを確認
        assert!(app.status.contains("画像を開く"));
    }

    /// `clear_image()` が画像がない状態でもパニックせずに動作すること
    #[test]
    fn clear_image_from_initial_state_does_not_panic() {
        let mut app = App::default();
        // 既に空の状態でも安全に動作すること
        app.clear_image();
        assert!(app.image.original.is_none());
        assert!(app.image.trim_target().is_none());
    }

    // ======================================================================
    // load_image_from_path() / poll_io()
    // ======================================================================
    /// `load_image_from_path()` が存在しないファイルを指定した場合にエラーステータスを設定すること
    #[test]
    fn load_image_from_path_sets_error_status_for_missing_file() {
        let mut app = App::default();
        let bad_path = Path::new("/nonexistent/path/image.png");
        app.load_image_from_path(bad_path);
        let done = poll_io_until(&mut app, |a| a.status.contains("読み込みエラー"));
        assert!(done, "status = {}", app.status);
    }

    /// `load_image_from_path()` がエラー時に元画像を設定しないこと
    #[test]
    fn load_image_from_path_does_not_set_original_on_error() {
        let mut app = App::default();
        let bad_path = Path::new("/nonexistent/path/image.png");
        app.load_image_from_path(bad_path);
        poll_io_until(&mut app, |a| a.status.contains("読み込みエラー"));
        assert!(app.image.original.is_none());
    }

    /// `load_image_from_path()` がバックグラウンドで読み込んだ画像を元画像に設定すること
    #[test]
    fn load_image_from_path_sets_original_in_background() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("input.png");
        make_test_image().save(&path).unwrap();

        let mut app = App::default();
        app.load_image_from_path(&path);
        // 呼び出し直後は読み込み中であること
        assert!(app.status.contains("読み込み中"), "status = {}", app.status);

        let done = poll_io_until(&mut app, |a| a.image.original.is_some());

        assert!(done, "元画像が設定されなかった: {}", app.status);
        assert!(
            app.status.contains("読み込み完了"),
            "status = {}",
            app.status
        );
    }

    /// 読み込み中に `clear_image()` した場合、遅れて届いた読み込み結果を無視すること
    #[test]
    fn clear_image_discards_pending_load_result() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("input.png");
        make_test_image().save(&path).unwrap();

        let mut app = App::default();
        app.load_image_from_path(&path);
        app.clear_image();

        // 読み込みスレッドの完了を待ってから結果を受信する
        thread::sleep(Duration::from_millis(300));
        app.poll_io();

        assert!(app.image.original.is_none());
        assert!(app.status.contains("画像を開く"), "status = {}", app.status);
    }
}
