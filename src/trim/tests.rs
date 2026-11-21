use image::{DynamicImage, Rgba, RgbaImage};

use super::*;
use crate::test_helpers::{bordered_rgba, solid_rgba};

// ======================================================================
// トリム
// ======================================================================
/// トリミング前後で元画像の幅と高さが保持されていること
#[test]
fn trim_info_preserves_original_dimensions() {
    let img = bordered_rgba(20, 16, 3, [255, 255, 255, 255], [0, 0, 0, 255]);
    let result = trim(&img, 0, 0, None);
    assert_eq!(result.info.orig_w, 20);
    assert_eq!(result.info.orig_h, 16);
}

/// 縁が正しく削除されること
#[test]
fn trim_removes_white_border() {
    // 10x10 / 縁 2px 白 / 中央 6x6 赤
    let img = bordered_rgba(10, 10, 2, [255, 255, 255, 255], [255, 0, 0, 255]);
    let result = trim(&img, 0, 0, None);
    assert_eq!(result.info.offset_x, 2);
    assert_eq!(result.info.offset_y, 2);
    assert_eq!(result.info.trim_w, 6);
    assert_eq!(result.info.trim_h, 6);
}

/// トリミング後の画像サイズが情報と一致すること
#[test]
fn trim_result_image_size_matches_info() {
    let img = bordered_rgba(10, 10, 2, [255, 255, 255, 255], [255, 0, 0, 255]);
    let trimmed = trim(&img, 0, 0, None);
    assert_eq!(trimmed.image.width(), trimmed.info.trim_w);
    assert_eq!(trimmed.image.height(), trimmed.info.trim_h);
}

/// 全ピクセルが背景色の場合、トリミング後もサイズが変わらないこと
#[test]
fn trim_solid_background_returns_original_size() {
    // 全ピクセルが背景色 → トリミング後もサイズ変わらず
    let img = solid_rgba(8, 8, [255, 255, 255, 255]);
    let trimmed = trim(&img, 0, 0, None);
    assert_eq!(trimmed.info.trim_w, 8);
    assert_eq!(trimmed.info.trim_h, 8);
    assert_eq!(trimmed.image.width(), 8);
    assert_eq!(trimmed.image.height(), 8);
}

/// 並列走査の対象サイズでも全面背景の画像が元のサイズのままであること
#[test]
fn trim_parallel_solid_background_returns_original_size() {
    let img = solid_rgba(1024, 768, [255, 255, 255, 255]);
    let info = detect_trim(&img, 0, 0, None).info;
    assert_eq!((info.trim_w, info.trim_h), (1024, 768));
    assert_eq!((info.offset_x, info.offset_y), (0, 0));
}

/// 並列走査でも離れた 2 点の内容から上下左右の端を正しく求めること
#[test]
fn trim_parallel_finds_bounds_of_sparse_content() {
    let mut buf = RgbaImage::from_pixel(1024, 768, Rgba([255, 255, 255, 255]));
    buf.put_pixel(900, 100, Rgba([255, 0, 0, 255]));
    buf.put_pixel(30, 700, Rgba([255, 0, 0, 255]));
    let img = DynamicImage::ImageRgba8(buf);
    let info = detect_trim(&img, 0, 0, None).info;
    // x: 30..=900 / y: 100..=700
    assert_eq!((info.offset_x, info.offset_y), (30, 100));
    assert_eq!((info.trim_w, info.trim_h), (871, 601));
}

/// 1x1 の画像でもパニックしないこと
#[test]
fn trim_1x1_image_does_not_panic() {
    let img = solid_rgba(1, 1, [255, 0, 0, 255]);
    let trimmed = trim(&img, 0, 0, None);
    assert_eq!(trimmed.info.orig_w, 1);
    assert_eq!(trimmed.info.orig_h, 1);
    assert_eq!(trimmed.image.width(), 1);
    assert_eq!(trimmed.image.height(), 1);
}

