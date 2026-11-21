use std::fs;
use std::path::Path;

use image::{ColorType, DynamicImage, ImageFormat, Luma, LumaA, Rgba, RgbaImage};

use super::*;
use crate::consts::{JPEG_QUALITY_DEFAULT, JPEG_QUALITY_MAX, JPEG_QUALITY_MIN};
use crate::image_load::open_image_with_icc;

/// 左半分が完全透明、右半分が不透明な赤の RGBA 画像
fn half_transparent_image() -> DynamicImage {
    DynamicImage::ImageRgba8(RgbaImage::from_fn(8, 8, |x, _| {
        if x < 4 {
            Rgba([0, 0, 0, 0])
        } else {
            Rgba([255, 0, 0, 255])
        }
    }))
}

/// 指定した色空間をヘッダに持つ ICC プロファイル風のバイト列
///
/// 埋め込み・取り出しの検証用で、プロファイルとしての妥当性は問わない
fn fake_icc(color_space: [u8; 4]) -> Vec<u8> {
    let mut icc = vec![0u8; 132];
    icc[0..4].copy_from_slice(&132u32.to_be_bytes());
    icc[16..20].copy_from_slice(&color_space);
    icc[36..40].copy_from_slice(b"acsp");
    icc
}

// ======================================================================
// JPEG 変換

/// 透過を含む画像を JPEG で保存でき、透明部分が白になること
#[test]
fn save_image_flattens_alpha_for_jpeg() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let path = dir.join("out.jpg");
    save_image(&half_transparent_image(), None, &path).expect("JPEG 保存");

    // 透明だった左側は白、不透明だった右側は赤として再読み込みされる
    let reloaded = image::open(&path).expect("再読み込み").to_rgb8();
    let left = reloaded.get_pixel(1, 4).0;
    let right = reloaded.get_pixel(6, 4).0;
    assert!(left.iter().all(|&c| c > 240), "left = {left:?}");
    assert!(
        right[0] > 200 && right[1] < 60 && right[2] < 60,
        "right = {right:?}"
    );
}

/// JPEG 品質が出力サイズに反映され、範囲外の値は丸められること
#[test]
fn save_image_with_applies_jpeg_quality() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    // ノイズを含む画像で品質差をファイルサイズに出す
    let noisy = RgbaImage::from_fn(64, 64, |x, y| {
        Rgba([
            u8::try_from((x * 37 + y * 11) % 256).unwrap(),
            u8::try_from((x * 5 + y * 53) % 256).unwrap(),
            u8::try_from((x * y) % 256).unwrap(),
            255,
        ])
    });
    let img = DynamicImage::ImageRgba8(noisy);
    let size_at = |quality: u8| {
        let path = dir.join(format!("q{quality}.jpg"));
        save_image_with(
            &img,
            None,
            &path,
            SaveOptions {
                jpeg_quality: quality,
            },
        )
        .expect("JPEG 保存");
        fs::metadata(&path).expect("metadata").len()
    };

    // 品質が高いほどサイズが大きい
    assert!(size_at(20) < size_at(95));
    // 範囲外の値は有効範囲の端と同じ結果になる
    assert_eq!(size_at(0), size_at(JPEG_QUALITY_MIN));
    assert_eq!(size_at(200), size_at(JPEG_QUALITY_MAX));
}

/// 既定のオプションが `JPEG_QUALITY_DEFAULT` で、`save_image` と同じ出力になること
#[test]
fn save_options_default_matches_save_image() {
    assert_eq!(SaveOptions::default().jpeg_quality, JPEG_QUALITY_DEFAULT);

    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let (a, b) = (dir.join("a.jpg"), dir.join("b.jpg"));
    save_image(&half_transparent_image(), None, &a).expect("save_image");
    save_image_with(&half_transparent_image(), None, &b, SaveOptions::default())
        .expect("save_image_with");
    assert_eq!(fs::read(&a).unwrap(), fs::read(&b).unwrap());
}

/// 16bit 画像も JPEG で保存できること
#[test]
fn save_image_converts_16bit_for_jpeg() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let path = dir.join("out.jpeg");
    let img = DynamicImage::ImageRgba16(image::ImageBuffer::from_pixel(
        4,
        4,
        Rgba([65535u16, 0, 0, 65535]),
    ));
    save_image(&img, None, &path).expect("16bit 画像の JPEG 保存");
    assert!(path.is_file());
}

