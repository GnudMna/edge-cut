//! 一括トリミングの実行
//!
//! ファイル単位の並列処理・キャンセル・集計を担う。

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;

use super::error::BatchFileError;
use super::memory_budget::MemoryBudget;
use super::result::{BatchTrimParams, BatchTrimResult};
use crate::consts::{BATCH_MAX_CONCURRENCY, BATCH_MEMORY_BUDGET_BYTES, BATCH_PEAK_MEMORY_FACTOR};
use crate::image_load::{LoadedImage, open_image_with_icc_reserving};
use crate::image_output::save_trimmed_output_with;
use crate::panic_message::catch_panic;
use crate::trim::{apply_trim, detect_trim_with_options};

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
pub(super) fn run_batch_trim_with_workers(
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
pub(super) fn run_batch_with(
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
/// 余白の追加で出力が元画像より大きくなる場合は、出力サイズが分かった時点で確保量を増やす
fn process_one(
    path: &Path,
    out_dir: &Path,
    params: BatchTrimParams,
    budget: &MemoryBudget,
) -> Result<bool, BatchFileError> {
    // ヘッダの検証後、デコード前に見積もり分を確保する
    let (
        LoadedImage {
            image,
            icc_profile,
            dpi,
            source_format,
            ..
        },
        mut permit,
    ) = open_image_with_icc_reserving(path, |decoded_bytes| {
        budget.acquire(decoded_bytes.saturating_mul(BATCH_PEAK_MEMORY_FACTOR))
    })?;
    let bounds = detect_trim_with_options(&image, &params.trim);
    let bytes_per_pixel = u64::from(image.color().bytes_per_pixel());
    let decoded_bytes = u64::from(image.width()) * u64::from(image.height()) * bytes_per_pixel;
    let output_bytes =
        u64::from(bounds.info.trim_w) * u64::from(bounds.info.trim_h) * bytes_per_pixel;
    // 出力キャンバスはデコード後の画像より大きくなりうるため、切り出し前に不足分を確保する
    permit.grow_to(estimate_peak_bytes(decoded_bytes, output_bytes));
    let trimmed = apply_trim(&image, &bounds.info);
    // エンコード中のピークを見積もり内に収めるため、元画像は切り出し後すぐに解放する
    drop(image);
    let (_, file_renamed) = save_trimmed_output_with(
        &trimmed,
        icc_profile.as_deref(),
        dpi,
        path,
        out_dir,
        params.format,
        source_format,
        params.save_options,
    )?;
    Ok(file_renamed)
}

/// 1 ファイルのピークメモリを見積もる
///
/// 次の最大値とする
/// * 保存時: 出力画像・エンコード向けの色変換・エンコード結果で出力の `BATCH_PEAK_MEMORY_FACTOR` 倍
/// * 切り出し時: 元画像・切り出した画像 (元画像以下)・出力キャンバスの合計
///
/// # Arguments
/// * `decoded_bytes` - デコード後の画像のバイト数
/// * `output_bytes` - 余白の追加を含む出力画像のバイト数
///
/// # Returns
/// * `u64` - 見積もったピークメモリ (バイト)
pub(super) fn estimate_peak_bytes(decoded_bytes: u64, output_bytes: u64) -> u64 {
    let save = output_bytes.saturating_mul(BATCH_PEAK_MEMORY_FACTOR);
    let crop = decoded_bytes.saturating_mul(2).saturating_add(output_bytes);
    decoded_bytes
        .saturating_mul(BATCH_PEAK_MEMORY_FACTOR)
        .max(save)
        .max(crop)
}