/// 非対称画像のトリミング後の寸法が正しいこと
#[test]
fn trim_asymmetric_image_correct_dimensions() {
    // 30x20 / 縁 3px 白 / 中央 24x14 黒
    let img = bordered_rgba(30, 20, 3, [255, 255, 255, 255], [0, 0, 0, 255]);
    let result = trim(&img, 0, 0, None);
    assert_eq!(result.info.orig_w, 30);
    assert_eq!(result.info.orig_h, 20);
    assert_eq!(result.info.trim_w, 24);
    assert_eq!(result.info.trim_h, 14);
}

// ======================================================================
// トリム - パディング
// ======================================================================
/// padding を指定したとき、トリミング結果が正しく拡張されること
#[test]
fn trim_padding_expands_result() {
    // 20x20 / 縁 5px 白 / 中央 10x10 青
    // padding=2 → offset=3, size=14
    let img = bordered_rgba(20, 20, 5, [255, 255, 255, 255], [0, 0, 255, 255]);
    let result = trim(&img, 0, 2, None);
    assert_eq!(result.info.offset_x, 3);
    assert_eq!(result.info.offset_y, 3);
    assert_eq!(result.info.trim_w, 14);
    assert_eq!(result.info.trim_h, 14);
}

/// padding が画像端を越える分は範囲外の余白として追加されること
#[test]
fn trim_padding_extends_beyond_image_boundary() {
    // 10x10 / 縁 1px 白 / 中央 8x8 緑 / padding=10 → 8 + 10*2 = 28
    let img = bordered_rgba(10, 10, 1, [255, 255, 255, 255], [0, 255, 0, 255]);
    let result = trim(&img, 0, 10, None);
    let info = result.info;

    // 画像内は全体を切り出し、越えた 9px を四辺へ拡張する
    assert_eq!((info.offset_x, info.offset_y), (0, 0));
    assert_eq!((info.crop_w(), info.crop_h()), (10, 10));
    assert_eq!(
        (
            info.extend_left,
            info.extend_top,
            info.extend_right,
            info.extend_bottom
        ),
        (9, 9, 9, 9)
    );
    assert_eq!((info.trim_w, info.trim_h), (28, 28));
    assert_eq!(info.fill, [255, 255, 255, 255]);

    // 出力画像は背景色で塗られ、内容は padding の位置から始まる
    assert_eq!((result.image.width(), result.image.height()), (28, 28));
    assert_eq!(
        sample_pixel(&result.image, 0, 0),
        Some([255, 255, 255, 255])
    );
    assert_eq!(
        sample_pixel(&result.image, 9, 9),
        Some([255, 255, 255, 255])
    );
    assert_eq!(sample_pixel(&result.image, 10, 10), Some([0, 255, 0, 255]));
    assert_eq!(sample_pixel(&result.image, 17, 17), Some([0, 255, 0, 255]));
    assert_eq!(
        sample_pixel(&result.image, 18, 18),
        Some([255, 255, 255, 255])
    );
    assert_eq!(
        sample_pixel(&result.image, 27, 27),
        Some([255, 255, 255, 255])
    );
}

/// 内容が画像の片側に寄っている場合、越えた辺だけが拡張されること
#[test]
fn trim_padding_extends_only_exceeding_sides() {
    // 20x10 白 / (1,2)-(3,4) に黒 → 左 1px・上 2px しか余白がない
    let mut buf = RgbaImage::from_pixel(20, 10, image::Rgba([255, 255, 255, 255]));
    for y in 2..=4 {
        for x in 1..=3 {
            buf.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
        }
    }
    let img = DynamicImage::ImageRgba8(buf);
    let info = detect_trim(&img, 0, 3, None).info;

    // 左は 2px、上は 1px 不足し、右・下は画像内に収まる
    assert_eq!((info.offset_x, info.offset_y), (0, 0));
    assert_eq!(
        (
            info.extend_left,
            info.extend_top,
            info.extend_right,
            info.extend_bottom
        ),
        (2, 1, 0, 0)
    );
    assert_eq!((info.crop_w(), info.crop_h()), (7, 8));
    // 内容 3px + 余白 3px*2 で縦横とも 9px に揃う
    assert_eq!((info.trim_w, info.trim_h), (9, 9));
}

