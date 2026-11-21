//! 一括トリミングのコア処理

use std::error::Error;
use std::fmt;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, PoisonError, mpsc};
use std::thread;

use crate::consts::{BATCH_MAX_CONCURRENCY, BATCH_MEMORY_BUDGET_BYTES, BATCH_PEAK_MEMORY_FACTOR};
use crate::image_load::{LoadError, LoadedImage, open_image_with_icc, probe_decoded_bytes};
use crate::image_output::{OutputFormat, SaveOptions, save_trimmed_output_with};
use crate::panic_message::catch_panic;
use crate::trim::trim;

// ======================================================================
// エラー型
// ======================================================================
/// 一括トリミングで 1 ファイルを処理する際のエラー
#[derive(Debug)]
pub enum BatchFileError {
    /// 入力画像の読み込みエラー
    Load(LoadError),
    /// 出力画像の保存エラー
    Save(crate::image_output::SaveError),
    /// 処理中に panic した (メッセージは panic の内容)
    Panic(String),
}

impl From<LoadError> for BatchFileError {
    fn from(value: LoadError) -> Self {
        Self::Load(value)
    }
}

impl From<crate::image_output::SaveError> for BatchFileError {
    fn from(value: crate::image_output::SaveError) -> Self {
        Self::Save(value)
    }
}

impl fmt::Display for BatchFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Load(e) => e.fmt(f),
            Self::Save(e) => e.fmt(f),
            Self::Panic(msg) => write!(f, "処理中に内部エラーが発生: {msg}"),
        }
    }
}

impl Error for BatchFileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Load(e) => Some(e),
            Self::Save(e) => Some(e),
            Self::Panic(_) => None,
        }
    }
}

// ======================================================================
// パラメータ・結果
// ======================================================================
/// 一括トリミングのトリム設定
#[derive(Debug, Clone, Copy)]
pub struct BatchTrimParams {
    /// 余白検出の許容度 (0–100)
    pub tolerance: u32,
    /// トリミング後に残す余白ピクセル数
    pub padding: u32,
    /// 手動指定の背景色。`None` のとき四辺から自動検出する
    pub bg_override: Option<[u8; 4]>,
    /// 出力形式
    pub format: OutputFormat,
    /// 保存時のエンコード設定
    pub save_options: SaveOptions,
}

/// バックグラウンド一括処理の進捗通知
#[derive(Debug, Clone)]
pub enum BatchProgress {
    /// 処理中 (`processed` は完了したファイル数)
    Working {
        /// 処理済みファイル数
        processed: usize,
        /// 対象ファイル総数
        total: usize,
    },
    /// 完了
    Finished(BatchTrimResult),
}

/// 一括トリミングの集計結果
#[derive(Debug, Clone)]
pub struct BatchTrimResult {
    /// 正常に保存したファイル数
    pub saved: usize,
    /// 連番サフィックスで衝突回避した保存数
    pub renamed: usize,
    /// 失敗したファイルごとのメッセージ (`{ファイル名}: {理由}`)
    pub errors: Vec<String>,
    /// ユーザーにより途中キャンセルされたか
    pub cancelled: bool,
    /// 対象ファイル総数
    pub total: usize,
    /// 出力先ディレクトリ
    pub out_dir: PathBuf,
    /// 出力フォルダ作成など、バッチ全体の失敗メッセージ
    pub fatal_error: Option<String>,
}

impl BatchTrimResult {
    /// 出力フォルダ作成失敗など、処理開始前の致命的エラーの結果を生成する
    ///
    /// # Arguments
    /// * `out_dir` - 出力先ディレクトリ
    /// * `message` - ステータスバーに表示する失敗メッセージ
    ///
    /// # Returns
    /// * `Self` - 処理件数が 0 で `fatal_error` を設定した結果
    #[must_use]
    pub fn fatal(out_dir: PathBuf, message: impl Into<String>) -> Self {
        Self {
            saved: 0,
            renamed: 0,
            errors: Vec::new(),
            cancelled: false,
            total: 0,
            out_dir,
            fatal_error: Some(message.into()),
        }
    }

