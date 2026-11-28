use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use super::runner::{
    estimate_peak_bytes, max_output_bytes, run_batch_trim_with_workers, run_batch_with,
};
use super::*;
use crate::consts::{MAX_IMAGE_HEIGHT, MAX_IMAGE_PIXELS, MAX_IMAGE_WIDTH, PADDING_MAX};
use crate::image_output::{OutputFormat, SaveOptions};
use crate::test_helpers::bordered_rgba;
use crate::trim::{AspectFit, AspectRatio, TrimOptions};

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
        trim: TrimOptions::default(),
        format: OutputFormat::Original,
        save_options: SaveOptions::default(),
    }
}

// ======================================================================
// メモリ見積もり
// ======================================================================
/// 出力が元画像以下のときは、デコード後のバイト数の 3 倍の見積もりになること
#[test]
fn peak_estimate_for_smaller_output_is_three_times_decoded() {
    assert_eq!(estimate_peak_bytes(1000, 400), 3000);
    assert_eq!(estimate_peak_bytes(1000, 1000), 3000);
}

/// 余白の追加で出力が大きくなる場合は、出力側の分を見積もりに含むこと
///
/// 100x100 の RGBA8 画像が 64MP の出力に広がる例
#[test]
fn peak_estimate_includes_extended_output() {
    let decoded = 100 * 100 * 4;
    let output = 64_000_000 * 4;
    let peak = estimate_peak_bytes(decoded, output);

    assert_eq!(peak, output * 3);
    assert!(peak > decoded * 3);
}

/// 出力サイズの上限が四辺の余白の最大分だけ大きく、向きの補正で入れ替わる場合も含むこと
#[test]
fn max_output_bytes_includes_padding_and_rotation() {
    let pad = u64::from(PADDING_MAX) * 2;
    // 100x50 の RGBA8
    assert_eq!(
        max_output_bytes(100, 50, 100 * 50 * 4, None),
        (100 + pad) * (50 + pad) * 4
    );
    // 90 度回転で幅と高さが入れ替わっても、大きい方の見積もりを使う
    assert_eq!(
        max_output_bytes(50, 100, 100 * 50 * 4, None),
        max_output_bytes(100, 50, 100 * 50 * 4, None)
    );
}

/// 余白を足してアスペクト比を合わせる場合の上限が、その結果を下回らないこと
#[test]
fn max_output_bytes_covers_aspect_expand() {
    let ratio = AspectRatio {
        w: 1,
        h: 4,
        fit: AspectFit::Expand,
    };
    // 幅 100+200 に対して高さは 4 倍の 1200 まで伸びる (元の高さ 50+200 より大きい)
    let bytes = max_output_bytes(100, 50, 100 * 50 * 4, Some(&ratio));
    assert!(bytes >= 300 * 1200 * 4);
}

/// 出力サイズの上限が `MAX_IMAGE_PIXELS` を超えないこと
#[test]
fn max_output_bytes_is_capped_by_image_limits() {
    let bytes = max_output_bytes(MAX_IMAGE_WIDTH, MAX_IMAGE_HEIGHT, 1, None);
    assert!(bytes <= MAX_IMAGE_PIXELS);
}