/// 範囲外の余白が透明な背景色で塗られること
#[test]
fn trim_padding_extension_uses_transparent_background() {
    let img = bordered_rgba(6, 6, 1, [0, 0, 0, 0], [255, 0, 0, 255]);
    let result = trim(&img, 0, 3, None);
    assert_eq!((result.image.width(), result.image.height()), (10, 10));
    assert_eq!(sample_pixel(&result.image, 0, 0).map(|p| p[3]), Some(0));
    assert_eq!(sample_pixel(&result.image, 3, 3), Some([255, 0, 0, 255]));
}

/// ほぼ透明な背景色が検出されても範囲外の余白は完全透明で塗られること
#[test]
fn trim_padding_extension_on_nearly_transparent_background_is_fully_transparent() {
    let img = bordered_rgba(6, 6, 1, [0, 0, 0, 3], [255, 0, 0, 255]);
    let result = trim(&img, 0, 3, None);
    // 背景色は実際の色のまま、塗り色だけアルファを 0 にする
    assert_eq!(result.background, [0, 0, 0, 3]);
    assert_eq!(result.info.fill, [0, 0, 0, 0]);
    assert_eq!(sample_pixel(&result.image, 0, 0), Some([0, 0, 0, 0]));
    assert_eq!(sample_pixel(&result.image, 3, 3), Some([255, 0, 0, 255]));
}

/// 範囲外の余白を追加しても元画像の色形式 (16bit) が保たれること
#[test]
fn trim_padding_extension_preserves_color_type() {
    let mut rgb = image::ImageBuffer::from_pixel(4, 4, image::Rgb([65535u16, 65535, 65535]));
    rgb.put_pixel(1, 1, image::Rgb([1000, 2000, 3000]));
    let img = DynamicImage::ImageRgb16(rgb);
    let result = trim(&img, 0, 2, None);

    let DynamicImage::ImageRgb16(out) = &result.image else {
        panic!("色形式が変わった: {:?}", result.image.color());
    };
    // 内容 1px + 余白 2px*2 → 左上 1px 分だけ拡張する
    assert_eq!(out.dimensions(), (5, 5));
    assert_eq!(out.get_pixel(0, 0).0, [65535, 65535, 65535]);
    assert_eq!(out.get_pixel(2, 2).0, [1000, 2000, 3000]);
}

/// 背景しかない画像には padding を指定しても余白を追加しないこと
#[test]
fn trim_padding_not_applied_to_solid_background() {
    let img = solid_rgba(8, 8, [255, 255, 255, 255]);
    let result = trim(&img, 0, 10, None);
    assert!(!result.info.is_extended());
    assert_eq!((result.image.width(), result.image.height()), (8, 8));
}

// ======================================================================
// トリム - 許容誤差
// ======================================================================
/// tol=0 のとき、正確なマッチングでトリミングが行われること
#[test]
fn trim_tolerance_zero_performs_exact_match_trim() {
    let img = bordered_rgba(10, 10, 2, [255, 255, 255, 255], [255, 0, 0, 255]);
    let result = trim(&img, 0, 0, None);
    assert_eq!(result.info.trim_w, 6);
    assert_eq!(result.info.trim_h, 6);
}

/// tol=100 のとき、すべての色が背景として扱われること
#[test]
fn trim_max_tolerance_treats_all_colors_as_background() {
    // tol=100 のとき背景(白)との RGB 二乗距離 130050 ≤ 195075 となり、
    // 赤 [255,0,0] も背景とみなされ → 全体が bg → サイズ変わらず
    let img = bordered_rgba(10, 10, 2, [255, 255, 255, 255], [255, 0, 0, 255]);
    let trimmed = trim(&img, 100, 0, None);
    assert_eq!(trimmed.info.trim_w, trimmed.info.orig_w);
    assert_eq!(trimmed.info.trim_h, trimmed.info.orig_h);
    assert_eq!(trimmed.image.width(), 10);
    assert_eq!(trimmed.image.height(), 10);
}