    /// ステータスバー向けの要約メッセージ (1 行) を生成する
    ///
    /// # Arguments
    /// * `self` - 一括処理の結果
    ///
    /// # Returns
    /// * `String` - 致命的エラーがあればそのメッセージ、なければ件数と出力先の要約
    #[must_use]
    pub fn summary(&self) -> String {
        if let Some(msg) = &self.fatal_error {
            return msg.clone();
        }

        let dest = self.out_dir.display();
        if self.cancelled {
            let renamed_note = if self.renamed > 0 {
                format!("、{renamed} 件は連番回避", renamed = self.renamed)
            } else {
                String::new()
            };
            return format!(
                "一括トリミングをキャンセルしました ({saved}/{total} 保存済み{renamed_note}) → {dest}",
                saved = self.saved,
                total = self.total,
                renamed_note = renamed_note,
            );
        }

        let error_count = self.errors.len();
        if error_count == 0 {
            let renamed_note = if self.renamed > 0 {
                format!(" ({renamed} 件は連番回避)", renamed = self.renamed)
            } else {
                String::new()
            };
            return format!(
                "一括トリミング完了: {saved}/{total} ファイルを保存{renamed_note} → {dest}",
                saved = self.saved,
                total = self.total,
                renamed_note = renamed_note,
            );
        }

        let first = self.errors.first().map_or("", String::as_str);
        if error_count == 1 {
            format!(
                "一括トリミング完了: {saved}/{total} 成功、1 件エラー → {dest} ({first})",
                saved = self.saved,
                total = self.total,
            )
        } else {
            format!(
                "一括トリミング完了: {saved}/{total} 成功、{error_count} 件エラー → {dest} (先頭: {first})",
                saved = self.saved,
                total = self.total,
            )
        }
    }
}

// ======================================================================
// 一括処理
// ======================================================================
/// 複数画像をトリミングして `out_dir` へ保存する
///
/// ファイル単位で最大 `BATCH_MAX_CONCURRENCY` 枚を並列に処理する
/// 同時に処理するファイルの見積もりメモリの合計は `BATCH_MEMORY_BUDGET_BYTES` までに抑える
/// 各ファイルの処理開始前にキャンセルフラグを確認し、処理中のファイルは完了まで続ける
/// 出力パスが既存ファイルと衝突する場合は連番サフィックスで回避する
/// 1 ファイルの処理が panic した場合はそのファイルを `BatchFileError::Panic` として集計し、残りを続ける
///
/// # Arguments
/// * `paths`        - 入力ファイルパス列
/// * `out_dir`      - 出力先ディレクトリ (呼び出し側で作成済みであること)
/// * `params`       - トリム設定
/// * `cancel`       - `true` になると未着手のファイルを処理せずに打ち切る
/// * `on_progress`  - 各ファイル完了後に `(処理済み数, 総数)` で呼ばれる (呼び出し元スレッドで実行)
///
/// # Returns
/// * `BatchTrimResult` - 保存件数・連番回避件数・失敗一覧・キャンセル有無
#[must_use]
pub fn run_batch_trim(
    paths: &[PathBuf],
    out_dir: &Path,
    params: BatchTrimParams,
    cancel: &AtomicBool,
    on_progress: impl FnMut(usize, usize),
) -> BatchTrimResult {
    let workers = thread::available_parallelism()
        .map_or(1, NonZeroUsize::get)
        .min(BATCH_MAX_CONCURRENCY);
    run_batch_trim_with_workers(paths, out_dir, params, cancel, on_progress, workers)
}

