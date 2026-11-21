//! 画像余白トリミングロジックを提供
use eframe::egui;
use image::DynamicImage;

/// トリミング結果の情報
pub struct TrimInfo {
    /// 元画像の幅 (px)
    pub orig_w: u32,
    /// 元画像の高さ (px)
    pub orig_h: u32,
    /// トリミング後の幅 (px)
    pub trim_w: u32,
    /// トリミング後の高さ (px)
    pub trim_h: u32,
    /// トリミング開始位置の X オフセット (px)
    pub offset_x: u32,
    /// トリミング開始位置の Y オフセット (px)
    pub offset_y: u32,
}

/// `DynamicImage` を egui 描画用の `ColorImage` へ変換する
///
/// # Arguments
/// * `img` - 変換元の画像
///
/// # Returns
/// * `egui::ColorImage` - RGBA ピクセル列を持つ egui 用画像
pub fn to_color_image(img: &DynamicImage) -> egui::ColorImage {
    let rgba = img.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let pixels: Vec<egui::Color32> = rgba
        .as_raw()
        .chunks_exact(4)
        .map(|c| egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
        .collect();
    egui::ColorImage::new(size, pixels)
}

/// 四隅ピクセルの最頻色を背景色として検出する
///
/// # Arguments
/// * `rgba` - 対象画像の RGBA バッファ
/// * `w`    - 画像の幅 (px)
/// * `h`    - 画像の高さ (px)
///
/// # Returns
/// * `[u8; 4]` - 背景と推定される RGBA 値
fn detect_bg(rgba: &image::RgbaImage, w: u32, h: u32) -> [u8; 4] {
    let corners = [
        rgba.get_pixel(0, 0).0,
        rgba.get_pixel(w - 1, 0).0,
        rgba.get_pixel(0, h - 1).0,
        rgba.get_pixel(w - 1, h - 1).0,
    ];
    let mut freq: std::collections::HashMap<[u8; 4], u32> = std::collections::HashMap::new();
    for &c in &corners {
        *freq.entry(c).or_default() += 1;
    }
    freq.into_iter()
        .max_by_key(|(_, v)| *v)
        .map(|(k, _)| k)
        .unwrap()
}

/// 画像の四辺から背景余白をトリミングする
///
/// 許容度 `tol` は 0–100 の範囲で指定する。
/// 0 のとき背景色と完全一致するピクセルのみを除去し、
/// 100 のとき最大 RGB 二乗距離 (195,075) 以内のすべての色を背景とみなす。
/// `padding` はトリミング後に四辺に追加する余白ピクセル数。
///
/// # Arguments
/// * `img`     - トリミング元の画像
/// * `tol`     - 余白検出の許容度 (0–100)
/// * `padding` - トリミング後に残す余白ピクセル数
///
/// # Returns
/// * `(DynamicImage, TrimInfo)` - トリミング後の画像とトリミング情報
pub fn trim(img: &DynamicImage, tol: u32, padding: u32) -> (DynamicImage, TrimInfo) {
    let rgba = img.to_rgba8();
    let (w, h) = (img.width(), img.height());

    // 0x0 画像は演算がアンダーフローするためそのまま返す
    if w == 0 || h == 0 {
        return (
            img.clone(),
            TrimInfo {
                orig_w: w,
                orig_h: h,
                trim_w: w,
                trim_h: h,
                offset_x: 0,
                offset_y: 0,
            },
        );
    }

    let bg = detect_bg(&rgba, w, h);

    // RGB 各チャネルの差の二乗和がこの値以下なら背景とみなす
    let sq_thresh = (tol as u64 * tol as u64 * 195_075 / 10_000) as u32;

    let is_bg = |x: u32, y: u32| -> bool {
        let p = rgba.get_pixel(x, y).0;
        if p[3] < 16 {
            return true;
        } // ほぼ完全に透明なピクセル → 背景とみなす
        if bg[3] < 16 {
            return p[3] < 16;
        } // ほぼ完全に透明なキャンバス → アルファのみで判定
        let dr = p[0] as i32 - bg[0] as i32;
        let dg = p[1] as i32 - bg[1] as i32;
        let db = p[2] as i32 - bg[2] as i32;
        (dr * dr + dg * dg + db * db) as u32 <= sq_thresh
    };

    let top = (0..h).find(|&y| (0..w).any(|x| !is_bg(x, y))).unwrap_or(0);
    let bottom = (0..h)
        .rev()
        .find(|&y| (0..w).any(|x| !is_bg(x, y)))
        .unwrap_or(h - 1);

    if top > bottom {
        // 背景しかない → 元画像をそのまま返す
        return (
            img.clone(),
            TrimInfo {
                orig_w: w,
                orig_h: h,
                trim_w: w,
                trim_h: h,
                offset_x: 0,
                offset_y: 0,
            },
        );
    }

    let left = (0..w)
        .find(|&x| (top..=bottom).any(|y| !is_bg(x, y)))
        .unwrap_or(0);
    let right = (0..w)
        .rev()
        .find(|&x| (top..=bottom).any(|y| !is_bg(x, y)))
        .unwrap_or(w - 1);

    // padding を四辺に加算し、画像境界でクランプ
    let px_top = top.saturating_sub(padding);
    let px_bottom = (bottom + padding).min(h - 1);
    let px_left = left.saturating_sub(padding);
    let px_right = (right + padding).min(w - 1);

    let tw = px_right - px_left + 1;
    let th = px_bottom - px_top + 1;
    let info = TrimInfo {
        orig_w: w,
        orig_h: h,
        trim_w: tw,
        trim_h: th,
        offset_x: px_left,
        offset_y: px_top,
    };
    (img.crop_imm(px_left, px_top, tw, th), info)
}
