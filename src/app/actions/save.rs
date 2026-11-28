//! トリミング後の画像の手動保存・自動保存

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::thread;

use crate::app::state::{App, AutoSaveKey, DialogResult, IoMsg, SaveKind, SaveRequest};
use crate::consts::is_supported_image_extension;
use crate::image_output::{self, OutputFormat, SaveOptions};
use crate::panic_message::catch_panic;
use crate::trim::apply_trim;

// ======================================================================
// 保存
// ======================================================================
impl App {
    /// ファイルダイアログを開いてトリミング後の画像をバックグラウンドで保存する
    ///
    /// 既定のファイル名と形式は自動保存・一括トリミングと同じ規則 (`trimmed_file_name`) で決め、
    /// ダイアログで入力された拡張子があればそれに従う
    /// 保存対象はダイアログを開いた時点の内容とし、選択後は `poll_io` 経由で保存を開始する
    /// 元画像の切り出しも保存スレッドで行う
    ///
    /// # Arguments
    pub(in crate::app) fn save_image(&mut self) {
        if self.reject_file_work_while_updating() {
            return;
        }
        let Some((original, info)) = self.current_trim_target() else {
            return;
        };
        let source = self.image.source_name.clone().unwrap_or_default();
        let format = self.prefs.output_format;
        // 形式は読み込み時に判定済みのものを使い、UI スレッドでファイルを開かない
        let ext = format.resolve_extension(&source, self.image.source_format);
        let file_name = format!("{}.{ext}", image_output::trimmed_file_stem(&source));
        let source_dir = self.image.source_dir.clone();
        let request = SaveRequest {
            original,
            info,
            icc_profile: self.image.icc_profile.clone(),
            dpi: self.image.dpi,
            ext,
        };
        self.spawn_dialog(move |dialog| {
            let mut dialog = dialog.set_file_name(file_name);
            // 元画像のフォルダが分かる場合はそこを初期位置にする
            if let Some(dir) = source_dir {
                dialog = dialog.set_directory(dir);
            }
            for f in save_dialog_formats(OutputFormat::from_extension(&request.ext)) {
                dialog = dialog.add_filter(f.label(), f.extensions());
            }
            let path = dialog.save_file();
            DialogResult::SaveImage { request, path }
        });
    }

    /// 選択された保存先へトリミング後の画像をバックグラウンドで保存する
    ///
    /// # Arguments
    /// * `request` - 保存対象
    /// * `path` - ダイアログで選択された保存先
    pub(super) fn start_manual_save(&mut self, request: SaveRequest, path: PathBuf) {
        // ダイアログを開いたまま更新の適用が始まった場合もここで止める
        if self.reject_file_work_while_updating() {
            return;
        }
        let path = with_default_extension(path, &request.ext);
        let options = self.save_options();
        self.spawn_save(SaveKind::Manual, move || {
            let img = apply_trim(&request.original, &request.info);
            image_output::save_image_with(
                &img,
                request.icc_profile.as_deref(),
                request.dpi,
                &path,
                options,
            )
            .map_err(|e| e.to_string())?;
            Ok(path)
        });
    }

    /// 現在の設定から保存時のエンコード設定を作る
    pub(super) fn save_options(&self) -> SaveOptions {
        SaveOptions {
            jpeg_quality: self.prefs.jpeg_quality,
            webp_quality: self.prefs.webp_quality,
            overwrite: self.prefs.overwrite_output,
        }
    }

