//! 一括トリミングのコア処理

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::image_load::{LoadError, open_image};
use crate::image_output::save_batch_output;
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
        }
    }
}

impl Error for BatchFileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Load(e) => Some(e),
            Self::Save(e) => Some(e),
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
    /// 出力フォルダ作成失敗など、処理開始前の致命的エラー
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

    /// ステータスバー向けの要約メッセージ (1 行)
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
/// 複数画像を順次トリミングして `out_dir` へ保存する
///
/// ファイルごとにキャンセルフラグを確認するため、並列処理は行わない
/// 出力パスが既存ファイルと衝突する場合は連番サフィックスで回避する
///
/// # Arguments
/// * `paths`        - 入力ファイルパス列
/// * `out_dir`      - 出力先ディレクトリ (呼び出し側で作成済みであること)
/// * `params`       - トリム設定
/// * `cancel`       - `true` になると次のファイル処理前に打ち切る
/// * `on_progress`  - 各ファイル完了後に `(処理済み数, 総数)` で呼ばれる
#[must_use]
pub fn run_batch_trim(
    paths: &[PathBuf],
    out_dir: &Path,
    params: BatchTrimParams,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(usize, usize),
) -> BatchTrimResult {
    let total = paths.len();
    let mut saved = 0usize;
    let mut renamed = 0usize;
    let mut errors = Vec::new();
    let mut cancelled = false;

    for path in paths {
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            break;
        }

        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();

        match process_one(path, out_dir, params) {
            Ok(file_renamed) => {
                saved += 1;
                if file_renamed {
                    renamed += 1;
                }
            }
            Err(e) => errors.push(format!("{name}: {e}")),
        }

        on_progress(saved + errors.len(), total);
    }

    BatchTrimResult {
        saved,
        renamed,
        errors,
        cancelled,
        total,
        out_dir: out_dir.to_path_buf(),
        fatal_error: None,
    }
}

/// 1 ファイル分の読み込み・トリム・保存
fn process_one(
    path: &Path,
    out_dir: &Path,
    params: BatchTrimParams,
) -> Result<bool, BatchFileError> {
    let img = open_image(path)?;
    let trimmed = trim(&img, params.tolerance, params.padding, params.bg_override);
    let (_, file_renamed) = save_batch_output(&trimmed.image, path, out_dir)?;
    Ok(file_renamed)
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::AtomicBool;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    use crate::test_helpers::bordered_rgba;

    // ======================================================================
    // ヘルパー
    // ======================================================================
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(prefix: &str) -> Self {
            let ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let dir = std::env::temp_dir().join(format!("edge_cut_batch_{prefix}_{ts}"));
            fs::create_dir_all(&dir).expect("temp dir");
            Self(dir)
        }

        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_bordered_png(path: &Path) {
        let img = bordered_rgba(12, 8, 2, [255, 255, 255, 255], [0, 128, 0, 255]);
        img.save(path).expect("write png");
    }

    // ======================================================================
    // run_batch_trim
    // ======================================================================
    /// 複数ファイルが既定の出力名で保存されることを確認する
    #[test]
    fn batch_saves_all_files() {
        let ws = TempDir::new("save_all");
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

    /// 既存出力があるとき連番サフィックスで回避することを確認する
    #[test]
    fn batch_avoids_overwriting_existing_output() {
        let ws = TempDir::new("no_overwrite");
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

    /// キャンセル後は未処理ファイルを処理しないことを確認する
    #[test]
    fn batch_honours_cancel_before_next_file() {
        let ws = TempDir::new("cancel");
        let out = ws.path("out");
        fs::create_dir_all(&out).expect("out dir");
        let inputs = [ws.path("a.png"), ws.path("b.png"), ws.path("c.png")];
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
            },
            &cancel,
            |processed, _| {
                if processed >= 1 {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
        );

        assert!(result.cancelled);
        assert_eq!(result.saved, 1);
        assert!(!out.join("b_trimmed.png").exists());
        assert!(!out.join("c_trimmed.png").exists());
    }

    /// 存在しない入力はエラーとして集計し、他ファイルは継続することを確認する
    #[test]
    fn batch_collects_per_file_errors_and_continues() {
        let ws = TempDir::new("errors");
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
            },
            &cancel,
            |_, _| {},
        );

        assert_eq!(result.saved, 1);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("missing.png"));
        assert!(out.join("good_trimmed.png").is_file());
    }

    /// `BatchFileError` が内包する読み込みエラーを `source()` で返すことを確認する
    #[test]
    fn batch_file_error_source_chains_load_error() {
        use std::error::Error;
        use std::io;

        use crate::image_load::LoadError;

        let inner = io::Error::new(io::ErrorKind::NotFound, "missing");
        let err = BatchFileError::Load(LoadError::Io(inner));
        assert!(Error::source(&err).is_some());
    }

    /// `summary()` がエラー件数を 1 行に要約することを確認する
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
