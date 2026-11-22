//! 一括トリミングの開始・キャンセル・結果の受信

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, TryRecvError};
use std::thread;

use eframe::egui;

use crate::app::state::{App, DialogResult};
use crate::batch_trim::{BatchProgress, BatchTrimParams, BatchTrimResult, run_batch_trim};
use crate::consts::SUPPORTED_IMAGE_EXTENSIONS;

/// 保存先フォルダ選択ダイアログの初期位置を決める
///
/// 前回の保存先を優先し、なければ先頭の入力ファイルのフォルダを使う
///
/// # Arguments
/// * `last` - 前回の保存先
/// * `paths` - トリミング対象の画像ファイル
///
/// # Returns
/// * `Option<PathBuf>` - 初期位置。決められない場合は `None`
fn initial_output_dir(last: Option<&Path>, paths: &[PathBuf]) -> Option<PathBuf> {
    last.map(Path::to_path_buf).or_else(|| {
        paths
            .first()
            .and_then(|p| p.parent())
            .filter(|p| !p.as_os_str().is_empty())
            .map(Path::to_path_buf)
    })
}

// ======================================================================
// 一括トリミング
// ======================================================================
impl App {
    /// 複数の画像ファイルを選択してまとめてトリミングし、指定フォルダへ保存する
    ///
    /// ダイアログは別スレッドで開き、選択後は `poll_io` 経由で保存先フォルダの選択に進む
    /// 処理もバックグラウンドスレッドで行うため UI はブロックしない
    /// 完了後は `poll_batch_result` でステータスを更新する
    ///
    /// # Arguments
    /// * `ctx` - egui のコンテキスト (再描画リクエストに使用)
    pub(in crate::app) fn batch_trim(&mut self, ctx: &egui::Context) {
        if self.batch.running || self.reject_file_work_while_updating() {
            return;
        }
        self.spawn_dialog(ctx, |dialog| {
            let paths = dialog
                .add_filter("画像", SUPPORTED_IMAGE_EXTENSIONS)
                .pick_files();
            DialogResult::BatchInputs(paths)
        });
    }

    /// 保存先フォルダを選択させ、指定ファイルの一括トリミングをバックグラウンドで開始する
    ///
    /// フォルダ選択後は `poll_io` 経由で `spawn_batch_trim` を呼ぶ
    ///
    /// # Arguments
    /// * `ctx` - egui のコンテキスト (再描画リクエストに使用)
    /// * `paths` - トリミング対象の画像ファイル
    /// * `skipped` - 呼び出し側で対象外として除外した件数 (完了時の要約に追記する)
    pub(super) fn start_batch_trim(
        &mut self,
        ctx: &egui::Context,
        paths: Vec<PathBuf>,
        skipped: usize,
    ) {
        if self.batch.running || self.reject_file_work_while_updating() {
            return;
        }
        let initial = initial_output_dir(self.batch.last_output_dir.as_deref(), &paths);
        self.spawn_dialog(ctx, move |dialog| {
            let dialog = dialog.set_title("保存先フォルダを選択");
            let dialog = match initial {
                Some(dir) => dialog.set_directory(dir),
                None => dialog,
            };
            let dir = dialog.pick_folder();
            DialogResult::BatchOutput {
                paths,
                skipped,
                dir,
            }
        });
    }