// ======================================================================
// トリム - 透明ピクセル
// ======================================================================
/// 完全に透明なピクセルが背景として扱われること
#[test]
fn trim_fully_transparent_border_treated_as_background() {
    let mut buf = RgbaImage::new(10, 10);
    for y in 0..10u32 {
        for x in 0..10u32 {
            let px = if !(2..8).contains(&x) || !(2..8).contains(&y) {
                image::Rgba([0u8, 0, 0, 0]) // 完全透明
            } else {
                image::Rgba([255u8, 0, 0, 255]) // 不透明赤
            };
            buf.put_pixel(x, y, px);
        }
    }
    let img = DynamicImage::ImageRgba8(buf);
    let result = trim(&img, 0, 0, None);
    assert_eq!(result.info.offset_x, 2);
    assert_eq!(result.info.offset_y, 2);
    assert_eq!(result.info.trim_w, 6);
    assert_eq!(result.info.trim_h, 6);
}

/// 四隅がすべて透明な画像は透明を背景色とし、半透明の内側を保持すること
///
/// 内側は半透明(alpha=100 >= 16) → 背景とみなされず保持される
#[test]
fn trim_transparent_canvas_uses_alpha_only_for_bg_detection() {
    let mut buf = RgbaImage::new(6, 6);
    for y in 0..6u32 {
        for x in 0..6u32 {
            let px = if x == 0 || x == 5 || y == 0 || y == 5 {
                image::Rgba([0u8, 0, 0, 0])
            } else {
                image::Rgba([128u8, 128, 128, 100])
            };
            buf.put_pixel(x, y, px);
        }
    }
    let img = DynamicImage::ImageRgba8(buf);
    let result = trim(&img, 0, 0, None);
    assert_eq!(result.info.offset_x, 1);
    assert_eq!(result.info.offset_y, 1);
    assert_eq!(result.info.trim_w, 4);
    assert_eq!(result.info.trim_h, 4);
}

/// 透明な背景上の半透明な影が許容度に応じて背景とみなされること
#[test]
fn trim_transparent_canvas_tolerance_removes_semi_transparent_shadow() {
    // 10x10 / 縁 1px 透明 / その内側 1px に半透明の影 (alpha=40) / 中央 6x6 不透明赤
    let mut buf = RgbaImage::new(10, 10);
    for y in 0..10u32 {
        for x in 0..10u32 {
            let px = if !(1..9).contains(&x) || !(1..9).contains(&y) {
                image::Rgba([0u8, 0, 0, 0])
            } else if !(2..8).contains(&x) || !(2..8).contains(&y) {
                image::Rgba([0u8, 0, 0, 40])
            } else {
                image::Rgba([255u8, 0, 0, 255])
            };
            buf.put_pixel(x, y, px);
        }
    }
    let img = DynamicImage::ImageRgba8(buf);

    // tol=0 では影も内容として残る
    let result = trim(&img, 0, 0, None);
    assert_eq!((result.info.offset_x, result.info.trim_w), (1, 8));

    // tol=15 (アルファ差 38 まで) では影 (アルファ差 40) はまだ残る
    let result = trim(&img, 15, 0, None);
    assert_eq!((result.info.offset_x, result.info.trim_w), (1, 8));

    // tol=16 (アルファ差 40 まで) で影が背景とみなされ、不透明な内容だけが残る
    let result = trim(&img, 16, 0, None);
    assert_eq!((result.info.offset_x, result.info.trim_w), (2, 6));

    // tol=99 (アルファ差 252 まで) でも不透明な内容 (アルファ差 255) は残る
    let result = trim(&img, 99, 0, None);
    assert_eq!((result.info.offset_x, result.info.trim_w), (2, 6));
}
// ======================================================================
// トリム - 不透明な背景上の半透明ピクセル
// ======================================================================
/// 不透明な背景と同じ RGB の半透明ピクセルが内容として残ること
#[test]
fn trim_semi_transparent_content_on_opaque_bg_is_kept() {
    // 10x10 / 縁 2px 不透明白 / 中央 6x6 半透明白 (alpha=100)
    let img = bordered_rgba(10, 10, 2, [255, 255, 255, 255], [255, 255, 255, 100]);
    let result = trim(&img, 0, 0, None);
    assert_eq!(result.background, [255, 255, 255, 255]);
    assert_eq!(result.info.offset_x, 2);
    assert_eq!(result.info.offset_y, 2);
    assert_eq!(result.info.trim_w, 6);
    assert_eq!(result.info.trim_h, 6);
}

