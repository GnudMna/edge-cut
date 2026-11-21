// テスト用の画像生成ヘルパー (ユニットテスト・統合テスト共通)

use image::{DynamicImage, Rgba, RgbaImage};

/// 単色の RGBA 画像を生成する
///
/// # Arguments
/// * `w` - 幅 (px)
/// * `h` - 高さ (px)
/// * `color` - 塗りつぶす RGBA 値
///
/// # Returns
/// * `DynamicImage` - 生成した RGBA8 画像
#[allow(clippy::missing_panics_doc, clippy::must_use_candidate, dead_code)]
pub fn solid_rgba(w: u32, h: u32, color: [u8; 4]) -> DynamicImage {
    let buf: Vec<u8> = std::iter::repeat_n(color, (w * h) as usize)
        .flatten()
        .collect();
    DynamicImage::ImageRgba8(RgbaImage::from_raw(w, h, buf).unwrap())
}

/// 縁と内側で色が異なる RGBA 画像を生成する
///
/// # Arguments
/// * `w` - 幅 (px)
/// * `h` - 高さ (px)
/// * `border` - 縁の太さ (px)
/// * `bg` - 縁の RGBA 値
/// * `fg` - 内側の RGBA 値
///
/// # Returns
/// * `DynamicImage` - 生成した RGBA8 画像
#[allow(clippy::missing_panics_doc, clippy::must_use_candidate)]
pub fn bordered_rgba(w: u32, h: u32, border: u32, bg: [u8; 4], fg: [u8; 4]) -> DynamicImage {
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let color = if x < border || x >= w - border || y < border || y >= h - border {
                Rgba(bg)
            } else {
                Rgba(fg)
            };
            img.put_pixel(x, y, color);
        }
    }
    DynamicImage::ImageRgba8(img)
}