/// 見積もりの計算がオーバーフローせず飽和すること
#[test]
fn peak_estimate_saturates_on_overflow() {
    assert_eq!(estimate_peak_bytes(u64::MAX, u64::MAX), u64::MAX);
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
            trim: TrimOptions::default(),
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
    crate::image_output::save_image(&img, Some(&icc), None, &input).expect("write png");

    let cancel = AtomicBool::new(false);
    let result = run_batch_trim(
        &[input],
        &out,
        BatchTrimParams {
            trim: TrimOptions::default(),
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

/// 元画像の解像度が一括出力へ引き継がれること
#[test]
fn batch_preserves_dpi() {
    let ws = TempDir::new();
    let out = ws.path("out");
    fs::create_dir_all(&out).expect("out dir");
    let input = ws.path("scan.png");
    let dpi = crate::dpi::Dpi::new(300.0, 300.0).unwrap();
    let img = bordered_rgba(12, 8, 2, [255, 255, 255, 255], [0, 128, 0, 255]);
    crate::image_output::save_image(&img, None, Some(dpi), &input).expect("write png");

    let cancel = AtomicBool::new(false);
    let result = run_batch_trim(
        &[input],
        &out,
        BatchTrimParams {
            trim: TrimOptions::default(),
            format: OutputFormat::Original,
            save_options: SaveOptions::default(),
        },
        &cancel,
        |_, _| {},
    );

    assert_eq!(result.saved, 1);
    let loaded = crate::image_load::open_image_with_icc(&out.join("scan_trimmed.png"))
        .expect("出力の再読み込み");
    assert_eq!(loaded.dpi, Some(dpi));
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
            trim: TrimOptions::default(),
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
            trim: TrimOptions::default(),
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

/// 上書き設定が有効なときは既存出力を連番で避けず上書きすること
#[test]
fn batch_overwrites_existing_output_when_enabled() {
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
            save_options: SaveOptions {
                overwrite: true,
                ..SaveOptions::default()
            },
            ..default_params()
        },
        &cancel,
        |_, _| {},
    );

    assert_eq!(result.saved, 1);
    assert_eq!(result.renamed, 0);
    assert!(!out.join("photo_trimmed_2.png").exists());
    assert_ne!(
        fs::read(out.join("photo_trimmed.png")).expect("read"),
        b"existing"
    );
}

/// 上書き設定が有効でも、同じ実行内の同名出力は連番で避けて互いに上書きしないこと
#[test]
fn batch_overwrite_keeps_outputs_of_same_run_apart() {
    let ws = TempDir::new();
    let out = ws.path("out");
    fs::create_dir_all(&out).expect("out dir");
    let inputs: Vec<PathBuf> = (0..3)
        .map(|i| {
            let dir = ws.path(&format!("d{i}"));
            fs::create_dir_all(&dir).expect("in dir");
            let p = dir.join("same.png");
            write_bordered_png(&p);
            p
        })
        .collect();

    let params = BatchTrimParams {
        save_options: SaveOptions {
            overwrite: true,
            ..SaveOptions::default()
        },
        ..default_params()
    };
    let cancel = AtomicBool::new(false);
    let result = run_batch_trim_with_workers(&inputs, &out, params, &cancel, |_, _| {}, 3);

    assert_eq!(result.saved, 3);
    assert_eq!(result.renamed, 2);
    assert!(out.join("same_trimmed.png").is_file());
    assert!(out.join("same_trimmed_2.png").is_file());
    assert!(out.join("same_trimmed_3.png").is_file());
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
            trim: TrimOptions::default(),
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

    assert_eq!(result.errors.len(), 0, "{:?}", result.errors);
    assert_eq!(result.saved, inputs.len());
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
    // エラーは入力パスで始まるため、失敗した入力の順と前方一致で照合する
    let missing = [&inputs[0], &inputs[2], &inputs[3]];
    assert_eq!(result.errors.len(), missing.len());
    for (error, input) in result.errors.iter().zip(missing) {
        assert!(
            error.starts_with(&format!("{}: ", input.display())),
            "{error}"
        );
    }
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

/// 別フォルダの同名ファイルが失敗した場合、エラーを入力パスで区別できること
#[test]
fn batch_errors_show_input_paths_for_same_named_files() {
    let inputs = vec![Path::new("a").join("x.png"), Path::new("b").join("x.png")];
    let cancel = AtomicBool::new(false);
    let result = run_batch_with(
        &inputs,
        Path::new("out"),
        &cancel,
        |_, _| {},
        1,
        |_| Err(BatchFileError::Panic("失敗".into())),
    );

    // 入力順のまま、フォルダを含むパスで始まる
    assert_eq!(result.errors.len(), 2);
    for (error, input) in result.errors.iter().zip(&inputs) {
        assert!(
            error.starts_with(&format!("{}: ", input.display())),
            "{error}"
        );
    }
    assert_ne!(result.errors[0], result.errors[1]);
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