    /// 自動保存が有効な場合にトリミング後の画像を保存先フォルダへバックグラウンドで保存する
    ///
    /// ファイル名は `{元画像名}_trimmed.{出力形式の拡張子}` とし、既存ファイルは連番で回避する (上書き設定が有効なときは上書きする)
    /// 直前の自動保存と同じ画像・矩形・形式・保存先なら保存しない
    /// 成功時は既存のステータスメッセージに追記し、エラー時のみ上書きする
    ///
    /// # Arguments
    pub(super) fn auto_save_if_enabled(&mut self) {
        // 更新の適用中は手動保存と同じく書き込みを始めない (自動実行のためステータスは変えない)
        if !self.prefs.auto_save || self.update.is_applying() {
            return;
        }
        let Some((original, info)) = self.current_trim_target() else {
            return;
        };
        let dir = self.prefs.auto_save_dir.clone();
        let format = self.prefs.output_format;
        let key = AutoSaveKey {
            load_request_id: self.io.load_request_id,
            info,
            format,
            jpeg_quality: self.prefs.jpeg_quality,
            webp_quality: self.prefs.webp_quality,
            overwrite: self.prefs.overwrite_output,
            dir: dir.clone(),
        };
        // 自動保存の OFF→ON などで同じ結果を連番ファイルとして増やさない
        if self.io.last_auto_save.as_ref() == Some(&key) {
            return;
        }
        // 実行中の重複保存を防ぐため開始時に記録し、失敗時は結果の受信側で消す
        self.io.last_auto_save = Some(key.clone());
        let icc_profile = self.image.icc_profile.clone();
        let dpi = self.image.dpi;
        let source = self.image.source_name.clone().unwrap_or_default();
        let stem = image_output::trimmed_file_stem(&source);
        // 形式は読み込み時に判定済みのものを使い、保存時にファイルを開き直さない
        let ext = format.resolve_extension(&source, self.image.source_format);
        let options = self.save_options();
        self.spawn_save(SaveKind::Auto(key), move || {
            fs::create_dir_all(&dir).map_err(|e| format!("自動保存フォルダの作成に失敗: {e}"))?;
            let img = apply_trim(&original, &info);
            let (path, _) = image_output::save_image_unique_with(
                &img,
                icc_profile.as_deref(),
                dpi,
                &dir,
                &stem,
                &ext,
                options,
            )
            .map_err(|e| {
                let cause = e
                    .source()
                    .map_or_else(|| e.to_string(), ToString::to_string);
                format!("自動保存エラー: {cause}")
            })?;
            Ok(path)
        });
    }

    /// 自動保存先フォルダを選ぶダイアログを開く
    ///
    /// ダイアログは現在の保存先を初期位置にして別スレッドで開き、選択後は `poll_io` で設定を更新する
    ///
    /// # Arguments
    pub(in crate::app) fn pick_auto_save_dir(&mut self) {
        let current = self.prefs.auto_save_dir.clone();
        self.spawn_dialog(move |dialog| {
            let dir = dialog.set_directory(current).pick_folder();
            DialogResult::AutoSaveDir(dir)
        });
    }

    /// 自動保存を要求する。再計算の待ち中・処理中なら完了後に保存する
    ///
    /// # Arguments
    pub(in crate::app) fn request_auto_save(&mut self) {
        if self.trim_worker.is_busy() {
            self.trim_worker.auto_save_after = true;
        } else {
            self.auto_save_if_enabled();
        }
    }

    /// 保存処理をワーカースレッドで実行し、完了を `IoMsg::Saved` で通知する
    ///
    /// 保存処理が panic した場合も失敗として通知する
    ///
    /// # Arguments
    /// * `kind` - 保存処理の種別
    /// * `job` - 保存先パス、または失敗時のステータスメッセージを返す保存処理
    fn spawn_save<F>(&mut self, kind: SaveKind, job: F)
    where
        F: FnOnce() -> Result<PathBuf, String> + Send + 'static,
    {
        let tx = self.io.tx.clone();
        let waker = self.waker.clone();
        let handle = thread::spawn(move || {
            let result = catch_panic(job)
                .unwrap_or_else(|msg| Err(format!("保存中にエラーが発生しました: {msg}")));
            let _ = tx.send(IoMsg::Saved { kind, result });
            waker.wake();
        });
        self.io.save_handles.push(handle);
    }
}

/// 保存先に対応画像の拡張子がなければ既定の拡張子を末尾に補う
///
/// 拡張子なしや `image v1.2` のように対応外の拡張子で終わる名前は形式を判定できないため、
/// 入力された名前を保ったまま `.{ext}` を付け足す
///
/// # Arguments
/// * `path` - ダイアログで入力された保存先
/// * `ext` - 補う拡張子 (ドットなし)
///
/// # Returns
/// * `PathBuf` - 対応画像の拡張子を持つ保存先
fn with_default_extension(path: PathBuf, ext: &str) -> PathBuf {
    let supported = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| is_supported_image_extension(&e.to_ascii_lowercase()));
    if supported {
        return path;
    }
    let mut name = path.into_os_string();
    name.push(".");
    name.push(ext);
    PathBuf::from(name)
}

