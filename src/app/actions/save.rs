//! トリミング後の画像の手動保存・自動保存

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::thread;

use crate::app::state::{App, AutoSaveKey, DialogResult, IoMsg, SaveKind, SaveRequest};
use crate::image_output::{self, OutputFormat, SaveOptions};
use crate::panic_message::catch_panic;
use crate::trim::apply_trim;

use eframe::egui;

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
    /// * `ctx` - egui のコンテキスト (完了時の再描画に使用)
    pub(in crate::app) fn save_image(&mut self, ctx: &egui::Context) {
        let Some((original, info)) = self.current_trim_target() else {
            return;
        };
        let source = self.image.source_name.clone().unwrap_or_default();
        let format = self.prefs.output_format;
        let ext = format.output_extension(&source);
        let request = SaveRequest {
            original,
            info,
            icc_profile: self.image.icc_profile.clone(),
            ext,
        };
        let file_name = image_output::trimmed_file_name(&source, format);
        self.spawn_dialog(ctx, move || {
            let mut dialog = rfd::FileDialog::new().set_file_name(file_name);
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
    /// * `ctx` - egui のコンテキスト (完了時の再描画に使用)
    /// * `request` - 保存対象
    /// * `path` - ダイアログで選択された保存先
    pub(super) fn start_manual_save(
        &mut self,
        ctx: &egui::Context,
        request: SaveRequest,
        mut path: PathBuf,
    ) {
        // 拡張子なしで入力された場合は形式を判定できないため設定の形式とする
        if path.extension().is_none() {
            path.set_extension(&request.ext);
        }
        let options = self.save_options();
        self.spawn_save(ctx, SaveKind::Manual, move || {
            let img = apply_trim(&request.original, &request.info);
            image_output::save_image_with(&img, request.icc_profile.as_deref(), &path, options)
                .map_err(|e| e.to_string())?;
            Ok(path)
        });
    }

    /// 現在の設定から保存時のエンコード設定を作る
    pub(super) fn save_options(&self) -> SaveOptions {
        SaveOptions {
            jpeg_quality: self.prefs.jpeg_quality,
        }
    }

    /// 自動保存が有効な場合にトリミング後の画像を保存先フォルダへバックグラウンドで保存する
    ///
    /// ファイル名は `{元画像名}_trimmed.{出力形式の拡張子}` とし、既存ファイルは上書きせず連番で回避する
    /// 直前の自動保存と同じ画像・矩形・形式・保存先なら保存しない
    /// 成功時は既存のステータスメッセージに追記し、エラー時のみ上書きする
    ///
    /// # Arguments
    /// * `ctx` - egui のコンテキスト (完了時の再描画に使用)
    pub(super) fn auto_save_if_enabled(&mut self, ctx: &egui::Context) {
        if !self.prefs.auto_save {
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
            dir: dir.clone(),
        };
        // 自動保存の OFF→ON などで同じ結果を連番ファイルとして増やさない
        if self.io.last_auto_save.as_ref() == Some(&key) {
            return;
        }
        self.io.last_auto_save = Some(key);
        let icc_profile = self.image.icc_profile.clone();
        let source = self.image.source_name.clone().unwrap_or_default();
        let options = self.save_options();
        self.spawn_save(ctx, SaveKind::Auto, move || {
            fs::create_dir_all(&dir).map_err(|e| format!("自動保存フォルダの作成に失敗: {e}"))?;
            let img = apply_trim(&original, &info);
            let (path, _) = image_output::save_trimmed_output_with(
                &img,
                icc_profile.as_deref(),
                &source,
                &dir,
                format,
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
    /// * `ctx` - egui のコンテキスト (完了時の再描画に使用)
    pub(in crate::app) fn pick_auto_save_dir(&mut self, ctx: &egui::Context) {
        let current = self.prefs.auto_save_dir.clone();
        self.spawn_dialog(ctx, move || {
            let dir = rfd::FileDialog::new().set_directory(current).pick_folder();
            DialogResult::AutoSaveDir(dir)
        });
    }

    /// 自動保存を要求する。再計算の待ち中・処理中なら完了後に保存する
    ///
    /// # Arguments
    /// * `ctx` - egui のコンテキスト (完了時の再描画に使用)
    pub(in crate::app) fn request_auto_save(&mut self, ctx: &egui::Context) {
        if self.trim_worker.is_busy() {
            self.trim_worker.auto_save_after = true;
        } else {
            self.auto_save_if_enabled(ctx);
        }
    }

    /// 保存処理をワーカースレッドで実行し、完了を `IoMsg::Saved` で通知する
    ///
    /// 保存処理が panic した場合も失敗として通知する
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
            let result = catch_panic(job)
                .unwrap_or_else(|msg| Err(format!("保存中にエラーが発生しました: {msg}")));
            let _ = tx.send(IoMsg::Saved { kind, result });
            ctx_clone.request_repaint();
        });
        self.io.save_handles.push(handle);
    }
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

    use crate::app::actions::test_helpers::{
        poll_io_until, poll_trim_until_idle, trimmed_image_state,
    };
    use crate::app::state::{App, ImageState, SaveKind, Settings};
    use crate::image_output::OutputFormat;

    use super::save_dialog_formats;
    use eframe::egui;

    // ======================================================================
    // auto_save_if_enabled()
    // ======================================================================
    /// `auto_save_if_enabled()` が自動保存が無効な場合に何もしないことを確認する
    #[test]
    fn auto_save_skips_when_disabled() {
        let ctx = egui::Context::default();
        let mut app = App {
            prefs: Settings {
                auto_save: false,
                ..Settings::default()
            },
            image: trimmed_image_state(),
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
            prefs: Settings {
                auto_save: true,
                ..Settings::default()
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
        app.auto_save_if_enabled(&ctx);

        // ステータスが自動保存パスを含むこと
        let done = poll_io_until(&mut app, &ctx, |a| a.status.contains("自動保存"));
        assert!(done, "status = {}", app.status);

        // 実際にファイルが生成されていること
        let saved_files: Vec<_> = fs::read_dir(tmp.path())
            .map(|rd| rd.filter_map(std::result::Result::ok).collect())
            .unwrap_or_default();
        assert!(
            !saved_files.is_empty(),
            "自動保存フォルダにファイルが存在しない"
        );
    }

    /// 自動保存が元画像名でファイルを作成し、2 回目は上書きせず連番を付けることを確認する
    #[test]
    fn auto_save_uses_source_name_and_avoids_overwrite() {
        let ctx = egui::Context::default();
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
        app.auto_save_if_enabled(&ctx);
        let first = tmp.path().join("photo_trimmed.png");
        assert!(poll_io_until(&mut app, &ctx, |_| first.is_file()));

        // 別の画像を同名で読み込んだ場合は既存ファイルを残したまま連番で保存する
        app.io.abort_load();
        app.auto_save_if_enabled(&ctx);
        let second = tmp.path().join("photo_trimmed_2.png");
        assert!(poll_io_until(&mut app, &ctx, |_| second.is_file()));
        assert!(first.is_file());
    }

    /// 自動保存が出力形式の設定に従い、`Original` では元画像の形式を維持することを確認する
    #[test]
    fn auto_save_follows_output_format() {
        let ctx = egui::Context::default();
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
        app.auto_save_if_enabled(&ctx);
        let original = tmp.path().join("photo_trimmed.jpg");
        assert!(poll_io_until(&mut app, &ctx, |_| original.is_file()));

        // 形式を指定するとその形式で保存する
        app.prefs.output_format = OutputFormat::Webp;
        app.auto_save_if_enabled(&ctx);
        let webp = tmp.path().join("photo_trimmed.webp");
        assert!(poll_io_until(&mut app, &ctx, |_| webp.is_file()));
    }

    /// `auto_save_if_enabled()` がディレクトリ作成に失敗した場合にエラーステータスを設定することを確認する
    #[test]
    fn auto_save_error_status_when_dir_creation_fails() {
        let ctx = egui::Context::default();
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
        app.auto_save_if_enabled(&ctx);
        let done = poll_io_until(&mut app, &ctx, |a| {
            a.status.contains("失敗") || a.status.contains("エラー")
        });

        assert!(done, "エラーメッセージを期待したが: {}", app.status);
    }

    /// `spawn_save()` が保存処理の panic を失敗として通知することを確認する
    #[test]
    fn spawn_save_reports_panic_as_failure() {
        let ctx = egui::Context::default();
        let mut app = App {
            status: "初期".into(),
            ..App::default()
        };

        app.spawn_save(&ctx, SaveKind::Manual, || panic!("テスト用の panic"));
        let done = poll_io_until(&mut app, &ctx, |a| a.status != "初期");

        assert!(done, "失敗の通知を期待したが: {}", app.status);
        assert!(app.status.contains("保存中にエラー"), "{}", app.status);
        assert!(app.status.contains("テスト用の panic"), "{}", app.status);
    }

    /// 再計算の待ち中に自動保存を有効にした場合、完了後の結果で保存することを確認する
    #[test]
    fn request_auto_save_waits_for_pending_trim() {
        let ctx = egui::Context::default();
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
        app.image.cache_key = Some((0, 0, None));
        app.prefs.tolerance = 1;
        app.schedule_trim(&ctx, true, false, false);

        // 待ち中は保存せず、完了後の保存として予約する
        app.request_auto_save(&ctx);
        assert!(!app.io.is_saving());
        assert!(app.trim_worker.auto_save_after);

        // 完了後に新しい設定の結果で保存される
        assert!(poll_trim_until_idle(&mut app, &ctx));
        assert_eq!(app.image.cache_key, Some((1, 0, None)));
        let done = poll_io_until(&mut app, &ctx, |a| a.status.contains("自動保存"));
        assert!(done, "status = {}", app.status);
        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 1);
    }

    /// 同じ結果で自動保存を繰り返し要求しても、連番ファイルを増やさないことを確認する
    #[test]
    fn auto_save_skips_same_result() {
        let ctx = egui::Context::default();
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

        app.request_auto_save(&ctx);
        assert!(poll_io_until(&mut app, &ctx, |a| a
            .status
            .contains("自動保存")));
        let status_after_first = app.status.clone();

        // 2 回目は保存を開始せず、ステータスも変えない
        app.request_auto_save(&ctx);
        assert!(!app.io.is_saving());
        assert_eq!(app.status, status_after_first);
        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 1);
    }

    /// 自動保存を繰り返しても、ステータスへの追記が重ならないことを確認する
    #[test]
    fn auto_save_status_does_not_accumulate() {
        let ctx = egui::Context::default();
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

        app.auto_save_if_enabled(&ctx);
        assert!(poll_io_until(&mut app, &ctx, |a| a
            .status
            .contains("自動保存")));

        // 形式を変えて再保存すると、前回の追記を置き換える
        app.prefs.output_format = OutputFormat::Webp;
        app.auto_save_if_enabled(&ctx);
        assert!(poll_io_until(&mut app, &ctx, |a| a
            .status
            .contains(".webp")));
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
        app.auto_save_if_enabled(&ctx);
        assert!(poll_io_until(&mut app, &ctx, |a| a
            .status
            .contains("自動保存")));
        assert!(
            app.status
                .starts_with("クリップボードへコピーしました  (自動保存: ")
        );
    }

    /// 保存ダイアログのフィルタが指定形式を先頭に、`Original` を除く全形式を並べることを確認する
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
