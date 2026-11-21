//! `EdgeCut` コア処理の統合テスト
//!
//! ファイル I/O (`open_image` / save) とトリミング (`trim`) を組み合わせた
//! エンドツーエンドの動作を検証する

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use edge_cut::trim::{
    AspectFit, AspectRatio, Margins, TrimInfo, TrimOptions, TrimSides, sample_pixel,
    trim_with_options,
};
use edge_cut::{OutputFormat, open_image, save_image, trim, trimmed_output_path};
use image::DynamicImage;

use common::images::bordered_rgba;

// ======================================================================
// テスト用ヘルパー
// ======================================================================
/// 統合テスト用の一時ディレクトリ。破棄時に削除される
struct TempWorkspace {
    dir: tempfile::TempDir,
}

impl TempWorkspace {
    /// 一意な一時ディレクトリを作成する
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("一時ディレクトリの作成に失敗");
        Self { dir }
    }

    /// 一時ディレクトリ内のファイルパスを返す
    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
}

/// 縁付き画像を PNG として書き出す
fn write_bordered_png(path: &Path, w: u32, h: u32, border: u32, bg: [u8; 4], fg: [u8; 4]) {
    let img = bordered_rgba(w, h, border, bg, fg);
    img.save(path).expect("テスト用 PNG の書き出しに失敗");
}

/// 画像を開き、トリミングして保存し、再読み込み後の寸法を検証する
fn trim_save_and_reload(
    input: &Path,
    output: &Path,
    tolerance: u32,
    padding: u32,
    bg_override: Option<[u8; 4]>,
) -> (TrimInfo, DynamicImage) {
    let img = open_image(input).expect("入力画像の読み込みに失敗");
    let trimmed = trim(&img, tolerance, padding, bg_override);
    save_image(&trimmed.image, None, None, output).expect("トリミング結果の保存に失敗");

    let reloaded = open_image(output).expect("出力画像の再読み込みに失敗");
    assert_eq!(reloaded.width(), trimmed.info.trim_w);
    assert_eq!(reloaded.height(), trimmed.info.trim_h);

    (trimmed.info, reloaded)
}

// ======================================================================
// 統合テスト
// ======================================================================
/// PNG を経由した読み込み・トリミング・保存・再読み込みを一連で行えるようにすること
#[test]
fn trim_pipeline_round_trips_through_png_file() {
    let ws = TempWorkspace::new();
    let input = ws.path("input.png");
    let output = ws.path("output.png");

    write_bordered_png(&input, 10, 10, 2, [255, 255, 255, 255], [255, 0, 0, 255]);

    let (info, reloaded) = trim_save_and_reload(&input, &output, 0, 0, None);

    assert_eq!(info.orig_w, 10);
    assert_eq!(info.orig_h, 10);
    assert_eq!(info.trim_w, 6);
    assert_eq!(info.trim_h, 6);
    assert_eq!(sample_pixel(&reloaded, 0, 0), Some([255, 0, 0, 255]));
}

/// padding 付きトリミング結果がファイル保存後も維持されること
#[test]
fn trim_with_padding_persists_after_save() {
    let ws = TempWorkspace::new();
    let input = ws.path("padded_input.png");
    let output = ws.path("padded_output.png");

    write_bordered_png(&input, 20, 20, 5, [255, 255, 255, 255], [0, 0, 255, 255]);

    let (info, _) = trim_save_and_reload(&input, &output, 0, 2, None);

    assert_eq!(info.offset_x, 3);
    assert_eq!(info.offset_y, 3);
    assert_eq!(info.trim_w, 14);
    assert_eq!(info.trim_h, 14);
}

/// 画像端を越える padding の余白が背景色で塗られ、保存後も維持されること
#[test]
fn trim_with_padding_beyond_image_persists_after_save() {
    let ws = TempWorkspace::new();
    let input = ws.path("extended_input.png");
    let output = ws.path("extended_output.png");

    write_bordered_png(&input, 10, 10, 1, [255, 255, 255, 255], [0, 0, 255, 255]);

    let (info, reloaded) = trim_save_and_reload(&input, &output, 0, 5, None);

    // 内容 8px + 余白 5px*2 に揃う
    assert_eq!(info.trim_w, 18);
    assert_eq!(info.trim_h, 18);
    assert_eq!(sample_pixel(&reloaded, 0, 0), Some([255, 255, 255, 255]));
    assert_eq!(sample_pixel(&reloaded, 5, 5), Some([0, 0, 255, 255]));
    assert_eq!(sample_pixel(&reloaded, 17, 17), Some([255, 255, 255, 255]));
}

/// 手動背景色を白に指定したとき、自動検出と同じ結果でラウンドトリップできること
#[test]
fn trim_with_manual_background_round_trips() {
    let ws = TempWorkspace::new();
    let input = ws.path("manual_input.png");
    let output = ws.path("manual_output.png");

    write_bordered_png(&input, 10, 10, 2, [255, 255, 255, 255], [255, 0, 0, 255]);

    let img = open_image(&input).expect("入力画像の読み込みに失敗");
    let manual_white = [255, 255, 255, 255];
    let auto = trim(&img, 0, 0, None);
    let manual = trim(&img, 0, 0, Some(manual_white));

    assert_eq!(manual.background, manual_white);
    assert_eq!(auto.info, manual.info);

    save_image(&auto.image, None, None, &output).expect("トリミング結果の保存に失敗");
    let reloaded = open_image(&output).expect("出力画像の再読み込みに失敗");
    assert_eq!(reloaded.width(), manual.info.trim_w);
    assert_eq!(reloaded.height(), manual.info.trim_h);
}