/// 保存ダイアログのファイル種類フィルタに並べる形式を返す
///
/// 既定で選択される先頭のフィルタを `first` とし、残りは `OutputFormat::ALL` の順に並べる
///
/// # Arguments
/// * `first` - 先頭に置く形式。`None` のときは `OutputFormat::ALL` の順
fn save_dialog_formats(first: Option<OutputFormat>) -> Vec<OutputFormat> {
    let mut formats: Vec<OutputFormat> = OutputFormat::ALL
        .into_iter()
        .filter(|f| *f != OutputFormat::Original)
        .collect();
    if let Some(i) = formats.iter().position(|f| Some(*f) == first) {
        let f = formats.remove(i);
        formats.insert(0, f);
    }
    formats
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::{save_dialog_formats, with_default_extension};
    use crate::app::actions::test_helpers::{
        poll_io_until, poll_trim_until_idle, trimmed_image_state,
    };
    use crate::app::state::{App, ImageState, SaveKind, Settings, UpdateUiPhase};
    use crate::image_output::OutputFormat;

    // ======================================================================
    // auto_save_if_enabled()
    // ======================================================================
    /// `auto_save_if_enabled()` が自動保存が無効な場合に何もしないこと
    #[test]
    fn auto_save_skips_when_disabled() {
        let mut app = App {
            prefs: Settings {
                auto_save: false,
                ..Settings::default()
            },
            image: trimmed_image_state(),
            ..App::default()
        };
        let status_before = app.status.clone();
        app.auto_save_if_enabled();
        // 保存スレッドを起動しないこと
        assert!(!app.io.is_saving());
        assert_eq!(app.status, status_before);
    }

    /// `auto_save_if_enabled()` が更新の適用中に保存を開始しないこと
    #[test]
    fn auto_save_skips_while_updating() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = App {
            prefs: Settings {
                auto_save: true,
                auto_save_dir: tmp.path().to_path_buf(),
                ..Settings::default()
            },
            image: trimmed_image_state(),
            ..App::default()
        };
        app.update.phase = UpdateUiPhase::Exiting;
        let status_before = app.status.clone();

        app.auto_save_if_enabled();

        // 保存スレッドを起動せず、ステータスも変えないこと
        assert!(!app.io.is_saving());
        assert_eq!(app.status, status_before);
        assert!(app.io.last_auto_save.is_none());
    }

    /// `auto_save_if_enabled()` がトリミング後の画像がない場合に何もしないこと
    #[test]
    fn auto_save_skips_when_no_trimmed_image() {
        let mut app = App {
            prefs: Settings {
                auto_save: true,
                ..Settings::default()
            },
            ..App::default()
        };
        let status_before = app.status.clone();
        app.auto_save_if_enabled();
        assert!(!app.io.is_saving());
        assert_eq!(app.status, status_before);
    }

    /// `auto_save_if_enabled()` が自動保存が有効でトリミング後の画像がある場合に PNG ファイルを保存すること
    #[test]
    fn auto_save_writes_png_file_to_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = App {
            prefs: Settings {
                auto_save: true,
                auto_save_dir: tmp.path().to_path_buf(),
                ..Settings::default()
            },
            image: trimmed_image_state(),
            ..App::default()
        };
        app.auto_save_if_enabled();

        // ステータスが自動保存パスを含むこと
        let done = poll_io_until(&mut app, |a| a.status.contains("自動保存"));
        assert!(done, "status = {}", app.status);

        // 実際にファイルが生成されていること
        let saved_files: Vec<_> = fs::read_dir(tmp.path())
            .map_or_default(|rd| rd.filter_map(std::result::Result::ok).collect());
        assert!(
            !saved_files.is_empty(),
            "自動保存フォルダにファイルが存在しない"
        );
    }

    /// 自動保存が元画像名でファイルを作成し、2 回目は上書きせず連番を付けること
    #[test]
    fn auto_save_uses_source_name_and_avoids_overwrite() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = App {
            prefs: Settings {
                auto_save: true,
                auto_save_dir: tmp.path().to_path_buf(),
                ..Settings::default()
            },
            image: ImageState {
                source_name: Some("photo.png".into()),
                ..trimmed_image_state()
            },
            ..App::default()
        };

        // 1 回目は `{元画像名}_trimmed.png`
        app.auto_save_if_enabled();
        let first = tmp.path().join("photo_trimmed.png");
        assert!(poll_io_until(&mut app, |_| first.is_file()));

        // 別の画像を同名で読み込んだ場合は既存ファイルを残したまま連番で保存する
        app.io.abort_load();
        app.auto_save_if_enabled();
        let second = tmp.path().join("photo_trimmed_2.png");
        assert!(poll_io_until(&mut app, |_| second.is_file()));
        assert!(first.is_file());
    }

    /// 自動保存が出力形式の設定に従い、`Original` では元画像の形式を維持すること
    #[test]
    fn auto_save_follows_output_format() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = App {
            prefs: Settings {
                auto_save: true,
                auto_save_dir: tmp.path().to_path_buf(),
                ..Settings::default()
            },
            image: ImageState {
                source_name: Some("photo.JPG".into()),
                ..trimmed_image_state()
            },
            ..App::default()
        };

        // `Original` は元の拡張子 (小文字化) で保存する
        app.auto_save_if_enabled();
        let original = tmp.path().join("photo_trimmed.jpg");
        assert!(poll_io_until(&mut app, |_| original.is_file()));

        // 形式を指定するとその形式で保存する
        app.prefs.output_format = OutputFormat::Webp;
        app.auto_save_if_enabled();
        let webp = tmp.path().join("photo_trimmed.webp");
        assert!(poll_io_until(&mut app, |_| webp.is_file()));
    }

    /// 自動保存が `Original` で、読み込み時に判定した形式が拡張子と食い違う場合は判定した形式で保存すること
    ///
    /// `source_name` はファイル名だけで、カレントディレクトリに実ファイルは存在しない
    #[test]
    fn auto_save_original_uses_detected_source_format() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = App {
            prefs: Settings {
                auto_save: true,
                auto_save_dir: tmp.path().to_path_buf(),
                ..Settings::default()
            },
            image: ImageState {
                source_name: Some("photo.jpg".into()),
                source_format: Some(OutputFormat::Png),
                ..trimmed_image_state()
            },
            ..App::default()
        };

        app.auto_save_if_enabled();
        let saved = tmp.path().join("photo_trimmed.png");
        assert!(poll_io_until(&mut app, |_| saved.is_file()));
        assert!(!tmp.path().join("photo_trimmed.jpg").exists());
    }

    /// `auto_save_if_enabled()` がディレクトリ作成に失敗した場合にエラーステータスを設定すること
    #[test]
    fn auto_save_error_status_when_dir_creation_fails() {
        // 既存ファイルをディレクトリとして扱おうとすると create_dir_all が失敗する
        let blocker = tempfile::NamedTempFile::new().expect("テスト用ファイルの作成に失敗");
        let existing_file = blocker.path();

        // ファイルのパス配下にサブディレクトリを作ろうとするので失敗する
        let mut app = App {
            prefs: Settings {
                auto_save: true,
                auto_save_dir: existing_file.join("subdir_cannot_exist"),
                ..Settings::default()
            },
            image: trimmed_image_state(),
            ..App::default()
        };
        app.auto_save_if_enabled();
        let done = poll_io_until(&mut app, |a| {
            a.status.contains("失敗") || a.status.contains("エラー")
        });

        assert!(done, "エラーメッセージを期待したが: {}", app.status);
    }

    /// 自動保存に失敗した後は、同じ条件でも再度保存できること
    #[test]
    fn auto_save_retries_same_key_after_failure() {
        let blocker = tempfile::NamedTempFile::new().expect("テスト用ファイルの作成に失敗");
        let mut app = App {
            prefs: Settings {
                auto_save: true,
                auto_save_dir: blocker.path().join("subdir_cannot_exist"),
                ..Settings::default()
            },
            image: trimmed_image_state(),
            ..App::default()
        };
        app.auto_save_if_enabled();
        assert!(poll_io_until(&mut app, |a| a.status.contains("失敗")
            || a.status.contains("エラー")));

        // 失敗した内容は記録に残らず、同じ条件で再度保存を始めること
        assert!(app.io.last_auto_save.is_none());
        app.auto_save_if_enabled();
        assert!(app.io.last_auto_save.is_some());
    }

    /// `spawn_save()` が保存処理の panic を失敗として通知すること
    #[test]
    fn spawn_save_reports_panic_as_failure() {
        let mut app = App {
            status: "初期".into(),
            ..App::default()
        };

        app.spawn_save(SaveKind::Manual, || panic!("テスト用の panic"));
        let done = poll_io_until(&mut app, |a| a.status != "初期");

        assert!(done, "失敗の通知を期待したが: {}", app.status);
        assert!(app.status.contains("保存中にエラー"), "{}", app.status);
        assert!(app.status.contains("テスト用の panic"), "{}", app.status);
    }

    /// 再計算の待ち中に自動保存を有効にした場合、完了後の結果で保存すること
    #[test]
    fn request_auto_save_waits_for_pending_trim() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = App {
            prefs: Settings {
                auto_save: true,
                auto_save_dir: tmp.path().to_path_buf(),
                ..Settings::default()
            },
            image: trimmed_image_state(),
            ..App::default()
        };
        app.image.cache_key = Some(app.prefs.trim_options());
        app.prefs.tolerance = 1;
        app.schedule_trim(true, false, false);

        // 待ち中は保存せず、完了後の保存として予約する
        app.request_auto_save();
        assert!(!app.io.is_saving());
        assert!(app.trim_worker.auto_save_after);

        // 完了後に新しい設定の結果で保存される
        assert!(poll_trim_until_idle(&mut app));
        assert_eq!(app.image.cache_key, Some(app.prefs.trim_options()));
        let done = poll_io_until(&mut app, |a| a.status.contains("自動保存"));
        assert!(done, "status = {}", app.status);
        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 1);
    }

    /// 同じ結果で自動保存を繰り返し要求しても、連番ファイルを増やさないこと
    #[test]
    fn auto_save_skips_same_result() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = App {
            prefs: Settings {
                auto_save: true,
                auto_save_dir: tmp.path().to_path_buf(),
                ..Settings::default()
            },
            image: trimmed_image_state(),
            ..App::default()
        };
        app.status = "読み込み完了: a.png".into();

        app.request_auto_save();
        assert!(poll_io_until(&mut app, |a| a.status.contains("自動保存")));
        let status_after_first = app.status.clone();

        // 2 回目は保存を開始せず、ステータスも変えない
        app.request_auto_save();
        assert!(!app.io.is_saving());
        assert_eq!(app.status, status_after_first);
        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 1);
    }

    /// 自動保存を繰り返しても、ステータスへの追記が重ならないこと
    #[test]
    fn auto_save_status_does_not_accumulate() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = App {
            prefs: Settings {
                auto_save: true,
                auto_save_dir: tmp.path().to_path_buf(),
                ..Settings::default()
            },
            image: trimmed_image_state(),
            ..App::default()
        };
        app.status = "読み込み完了: a.png".into();

        app.auto_save_if_enabled();
        assert!(poll_io_until(&mut app, |a| a.status.contains("自動保存")));

        // 形式を変えて再保存すると、前回の追記を置き換える
        app.prefs.output_format = OutputFormat::Webp;
        app.auto_save_if_enabled();
        assert!(poll_io_until(&mut app, |a| a.status.contains(".webp")));
        assert!(app.status.starts_with("読み込み完了: a.png  (自動保存: "));
        assert_eq!(
            app.status.matches("自動保存").count(),
            1,
            "status = {}",
            app.status
        );

        // 別のメッセージが表示された後は、そのメッセージに追記する
        app.status = "クリップボードへコピーしました".into();
        app.prefs.output_format = OutputFormat::Png;
        app.auto_save_if_enabled();
        assert!(poll_io_until(&mut app, |a| a.status.contains("自動保存")));
        assert!(
            app.status
                .starts_with("クリップボードへコピーしました  (自動保存: ")
        );
    }

    /// 保存先に対応画像の拡張子がない場合だけ既定の拡張子を末尾に補うこと
    ///
    /// `image v1.2` のように対応外の拡張子で終わる名前は置き換えず付け足す
    #[test]
    fn with_default_extension_appends_only_when_unsupported() {
        let append = |name: &str| with_default_extension(PathBuf::from(name), "png");

        // 拡張子なしと対応外の拡張子は末尾に補う
        assert_eq!(append("out"), PathBuf::from("out.png"));
        assert_eq!(append("image v1.2"), PathBuf::from("image v1.2.png"));
        assert_eq!(append("report.pdf"), PathBuf::from("report.pdf.png"));

        // 対応画像の拡張子は大文字小文字を問わずそのまま使う
        assert_eq!(append("out.jpg"), PathBuf::from("out.jpg"));
        assert_eq!(append("out.WEBP"), PathBuf::from("out.WEBP"));
    }

    /// 保存ダイアログのフィルタが指定形式を先頭に、`Original` を除く全形式を並べること
    #[test]
    fn save_dialog_formats_puts_selected_first() {
        let formats = save_dialog_formats(Some(OutputFormat::Webp));
        assert_eq!(formats.first(), Some(&OutputFormat::Webp));
        assert_eq!(formats.len(), OutputFormat::ALL.len() - 1);
        assert!(!formats.contains(&OutputFormat::Original));

        // 指定なしは `ALL` の順
        assert_eq!(save_dialog_formats(None).first(), Some(&OutputFormat::Png));
    }
}