/// 許容度最大ではアルファ差があっても全体を背景とみなすこと
#[test]
fn trim_max_tolerance_treats_semi_transparent_as_background() {
    // 不透明白の背景と半透明黒の内容は RGBA 距離が RGB 基準の上限を超えるが、
    // tol=100 では全体が背景 → サイズ変わらず
    let img = bordered_rgba(10, 10, 2, [255, 255, 255, 255], [0, 0, 0, 100]);
    let result = trim(&img, 100, 0, None);
    assert_eq!(result.info.trim_w, 10);
    assert_eq!(result.info.trim_h, 10);
}

/// 手動指定の背景色がトリミング結果に反映されること
#[test]
fn trim_manual_background_returns_specified_color() {
    let img = bordered_rgba(10, 10, 2, [255, 255, 255, 255], [255, 0, 0, 255]);
    let manual = [10, 20, 30, 255];
    let result = trim(&img, 0, 0, Some(manual));
    assert_eq!(result.background, manual);
}

/// 手動で白を指定したとき、自動検出と同じトリミング結果になること
#[test]
fn trim_manual_white_matches_auto_detection() {
    let img = bordered_rgba(10, 10, 2, [255, 255, 255, 255], [255, 0, 0, 255]);
    let auto = trim(&img, 0, 0, None);
    let manual = trim(&img, 0, 0, Some([255, 255, 255, 255]));
    assert_eq!(auto.info.trim_w, manual.info.trim_w);
    assert_eq!(auto.info.trim_h, manual.info.trim_h);
    assert_eq!(auto.info.offset_x, manual.info.offset_x);
    assert_eq!(auto.info.offset_y, manual.info.offset_y);
}
/// `sample_pixel` が画像内の色を正しく返すこと
#[test]
fn sample_pixel_returns_rgba_at_coordinates() {
    let img = bordered_rgba(10, 10, 2, [255, 255, 255, 255], [255, 0, 0, 255]);
    assert_eq!(sample_pixel(&img, 0, 0), Some([255, 255, 255, 255]));
    assert_eq!(sample_pixel(&img, 5, 5), Some([255, 0, 0, 255]));
    assert_eq!(sample_pixel(&img, 10, 0), None);
}

/// RGBA8 以外の画像でも `sample_pixel` が RGBA に変換した色を返すこと
#[test]
fn sample_pixel_converts_non_rgba8_pixel() {
    let mut rgb = image::RgbImage::from_pixel(4, 3, image::Rgb([10, 20, 30]));
    rgb.put_pixel(3, 2, image::Rgb([200, 100, 50]));
    let img = DynamicImage::ImageRgb8(rgb);

    // アルファは不透明として補われる
    assert_eq!(sample_pixel(&img, 0, 0), Some([10, 20, 30, 255]));
    assert_eq!(sample_pixel(&img, 3, 2), Some([200, 100, 50, 255]));

    // 範囲外は幅・高さどちらの超過でも `None`
    assert_eq!(sample_pixel(&img, 4, 0), None);
    assert_eq!(sample_pixel(&img, 0, 3), None);
}