/// 並列数を指定して一括トリミングを行う
///
/// ワーカーは共有インデックスから次のファイルを取り出して処理し、結果をチャネルで返す
/// 集計と `on_progress` の呼び出しは呼び出し元スレッドで行う
/// 並列数が 1 のときはスレッドを起こさず呼び出し元スレッドで逐次処理する
/// 各ファイルはデコード前に見積もったメモリを `MemoryBudget` から確保してから処理する
///
/// # Arguments
/// * `workers` - 同時に処理するファイル数の上限 (0 は 1 として扱う)
fn run_batch_trim_with_workers(
    paths: &[PathBuf],
    out_dir: &Path,
    params: BatchTrimParams,
    cancel: &AtomicBool,
    on_progress: impl FnMut(usize, usize),
    workers: usize,
) -> BatchTrimResult {
    let budget = MemoryBudget::new(BATCH_MEMORY_BUDGET_BYTES);
    run_batch_with(paths, out_dir, cancel, on_progress, workers, |path| {
        process_one(path, out_dir, params, &budget)
    })
}

/// 1 ファイル分の処理を差し替えて一括処理する
///
/// `process` の panic は `catch_unwind` で捕捉し、そのファイルのエラーとして集計する
/// (捕捉しないと `thread::scope` が panic を呼び出し元へ伝え、完了を通知できなくなる)
///
/// # Arguments
/// * `process` - 1 ファイル分の処理。保存時に連番で衝突回避したら `Ok(true)` を返す
fn run_batch_with(
    paths: &[PathBuf],
    out_dir: &Path,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(usize, usize),
    workers: usize,
    process: impl Fn(&Path) -> Result<bool, BatchFileError> + Sync,
) -> BatchTrimResult {
    let total = paths.len();
    let workers = workers.clamp(1, total.max(1));
    let queue = JobQueue {
        paths,
        cancel,
        next: AtomicUsize::new(0),
        skipped: AtomicBool::new(false),
    };
    let mut tally = Tally::default();

    if workers == 1 {
        while let Some((index, path)) = queue.claim() {
            tally.record(index, process_guarded(&process, path));
            on_progress(tally.processed(), total);
        }
    } else {
        thread::scope(|s| {
            let (tx, rx) = mpsc::channel::<(usize, Result<bool, BatchFileError>)>();
            for _ in 0..workers {
                let tx = tx.clone();
                let queue = &queue;
                let process = &process;
                s.spawn(move || {
                    while let Some((index, path)) = queue.claim() {
                        if tx.send((index, process_guarded(process, path))).is_err() {
                            break;
                        }
                    }
                });
            }
            // 全ワーカーの終了で受信ループを抜けるため、手元の送信側を先に破棄する
            drop(tx);

            for (index, result) in rx {
                tally.record(index, result);
                on_progress(tally.processed(), total);
            }
        });
    }

    // 完了順に依存しないよう、エラーは入力順に並べる
    tally.errors.sort_unstable_by_key(|(index, _)| *index);
    let errors = tally
        .errors
        .into_iter()
        .map(|(index, e)| {
            let name = paths[index]
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            format!("{name}: {e}")
        })
        .collect();

    BatchTrimResult {
        saved: tally.saved,
        renamed: tally.renamed,
        errors,
        cancelled: queue.skipped.into_inner(),
        total,
        out_dir: out_dir.to_path_buf(),
        fatal_error: None,
    }
}

/// ワーカー間で共有する未着手ファイルの払い出し口
struct JobQueue<'a> {
    /// 入力ファイルパス列
    paths: &'a [PathBuf],
    /// キャンセルフラグ
    cancel: &'a AtomicBool,
    /// 次に払い出すインデックス
    next: AtomicUsize,
    /// キャンセルにより未着手のファイルを残したか
    skipped: AtomicBool,
}

impl<'a> JobQueue<'a> {
    /// 次に処理するファイルを `(インデックス, パス)` で払い出す
    ///
    /// 全件払い出し済み、またはキャンセル済みの場合は `None` を返す
    fn claim(&self) -> Option<(usize, &'a Path)> {
        let index = self.next.fetch_add(1, Ordering::Relaxed);
        let path = self.paths.get(index)?;
        if self.cancel.load(Ordering::Relaxed) {
            self.skipped.store(true, Ordering::Relaxed);
            return None;
        }
        Some((index, path))
    }
}