/// 全出力形式で各カラータイプの画像を保存・再読み込みできること
#[test]
fn save_image_supports_all_formats_and_color_types() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let rgba = half_transparent_image();
    let images = [
        DynamicImage::ImageLuma8(rgba.to_luma8()),
        DynamicImage::ImageLumaA8(rgba.to_luma_alpha8()),
        DynamicImage::ImageRgb8(rgba.to_rgb8()),
        rgba.clone(),
        DynamicImage::ImageLuma16(rgba.to_luma16()),
        DynamicImage::ImageLumaA16(rgba.to_luma_alpha16()),
        DynamicImage::ImageRgb16(rgba.to_rgb16()),
        DynamicImage::ImageRgba16(rgba.to_rgba16()),
        DynamicImage::ImageRgb32F(rgba.to_rgb32f()),
        DynamicImage::ImageRgba32F(rgba.to_rgba32f()),
    ];
    for format in OutputFormat::ALL {
        if format == OutputFormat::Original {
            continue;
        }
        for img in &images {
            let path = dir.join(format!(
                "{:?}_{:?}.{}",
                format,
                img.color(),
                format.output_extension(Path::new(""))
            ));
            save_image(img, None, &path)
                .unwrap_or_else(|e| panic!("{format:?} / {:?}: {e}", img.color()));
            let reloaded = image::open(&path).expect("再読み込み");
            assert_eq!((reloaded.width(), reloaded.height()), (8, 8));
        }
    }
}

/// 拡張子から形式を判定できない場合はエラーとなり、既存ファイルを壊さないこと
#[test]
fn save_image_unknown_extension_keeps_existing_file() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let path = dir.join("out.xyz");
    fs::write(&path, b"keep").expect("placeholder");

    let result = save_image(&half_transparent_image(), None, &path);
    assert!(matches!(result, Err(SaveError::Encode(_))));
    assert_eq!(fs::read(&path).expect("read"), b"keep");
}

/// 既存ファイルを新しい内容で上書きし、一時ファイルを残さないこと
#[test]
fn save_image_overwrites_without_leaving_temp_file() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let path = dir.join("out.png");
    fs::write(&path, b"old").expect("placeholder");

    save_image(&half_transparent_image(), None, &path).expect("上書き保存");
    assert!(image::open(&path).is_ok());
    // ディレクトリには保存先のファイルだけが残る
    assert_eq!(fs::read_dir(dir).expect("read_dir").count(), 1);
}

/// 置き換えに失敗した場合はエラーとなり、一時ファイルを残さないこと
#[test]
fn save_image_replace_failure_cleans_up_temp_file() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    // 保存先がディレクトリのため rename が失敗する
    let path = dir.join("out.png");
    fs::create_dir(&path).expect("blocking dir");

    let result = save_image(&half_transparent_image(), None, &path);
    assert!(matches!(result, Err(SaveError::Io(_))));
    assert!(path.is_dir());
    assert_eq!(fs::read_dir(dir).expect("read_dir").count(), 1);
}

// ======================================================================
// ICC プロファイル
// ======================================================================
/// ICC 対応形式で保存すると元のプロファイルがそのまま埋め込まれること
#[test]
fn save_image_embeds_icc_profile() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let icc = fake_icc(*b"RGB ");
    // `image` の TIFF デコーダはタグ値の読み込み上限を画像バッファサイズに合わせるため、
    // 極小画像だと ICC タグを読めない。実用的な大きさの画像で検証する
    let img = DynamicImage::ImageRgba8(RgbaImage::from_pixel(64, 64, Rgba([255, 0, 0, 255])));
    for ext in ["png", "jpg", "tif", "webp"] {
        let path = dir.join(format!("out.{ext}"));
        save_image(&img, Some(&icc), &path).expect("保存");

        // 再読み込みで同じバイト列が取り出せる
        let loaded = open_image_with_icc(&path).expect("再読み込み");
        assert_eq!(loaded.icc_profile.as_deref(), Some(icc.as_slice()), "{ext}");
    }
}

/// ICC プロファイル未指定なら埋め込まないこと
#[test]
fn save_image_without_icc_profile() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let path = dir.join("out.png");
    save_image(&half_transparent_image(), None, &path).expect("保存");
    assert!(
        open_image_with_icc(&path)
            .expect("再読み込み")
            .icc_profile
            .is_none()
    );
}

/// CMYK プロファイルは RGB 画像に埋め込まないこと
#[test]
fn save_image_skips_mismatched_icc_color_space() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let path = dir.join("out.jpg");
    save_image(&half_transparent_image(), Some(&fake_icc(*b"CMYK")), &path).expect("保存");
    assert!(
        open_image_with_icc(&path)
            .expect("再読み込み")
            .icc_profile
            .is_none()
    );
}

/// グレースケールのプロファイルは、JPEG 化で RGB になった画像には埋め込まないこと
#[test]
fn save_image_skips_gray_icc_after_rgb_conversion() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let path = dir.join("out.jpg");
    save_image(&half_transparent_image(), Some(&fake_icc(*b"GRAY")), &path).expect("保存");
    assert!(
        open_image_with_icc(&path)
            .expect("再読み込み")
            .icc_profile
            .is_none()
    );
}