/// 一括トリミング相当の入出力パスで複数ファイルを処理できること
#[test]
fn batch_trim_workflow_writes_expected_output_files() {
    let ws = TempWorkspace::new();
    let out_dir = ws.path("batch_out");
    fs::create_dir_all(&out_dir).expect("出力ディレクトリの作成に失敗");

    let sources = ["photo_a.png", "photo_b.png"];
    for name in sources {
        let input = ws.path(name);
        write_bordered_png(&input, 12, 8, 2, [255, 255, 255, 255], [0, 128, 0, 255]);
    }

    let mut saved = 0usize;
    for name in sources {
        let input = ws.path(name);
        let img = open_image(&input).expect("一括入力の読み込みに失敗");
        let trimmed = trim(&img, 0, 0, None);
        let out_path = trimmed_output_path(&input, &out_dir, OutputFormat::Original);

        assert_eq!(
            out_path,
            out_dir.join(format!(
                "{}_trimmed.png",
                name.strip_suffix(".png").unwrap()
            ))
        );

        save_image(&trimmed.image, None, None, &out_path).expect("一括出力の保存に失敗");
        assert!(out_path.is_file());

        let reloaded = open_image(&out_path).expect("一括出力の再読み込みに失敗");
        assert_eq!(reloaded.width(), trimmed.info.trim_w);
        assert_eq!(reloaded.height(), trimmed.info.trim_h);
        assert_eq!(trimmed.info.trim_w, 8);
        assert_eq!(trimmed.info.trim_h, 4);
        saved += 1;
    }

    assert_eq!(saved, sources.len());
}

/// JPEG 形式でも読み込み・トリミング・保存のラウンドトリップができること
#[test]
fn trim_pipeline_round_trips_through_jpeg_file() {
    let ws = TempWorkspace::new();
    let input = ws.path("input.jpg");
    let output = ws.path("output.jpg");

    write_bordered_png(
        &ws.path("source.png"),
        10,
        10,
        2,
        [255, 255, 255, 255],
        [255, 0, 0, 255],
    );
    // PNG で作成した画像を JPEG として保存 (アプリが扱う JPEG 入出力を模倣)
    let source = open_image(&ws.path("source.png")).expect("ソース画像の読み込みに失敗");
    source.save(&input).expect("JPEG 入力の書き出しに失敗");

    let (info, reloaded) = trim_save_and_reload(&input, &output, 0, 0, None);

    assert_eq!(info.orig_w, 10);
    assert_eq!(info.orig_h, 10);
    // JPEG は非可逆圧縮のため境界付近の色が変化し、PNG と同じ厳密寸法にはならない
    assert!(info.trim_w <= 10);
    assert!(info.trim_h <= 10);
    assert!(info.trim_w >= 6);
    assert!(info.trim_h >= 6);
    assert_eq!(reloaded.width(), info.trim_w);
    assert_eq!(reloaded.height(), info.trim_h);
}

/// 画像以外のファイルを開くとエラーになること
#[test]
fn open_image_rejects_non_image_file() {
    let ws = TempWorkspace::new();
    let path = ws.path("not_an_image.txt");
    fs::write(&path, b"not an image").expect("ダミーファイルの書き出しに失敗");

    let err = open_image(&path).expect_err("非画像ファイルは拒否されるべき");
    let msg = err.to_string();
    assert!(
        msg.contains("読み込みエラー"),
        "エラーメッセージに読み込みエラーが含まれる: {msg}"
    );
}

/// 辺ごとの余白・除去する辺・アスペクト比の指定が、保存後のファイルにも反映されること
#[test]
fn trim_with_options_persists_after_save() {
    let ws = TempWorkspace::new();
    let input = ws.path("options_input.png");
    let output = ws.path("options_output.png");

    // 20x20 の白地に、縁 5 px を除いた 10x10 の青い内容
    write_bordered_png(&input, 20, 20, 5, [255, 255, 255, 255], [0, 0, 255, 255]);
    let img = open_image(&input).expect("open input");

    let opts = TrimOptions {
        margins: Margins {
            top: 0,
            right: 0,
            bottom: 0,
            left: 2,
        },
        // 下辺は除去せず元画像の端まで残す
        sides: TrimSides {
            bottom: false,
            ..TrimSides::ALL
        },
        aspect: Some(AspectRatio {
            w: 1,
            h: 1,
            fit: AspectFit::Expand,
        }),
        ..TrimOptions::default()
    };
    let trimmed = trim_with_options(&img, &opts);
    save_image(&trimmed.image, None, None, &output).expect("save output");
    let reloaded = open_image(&output).expect("reload output");

    // 左 2 + 内容 10 = 幅 12、上の内容端 5 から下端 20 までで高さ 15。1:1 に広げて 15x15
    assert_eq!((trimmed.info.trim_w, trimmed.info.trim_h), (15, 15));
    assert_eq!((reloaded.width(), reloaded.height()), (15, 15));
}