/// 呼び出し元スレッドで集計する処理結果
#[derive(Default)]
struct Tally {
    /// 正常に保存したファイル数
    saved: usize,
    /// 連番サフィックスで衝突回避した保存数
    renamed: usize,
    /// 失敗したファイルの `(インデックス, 理由)`
    errors: Vec<(usize, String)>,
}

impl Tally {
    /// 1 ファイル分の結果を加算する
    fn record(&mut self, index: usize, result: Result<bool, BatchFileError>) {
        match result {
            Ok(file_renamed) => {
                self.saved += 1;
                if file_renamed {
                    self.renamed += 1;
                }
            }
            Err(e) => self.errors.push((index, e.to_string())),
        }
    }

    /// 処理済みファイル数 (成功 + 失敗)
    fn processed(&self) -> usize {
        self.saved + self.errors.len()
    }
}

/// 1 ファイル分の処理を実行し、panic をエラーに変換する
///
/// # Arguments
/// * `process` - 1 ファイル分の処理
/// * `path` - 処理するファイル
///
/// # Returns
/// * `Result<bool, BatchFileError>` - `process` の結果。panic した場合は `BatchFileError::Panic`
fn process_guarded(
    process: &impl Fn(&Path) -> Result<bool, BatchFileError>,
    path: &Path,
) -> Result<bool, BatchFileError> {
    // 共有状態は払い出し口のアトミック変数のみで、panic 後も不整合にならない
    catch_panic(|| process(path)).unwrap_or_else(|msg| Err(BatchFileError::Panic(msg)))
}

/// 1 ファイル分の読み込み・トリム・保存
///
/// ヘッダから見積もったピークメモリを `budget` から確保し、保存し終えるまで保持する
fn process_one(
    path: &Path,
    out_dir: &Path,
    params: BatchTrimParams,
    budget: &MemoryBudget,
) -> Result<bool, BatchFileError> {
    let decoded_bytes = probe_decoded_bytes(path)?;
    let _permit = budget.acquire(decoded_bytes.saturating_mul(BATCH_PEAK_MEMORY_FACTOR));

    let LoadedImage { image, icc_profile } = open_image_with_icc(path)?;
    let trimmed = trim(&image, params.tolerance, params.padding, params.bg_override);
    // エンコード中のピークを見積もり内に収めるため、元画像は切り出し後すぐに解放する
    drop(image);
    let (_, file_renamed) = save_trimmed_output_with(
        &trimmed.image,
        icc_profile.as_deref(),
        path,
        out_dir,
        params.format,
        params.save_options,
    )?;
    Ok(file_renamed)
}

// ======================================================================
// メモリ予算
// ======================================================================
/// 並列処理するファイル間で共有するメモリ予算
///
/// 確保できるまで待つ計数セマフォとして働く。各ワーカーが保持する許可は高々 1 つのため、
/// 1 件の要求を予算全体に切り詰めておけばデッドロックしない
struct MemoryBudget {
    /// 予算全体のバイト数
    capacity: u64,
    /// 未確保のバイト数
    available: Mutex<u64>,
    /// 許可の返却を待つワーカーへの通知
    released: Condvar,
}

impl MemoryBudget {
    /// 予算全体が未確保の状態で生成する
    fn new(capacity: u64) -> Self {
        Self {
            capacity,
            available: Mutex::new(capacity),
            released: Condvar::new(),
        }
    }

    /// `bytes` を確保できるまで待ち、確保した許可を返す
    ///
    /// 予算全体を超える要求は予算全体に切り詰め、他の許可がすべて返却された時点で確保する
    ///
    /// # Arguments
    /// * `bytes` - 確保するバイト数
    ///
    /// # Returns
    /// * `MemoryPermit` - 破棄時 (panic による巻き戻しを含む) に確保分を返却する許可
    fn acquire(&self, bytes: u64) -> MemoryPermit<'_> {
        let bytes = bytes.min(self.capacity);
        // 保護対象は数値 1 つで、panic 後も不整合にならない
        let mut available = self
            .released
            .wait_while(
                self.available
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner),
                |available| *available < bytes,
            )
            .unwrap_or_else(PoisonError::into_inner);
        *available -= bytes;
        MemoryPermit {
            budget: self,
            bytes,
        }
    }
}