/// グレースケール画像は JPEG でもグレースケールのまま保存し、GRAY プロファイルを引き継ぐこと
#[test]
fn save_image_keeps_gray_icc_for_grayscale_jpeg() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let icc = fake_icc(*b"GRAY");
    let images = [
        DynamicImage::ImageLuma8(image::ImageBuffer::from_pixel(4, 4, Luma([128]))),
        DynamicImage::ImageLumaA8(image::ImageBuffer::from_pixel(4, 4, LumaA([128, 255]))),
        DynamicImage::ImageLuma16(image::ImageBuffer::from_pixel(4, 4, Luma([32896]))),
        DynamicImage::ImageLumaA16(image::ImageBuffer::from_pixel(4, 4, LumaA([32896, 65535]))),
    ];
    for img in &images {
        let path = dir.join(format!("{:?}.jpg", img.color()));
        save_image(img, Some(&icc), &path).expect("JPEG 保存");

        // 8bit グレースケールとして保存され、プロファイルも埋め込まれる
        let loaded = open_image_with_icc(&path).expect("再読み込み");
        assert_eq!(loaded.image.color(), ColorType::L8, "{:?}", img.color());
        assert_eq!(
            loaded.icc_profile.as_deref(),
            Some(icc.as_slice()),
            "{:?}",
            img.color()
        );
    }
}

/// グレースケールの透過画像を JPEG で保存すると、透明部分が背景色に合成されること
#[test]
fn save_image_flattens_gray_alpha_for_jpeg() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let path = dir.join("out.jpg");
    // 左半分は透明な黒、右半分は不透明な黒
    let img = DynamicImage::ImageLumaA8(image::ImageBuffer::from_fn(8, 8, |x, _| {
        LumaA([0, if x < 4 { 0 } else { 255 }])
    }));
    save_image(&img, None, &path).expect("JPEG 保存");

    let reloaded = image::open(&path).expect("再読み込み").to_luma8();
    let left = reloaded.get_pixel(1, 4).0[0];
    let right = reloaded.get_pixel(6, 4).0[0];
    assert!(left > 240, "left = {left}");
    assert!(right < 15, "right = {right}");
}

/// ICC 非対応形式ではプロファイルを破棄して保存に成功すること
#[test]
fn save_image_ignores_icc_for_unsupported_format() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let path = dir.join("out.bmp");
    save_image(&half_transparent_image(), Some(&fake_icc(*b"RGB ")), &path).expect("保存");
    assert!(path.is_file());
}
// ======================================================================
// 連番保存
// ======================================================================
/// 同名ファイルが無いときは指定名のまま保存すること
#[test]
fn save_unique_uses_base_name_when_not_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let (path, renamed) =
        save_image_unique(&half_transparent_image(), None, dir, "photo", "png").expect("保存");
    assert_eq!(path, dir.join("photo.png"));
    assert!(!renamed);
    assert!(path.is_file());
}

/// 既存ファイルを上書きせず連番サフィックスで回避すること
#[test]
fn save_unique_appends_suffix_without_overwriting() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let existing = dir.join("photo.png");
    fs::write(&existing, b"x").expect("placeholder");

    let (path, renamed) =
        save_image_unique(&half_transparent_image(), None, dir, "photo", "png").expect("保存");
    assert_eq!(path, dir.join("photo_2.png"));
    assert!(renamed);
    // 既存ファイルの内容は変わらない
    assert_eq!(fs::read(&existing).expect("read"), b"x");
}

/// 連番で回避した場合も一時ファイルを残さないこと
#[test]
fn save_unique_leaves_no_temp_file() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let img = half_transparent_image();
    save_image_unique(&img, None, dir, "photo", "png").expect("1 回目");
    save_image_unique(&img, None, dir, "photo", "png").expect("2 回目");

    let mut names: Vec<_> = fs::read_dir(dir)
        .expect("read_dir")
        .map(|e| e.expect("entry").file_name())
        .collect();
    names.sort();
    assert_eq!(names, ["photo.png", "photo_2.png"]);
}
/// トリミング結果が `{名前}_trimmed.{拡張子}` で保存され、2 回目は連番になること
#[test]
fn save_trimmed_output_names_and_suffixes() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let img = half_transparent_image();
    let src = Path::new("in/photo.JPG");

    let (first, renamed_first) =
        save_trimmed_output(&img, None, src, dir, OutputFormat::Original).expect("1 回目");
    assert_eq!(first, dir.join("photo_trimmed.jpg"));
    assert!(!renamed_first);

    let (second, renamed_second) =
        save_trimmed_output(&img, None, src, dir, OutputFormat::Original).expect("2 回目");
    assert_eq!(second, dir.join("photo_trimmed_2.jpg"));
    assert!(renamed_second);

    // 形式を指定すると元の拡張子に関係なくその形式で保存する
    let (webp, _) =
        save_trimmed_output(&img, None, src, dir, OutputFormat::Webp).expect("WebP 保存");
    assert_eq!(webp, dir.join("photo_trimmed.webp"));
    assert_eq!(
        ImageFormat::from_path(&webp).expect("形式"),
        image::guess_format(&fs::read(&webp).expect("read")).expect("推定")
    );
}