    /// 指定ファイルの一括トリミングをバックグラウンドで開始する
    ///
    /// # Arguments
    /// * `ctx` - egui のコンテキスト (再描画リクエストに使用)
    /// * `paths` - トリミング対象の画像ファイル
    /// * `skipped` - 呼び出し側で対象外として除外した件数 (完了時の要約に追記する)
    /// * `out_dir` - 保存先フォルダ
    pub(super) fn spawn_batch_trim(
        &mut self,
        ctx: &egui::Context,
        paths: Vec<PathBuf>,
        skipped: usize,
        out_dir: PathBuf,
    ) {
        // 二重起動を防ぐ
        // ダイアログを開いたまま更新の適用が始まった場合もここで止める
        if self.batch.running || self.reject_file_work_while_updating() {
            return;
        }

        let trim = self.prefs.trim_options();
        let format = self.prefs.output_format;
        let save_options = self.save_options();
        let ctx_clone = ctx.clone();
        let total = paths.len();

        self.batch.last_error_details.clear();
        self.batch.errors_open = false;
        self.batch.skipped_inputs = skipped;
        self.batch.last_output_dir = Some(out_dir.clone());
        self.batch.cancel.store(false, Ordering::Relaxed);
        let cancel_flag = Arc::clone(&self.batch.cancel);

        let (tx, rx) = mpsc::channel::<BatchProgress>();
        self.batch.rx = Some(rx);
        self.batch.running = true;
        self.status = format!("一括トリミング中... (0/{total})");

        let handle = thread::spawn(move || {
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
                    trim,
                    format,
                    save_options,
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
        self.batch.handle = Some(handle);
    }

    /// 実行中の一括トリミングをキャンセルする
    ///
    /// フラグをセットするのみ。処理中のファイル (最大 `BATCH_MAX_CONCURRENCY` 枚) は完了まで続け、未着手のファイルは処理しない
    pub(in crate::app) fn cancel_batch(&self) {
        self.batch.cancel.store(true, Ordering::Relaxed);
    }

    /// バックグラウンドの一括トリミング処理が完了していればステータスを更新する
    ///
    /// 毎フレーム呼び出すことで結果をポーリングする
    /// 進捗はステータスバーに表示し、エラー詳細は `batch.last_error_details` に保持して一覧ウィンドウで表示する
    /// 完了通知の前に送信側が切断された場合 (処理スレッドの panic など) は失敗として終了する
    pub(in crate::app) fn poll_batch_result(&mut self) {
        if !self.batch.running {
            return;
        }
        let mut done = false;
        while let Some(rx) = self.batch.rx.as_ref() {
            match rx.try_recv() {
                Ok(BatchProgress::Finished(result)) => {
                    let note = skipped_note(self.batch.skipped_inputs);
                    self.status = format!("{}{note}", result.summary());
                    self.batch.last_error_details = result.errors;
                    // 件数が多いとステータスバーでは読めないため、失敗があれば一覧を自動で開く
                    self.batch.errors_open = !self.batch.last_error_details.is_empty();
                    done = true;
                    break;
                }
                Ok(BatchProgress::Working { processed, total }) => {
                    self.status = format!("一括トリミング中... ({processed}/{total})");
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.status = "一括トリミングが異常終了しました".into();
                    self.batch.last_error_details.clear();
                    self.batch.errors_open = false;
                    done = true;
                    break;
                }
            }
        }
        // 受信側がない状態では完了を検知できないため、実行中のまま残さない
        if self.batch.rx.is_none() {
            done = true;
        }
        if done {
            self.batch.running = false;
            self.batch.rx = None;
            // 完了を送信済みのスレッドはすぐ終わるため、ここで回収する
            if let Some(handle) = self.batch.handle.take() {
                let _ = handle.join();
            }
        }
    }
}

/// 除外件数をステータスへ追記する補足文を返す (0 件なら空文字列)
pub(super) fn skipped_note(skipped: usize) -> String {
    if skipped == 0 {
        String::new()
    } else {
        format!(" (対応外の {skipped} 件を除外)")
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::mpsc;

    use super::{initial_output_dir, skipped_note};
    use crate::app::state::App;
    use crate::batch_trim::{BatchProgress, BatchTrimResult};

    /// 前回の保存先があれば入力ファイルのフォルダより優先すること
    #[test]
    fn initial_output_dir_prefers_last_output() {
        let last = PathBuf::from("out");
        let paths = vec![PathBuf::from("in").join("a.png")];
        assert_eq!(initial_output_dir(Some(&last), &paths), Some(last));
    }

    /// 前回の保存先がなければ先頭の入力ファイルのフォルダを使うこと
    ///
    /// 入力が空、または親フォルダが空のパスでは決めない
    #[test]
    fn initial_output_dir_falls_back_to_first_input_parent() {
        let paths = vec![PathBuf::from("in").join("a.png")];
        assert_eq!(initial_output_dir(None, &paths), Some(PathBuf::from("in")));
        assert_eq!(initial_output_dir(None, &[]), None);
        assert_eq!(initial_output_dir(None, &[PathBuf::from("a.png")]), None);
    }

    /// 除外件数が 0 のときのみ補足文が空になること
    #[test]
    fn skipped_note_is_empty_only_for_zero() {
        assert_eq!(skipped_note(0), "");
        assert!(skipped_note(3).contains("3 件"));
    }

    /// 完了通知の前に送信側が切断されると、一括トリミングを失敗として終了すること
    #[test]
    fn poll_batch_result_finishes_when_sender_disconnected() {
        let mut app = App::default();
        let (tx, rx) = mpsc::channel();
        app.batch.rx = Some(rx);
        app.batch.running = true;
        app.batch.last_error_details = vec!["前回のエラー".into()];
        // 進捗だけ送って完了を通知せずに終了したスレッドを再現する
        tx.send(BatchProgress::Working {
            processed: 1,
            total: 3,
        })
        .unwrap();
        drop(tx);

        app.poll_batch_result();

        // 実行中が解除され、再実行できる状態に戻る
        assert!(!app.batch.running);
        assert!(app.batch.rx.is_none());
        assert_eq!(app.batch.last_error_details, Vec::<String>::new());
        assert_eq!(app.status, "一括トリミングが異常終了しました");
    }

    /// 失敗したファイルがあるときのみ、完了時にエラー一覧を開くこと
    #[test]
    fn poll_batch_result_opens_error_list_only_when_errors_exist() {
        for (errors, expect_open) in [(vec!["a.png: 失敗".to_string()], true), (Vec::new(), false)]
        {
            let mut app = App::default();
            let (tx, rx) = mpsc::channel();
            app.batch.rx = Some(rx);
            app.batch.running = true;
            let mut result = BatchTrimResult::fatal(PathBuf::from("out"), "");
            result.fatal_error = None;
            result.errors = errors.clone();
            tx.send(BatchProgress::Finished(result)).unwrap();

            app.poll_batch_result();

            assert!(!app.batch.running);
            assert_eq!(app.batch.last_error_details, errors);
            assert_eq!(app.batch.errors_open, expect_open);
        }
    }

    /// 送信側が生きている間は、通知がなくても実行中のまま待つこと
    #[test]
    fn poll_batch_result_keeps_running_while_sender_alive() {
        let mut app = App::default();
        let (_tx, rx) = mpsc::channel();
        app.batch.rx = Some(rx);
        app.batch.running = true;

        app.poll_batch_result();

        assert!(app.batch.running);
        assert!(app.batch.rx.is_some());
    }
}