/// `MemoryBudget` から確保したメモリの許可
struct MemoryPermit<'a> {
    /// 返却先の予算
    budget: &'a MemoryBudget,
    /// 確保したバイト数
    bytes: u64,
}

impl Drop for MemoryPermit<'_> {
    fn drop(&mut self) {
        *self
            .budget
            .available
            .lock()
            .unwrap_or_else(PoisonError::into_inner) += self.bytes;
        self.budget.released.notify_all();
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::AtomicBool;

    use super::*;
    use crate::test_helpers::bordered_rgba;

    // ======================================================================
    // ヘルパー
    // ======================================================================
    /// テスト用の一時ディレクトリ。破棄時に削除される
    struct TempDir(tempfile::TempDir);

    impl TempDir {
        fn new() -> Self {
            Self(tempfile::tempdir().expect("temp dir"))
        }

        fn path(&self, name: &str) -> PathBuf {
            self.0.path().join(name)
        }
    }

    fn write_bordered_png(path: &Path) {
        let img = bordered_rgba(12, 8, 2, [255, 255, 255, 255], [0, 128, 0, 255]);
        img.save(path).expect("write png");
    }

    fn default_params() -> BatchTrimParams {
        BatchTrimParams {
            tolerance: 0,
            padding: 0,
            bg_override: None,
            format: OutputFormat::Original,
            save_options: SaveOptions::default(),
        }
    }

    // ======================================================================
    // run_batch_trim
    // ======================================================================
    /// 複数ファイルが既定の出力名で保存されること
    #[test]
    fn batch_saves_all_files() {
        let ws = TempDir::new();
        let out = ws.path("out");
        fs::create_dir_all(&out).expect("out dir");
        let inputs = [ws.path("a.png"), ws.path("b.png")];
        for p in &inputs {
            write_bordered_png(p);
        }

        let cancel = AtomicBool::new(false);
        let result = run_batch_trim(
            &inputs,
            &out,
            BatchTrimParams {
                tolerance: 0,
                padding: 0,
                bg_override: None,
                format: OutputFormat::Original,
                save_options: SaveOptions::default(),
            },
            &cancel,
            |_, _| {},
        );

        assert_eq!(result.saved, 2);
        assert_eq!(result.errors.len(), 0);
        assert!(!result.cancelled);
        assert!(out.join("a_trimmed.png").is_file());
        assert!(out.join("b_trimmed.png").is_file());
    }

    /// 元画像の ICC プロファイルが一括出力へ引き継がれること
    #[test]
    fn batch_preserves_icc_profile() {
        let ws = TempDir::new();
        let out = ws.path("out");
        fs::create_dir_all(&out).expect("out dir");
        let input = ws.path("photo.png");
        let mut icc = vec![0u8; 132];
        icc[16..20].copy_from_slice(b"RGB ");
        let img = bordered_rgba(12, 8, 2, [255, 255, 255, 255], [0, 128, 0, 255]);
        crate::image_output::save_image(&img, Some(&icc), &input).expect("write png");

        let cancel = AtomicBool::new(false);
        let result = run_batch_trim(
            &[input],
            &out,
            BatchTrimParams {
                tolerance: 0,
                padding: 0,
                bg_override: None,
                format: OutputFormat::Original,
                save_options: SaveOptions::default(),
            },
            &cancel,
            |_, _| {},
        );

        assert_eq!(result.saved, 1);
        let loaded = crate::image_load::open_image_with_icc(&out.join("photo_trimmed.png"))
            .expect("出力の再読み込み");
        assert_eq!(loaded.icc_profile, Some(icc));
    }

    /// 出力形式を指定すると元の形式に関係なくその形式で保存すること
    #[test]
    fn batch_saves_in_specified_format() {
        let ws = TempDir::new();
        let out = ws.path("out");
        fs::create_dir_all(&out).expect("out dir");
        let input = ws.path("a.png");
        write_bordered_png(&input);

        let cancel = AtomicBool::new(false);
        let result = run_batch_trim(
            &[input],
            &out,
            BatchTrimParams {
                tolerance: 0,
                padding: 0,
                bg_override: None,
                format: OutputFormat::Jpeg,
                save_options: SaveOptions::default(),
            },
            &cancel,
            |_, _| {},
        );

        assert_eq!(result.saved, 1);
        let saved = out.join("a_trimmed.jpg");
        let bytes = fs::read(&saved).expect("read");
        // 拡張子だけでなく中身も JPEG であること
        assert_eq!(
            image::guess_format(&bytes).expect("推定"),
            image::ImageFormat::Jpeg
        );
    }

    /// 既存出力があるとき連番サフィックスで回避すること
    #[test]
    fn batch_avoids_overwriting_existing_output() {
        let ws = TempDir::new();
        let out = ws.path("out");
        fs::create_dir_all(&out).expect("out dir");
        let input = ws.path("photo.png");
        write_bordered_png(&input);
        fs::write(out.join("photo_trimmed.png"), b"existing").expect("placeholder");

        let cancel = AtomicBool::new(false);
        let result = run_batch_trim(
            &[input],
            &out,
            BatchTrimParams {
                tolerance: 0,
                padding: 0,
                bg_override: None,
                format: OutputFormat::Original,
                save_options: SaveOptions::default(),
            },
            &cancel,
            |_, _| {},
        );

        assert_eq!(result.saved, 1);
        assert_eq!(result.renamed, 1);
        assert!(out.join("photo_trimmed.png").exists());
        assert!(out.join("photo_trimmed_2.png").is_file());
        let existing = fs::read(out.join("photo_trimmed.png")).expect("read");
        assert_eq!(existing, b"existing");
    }

    /// キャンセル後は未処理ファイルを処理しないこと
    ///
    /// 完了時点で着手済みのファイルを確定させるため、並列数 1 で検証する
    #[test]
    fn batch_honours_cancel_before_next_file() {
        let ws = TempDir::new();
        let out = ws.path("out");
        fs::create_dir_all(&out).expect("out dir");
        let inputs = [ws.path("a.png"), ws.path("b.png"), ws.path("c.png")];
        for p in &inputs {
            write_bordered_png(p);
        }

        let cancel = AtomicBool::new(false);
        let result = run_batch_trim_with_workers(
            &inputs,
            &out,
            default_params(),
            &cancel,
            |processed, _| {
                if processed >= 1 {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
            1,
        );

        assert!(result.cancelled);
        assert_eq!(result.saved, 1);
        assert!(!out.join("b_trimmed.png").exists());
        assert!(!out.join("c_trimmed.png").exists());
    }

    /// 存在しない入力はエラーとして集計し、他ファイルは継続すること
    #[test]
    fn batch_collects_per_file_errors_and_continues() {
        let ws = TempDir::new();
        let out = ws.path("out");
        fs::create_dir_all(&out).expect("out dir");
        let good = ws.path("good.png");
        write_bordered_png(&good);
        let bad = ws.path("missing.png");

        let cancel = AtomicBool::new(false);
        let result = run_batch_trim(
            &[good.clone(), bad],
            &out,
            BatchTrimParams {
                tolerance: 0,
                padding: 0,
                bg_override: None,
                format: OutputFormat::Original,
                save_options: SaveOptions::default(),
            },
            &cancel,
            |_, _| {},
        );

        assert_eq!(result.saved, 1);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("missing.png"));
        assert!(out.join("good_trimmed.png").is_file());
    }

    /// 並列処理でも全ファイルを保存し、進捗が 1 件ずつ単調に増えること
    #[test]
    fn batch_parallel_saves_all_files_and_reports_progress() {
        let ws = TempDir::new();
        let out = ws.path("out");
        fs::create_dir_all(&out).expect("out dir");
        let inputs: Vec<PathBuf> = (0..12).map(|i| ws.path(&format!("img{i}.png"))).collect();
        for p in &inputs {
            write_bordered_png(p);
        }

        let cancel = AtomicBool::new(false);
        let mut reported = Vec::new();
        let result = run_batch_trim_with_workers(
            &inputs,
            &out,
            default_params(),
            &cancel,
            |processed, total| reported.push((processed, total)),
            4,
        );

        assert_eq!(result.saved, inputs.len());
        assert!(result.errors.is_empty());
        assert!(!result.cancelled);
        for i in 0..inputs.len() {
            assert!(out.join(format!("img{i}_trimmed.png")).is_file());
        }
        // 進捗は呼び出し元スレッドで 1..=total の順に通知される
        let expected: Vec<_> = (1..=inputs.len()).map(|n| (n, inputs.len())).collect();
        assert_eq!(reported, expected);
    }

    /// 並列処理でも同名出力は連番で衝突回避し、上書きしないこと
    #[test]
    fn batch_parallel_avoids_collisions_between_workers() {
        let ws = TempDir::new();
        let out = ws.path("out");
        fs::create_dir_all(&out).expect("out dir");
        // 別フォルダの同名ファイルは同じ出力名を奪い合う
        let inputs: Vec<PathBuf> = (0..4)
            .map(|i| {
                let dir = ws.path(&format!("d{i}"));
                fs::create_dir_all(&dir).expect("in dir");
                let p = dir.join("same.png");
                write_bordered_png(&p);
                p
            })
            .collect();

        let cancel = AtomicBool::new(false);
        let result =
            run_batch_trim_with_workers(&inputs, &out, default_params(), &cancel, |_, _| {}, 4);

        assert_eq!(result.saved, 4);
        assert_eq!(result.renamed, 3);
        assert!(out.join("same_trimmed.png").is_file());
        for n in 2..=4 {
            assert!(out.join(format!("same_trimmed_{n}.png")).is_file());
        }
    }

    /// 開始前にキャンセル済みなら 1 件も処理せず、キャンセル扱いになること
    #[test]
    fn batch_parallel_cancelled_before_start_processes_nothing() {
        let ws = TempDir::new();
        let out = ws.path("out");
        fs::create_dir_all(&out).expect("out dir");
        let inputs = [ws.path("a.png"), ws.path("b.png"), ws.path("c.png")];
        for p in &inputs {
            write_bordered_png(p);
        }

        let cancel = AtomicBool::new(true);
        let result =
            run_batch_trim_with_workers(&inputs, &out, default_params(), &cancel, |_, _| {}, 4);

        assert!(result.cancelled);
        assert_eq!(result.saved, 0);
        assert_eq!(fs::read_dir(&out).expect("read out").count(), 0);
    }

    /// 並列処理でもエラーは完了順ではなく入力順に並ぶこと
    #[test]
    fn batch_parallel_orders_errors_by_input() {
        let ws = TempDir::new();
        let out = ws.path("out");
        fs::create_dir_all(&out).expect("out dir");
        let good = ws.path("good.png");
        write_bordered_png(&good);
        let inputs = [
            ws.path("missing1.png"),
            good,
            ws.path("missing2.png"),
            ws.path("missing3.png"),
        ];

        let cancel = AtomicBool::new(false);
        let result =
            run_batch_trim_with_workers(&inputs, &out, default_params(), &cancel, |_, _| {}, 4);

        assert_eq!(result.saved, 1);
        let names: Vec<_> = result
            .errors
            .iter()
            .map(|e| e.split(':').next().unwrap_or_default())
            .collect();
        assert_eq!(names, ["missing1.png", "missing2.png", "missing3.png"]);
    }

    /// 1 ファイルの処理が panic しても、そのファイルをエラーとして数えて残りを続けること
    #[test]
    fn batch_counts_panicked_file_as_error_and_continues() {
        let inputs: Vec<PathBuf> = ["a.png", "boom.png", "c.png", "d.png"]
            .iter()
            .map(PathBuf::from)
            .collect();
        let process = |path: &Path| {
            assert!(path != Path::new("boom.png"), "テスト用の panic");
            Ok(false)
        };

        // 逐次・並列のどちらでも panic を呼び出し元へ伝えない
        for workers in [1, 4] {
            let cancel = AtomicBool::new(false);
            let mut last_progress = None;
            let result = run_batch_with(
                &inputs,
                Path::new("out"),
                &cancel,
                |processed, total| last_progress = Some((processed, total)),
                workers,
                process,
            );

            assert_eq!(result.saved, 3, "workers={workers}");
            assert_eq!(result.errors.len(), 1, "workers={workers}");
            assert!(result.errors[0].starts_with("boom.png: "));
            assert!(result.errors[0].contains("テスト用の panic"));
            assert!(!result.cancelled);
            // panic したファイルも処理済みとして進捗に含める
            assert_eq!(last_progress, Some((4, 4)));
        }
    }

    /// 空の入力では何もせず正常終了すること
    #[test]
    fn batch_empty_input_finishes_immediately() {
        let ws = TempDir::new();
        let cancel = AtomicBool::new(false);
        let result = run_batch_trim(&[], &ws.path("out"), default_params(), &cancel, |_, _| {});

        assert_eq!(result.saved, 0);
        assert_eq!(result.total, 0);
        assert!(!result.cancelled);
    }

    // ======================================================================
    // MemoryBudget
    // ======================================================================
    /// 同時に保持される許可の合計が予算を超えないこと
    #[test]
    fn memory_budget_limits_concurrent_permits() {
        let budget = MemoryBudget::new(100);
        let active = AtomicUsize::new(0);
        let max_active = AtomicUsize::new(0);

        thread::scope(|s| {
            for _ in 0..4 {
                s.spawn(|| {
                    for _ in 0..5 {
                        let _permit = budget.acquire(60);
                        let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                        max_active.fetch_max(now, Ordering::SeqCst);
                        // 他スレッドと重なる機会を作る
                        thread::sleep(std::time::Duration::from_millis(1));
                        active.fetch_sub(1, Ordering::SeqCst);
                    }
                });
            }
        });

        // 60 バイトの許可は 100 バイトの予算に 1 つしか入らない
        assert_eq!(max_active.into_inner(), 1);
        assert_eq!(*budget.available.lock().unwrap(), 100);
    }

    /// 予算全体を超える要求も予算全体に切り詰めて確保できること
    #[test]
    fn memory_budget_clamps_oversized_request() {
        let budget = MemoryBudget::new(100);
        let permit = budget.acquire(u64::MAX);

        assert_eq!(permit.bytes, 100);
        assert_eq!(*budget.available.lock().unwrap(), 0);
        // 破棄すると全額が返却される
        drop(permit);
        assert_eq!(*budget.available.lock().unwrap(), 100);
    }

    /// 許可を保持したまま panic しても確保分が返却されること
    #[test]
    fn memory_budget_releases_permit_on_panic() {
        let budget = MemoryBudget::new(100);
        let result = catch_panic(|| {
            let _permit = budget.acquire(80);
            panic!("テスト用の panic");
        });

        assert!(result.is_err());
        assert_eq!(*budget.available.lock().unwrap(), 100);
    }

    /// `BatchFileError` が内包する読み込みエラーを `source()` で返すこと
    #[test]
    fn batch_file_error_source_chains_load_error() {
        use std::error::Error;
        use std::io;

        use crate::image_load::LoadError;

        let inner = io::Error::new(io::ErrorKind::NotFound, "missing");
        let err = BatchFileError::Load(LoadError::Io(inner));
        assert!(Error::source(&err).is_some());
    }

    /// `summary()` がエラー件数を 1 行に要約すること
    #[test]
    fn summary_omits_full_error_list() {
        let result = BatchTrimResult {
            saved: 1,
            renamed: 0,
            errors: vec!["a.png: err1".into(), "b.png: err2".into()],
            cancelled: false,
            total: 3,
            out_dir: PathBuf::from("/out"),
            fatal_error: None,
        };
        let msg = result.summary();
        assert!(msg.contains("2 件エラー"));
        assert!(msg.contains("先頭: a.png: err1"));
        assert!(!msg.contains("b.png: err2"));
    }
}
