//! 元画像・トリミング後画像のプレビュー描画

use crate::app::state::App;
use crate::trim::TrimInfo;

use eframe::egui;

/// 元画像プレビューで削られる領域に重ねる暗幕の色
const TRIM_SHADE_COLOR: egui::Color32 = egui::Color32::from_black_alpha(140);

/// 元画像プレビューに重ねるトリミング矩形の枠線の太さ
const TRIM_FRAME_WIDTH: f32 = 1.5;

/// 画像プレビューに敷く市松模様の 1 マスの大きさ
const PREVIEW_CHECKER_CELL: f32 = 8.0;

/// プレビューでのトリミング矩形の見せ方
#[derive(Clone, Copy)]
enum CropView {
    /// 画像全体を表示し、矩形の外側を暗くして枠線を重ねる
    Overlay(egui::Rect),
    /// 矩形の内側だけを UV で切り出して表示する
    ///
    /// 矩形が元画像の範囲外へはみ出す部分は `fill` で塗る
    Clip {
        /// 正規化座標のトリミング矩形 (範囲外の余白を含むため 0–1 を超えうる)
        uv: egui::Rect,
        /// 範囲外の余白を塗る色
        fill: egui::Color32,
    },
}

// ======================================================================
// プレビュー
// ======================================================================
impl App {
    pub(super) fn render_central_panel(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            if self.image.original.is_none() {
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(ui.available_height() / 2.0 - 40.0);
                        ui.label(
                            egui::RichText::new("📂 画像をここにドロップ")
                                .size(24.0)
                                .weak(),
                        );
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new("または「開く」ボタンから画像を選択")
                                .size(13.0)
                                .weak(),
                        );
                    });
                });
                return;
            }

            let avail = ui.available_size();
            let orig_dims = self
                .image
                .original
                .as_ref()
                .map(|img| [img.width(), img.height()]);
            let trim_dims = self.image.info.map(|info| [info.trim_w, info.trim_h]);
            let trim_uv = self.image.info.as_ref().map(trim_uv_rect);
            let trim_clip = self.image.info.zip(trim_uv).map(|(info, uv)| {
                let [r, g, b, a] = info.fill;
                CropView::Clip {
                    uv,
                    fill: egui::Color32::from_rgba_unmultiplied(r, g, b, a),
                }
            });

            if self.prefs.compact_mode {
                let col_w = avail.x - 8.0;
                let trim_label = trim_dims.map_or_else(
                    || "トリミング後".into(),
                    |[w, h]| format!("トリミング後  {w}×{h}"),
                );
                show_panel(
                    ui,
                    &trim_label,
                    self.image.orig_tex.as_ref().filter(|_| trim_uv.is_some()),
                    trim_clip,
                    col_w,
                    false,
                    None,
                );
            } else {
                let col_w = (avail.x - 16.0) / 2.0;
                let ctx = ui.ctx().clone();
                ui.columns(2, |cols| {
                    let orig_label = if self.background.pick_mode {
                        orig_dims.map_or_else(
                            || "元画像 (クリックで背景色を選択)".into(),
                            |[w, h]| format!("元画像 (クリックで背景色を選択)  {w}×{h}"),
                        )
                    } else {
                        orig_dims
                            .map_or_else(|| "元画像".into(), |[w, h]| format!("元画像  {w}×{h}"))
                    };
                    let trim_label = trim_dims.map_or_else(
                        || "トリミング後".into(),
                        |[w, h]| format!("トリミング後  {w}×{h}"),
                    );
                    if let Some((x, y)) = show_panel(
                        &mut cols[0],
                        &orig_label,
                        self.image.orig_tex.as_ref(),
                        trim_uv.map(CropView::Overlay),
                        col_w,
                        self.background.pick_mode,
                        orig_dims,
                    ) {
                        self.pick_bg_from_pixel(&ctx, x, y);
                    }
                    show_panel(
                        &mut cols[1],
                        &trim_label,
                        self.image.orig_tex.as_ref().filter(|_| trim_uv.is_some()),
                        trim_clip,
                        col_w,
                        false,
                        None,
                    );
                });
            }
        });
    }
}

/// 透明部分を見分けるための市松模様を `rect` に描く
///
/// マス目は `rect` の左上を基準に並べ、描画先のクリップ領域と重なるマスだけを描く
///
/// # Arguments
/// * `painter` - 描画先
/// * `rect` - 市松模様を敷く領域
/// * `cell` - 1 マスの一辺の長さ
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(super) fn paint_checkerboard(painter: &egui::Painter, rect: egui::Rect, cell: f32) {
    let visible = rect.intersect(painter.clip_rect());
    if !visible.is_positive() {
        return;
    }
    // 大きなプレビューでも見えている範囲のマスだけを描くよう、開始位置をマス目に揃える
    let first_col = ((visible.min.x - rect.min.x) / cell).floor() as usize;
    let first_row = ((visible.min.y - rect.min.y) / cell).floor() as usize;
    let light = egui::Color32::from_gray(210);
    let dark = egui::Color32::from_gray(170);
    let mut row = first_row;
    #[allow(clippy::cast_precision_loss)]
    let mut y = rect.min.y + row as f32 * cell;
    while y < visible.max.y {
        let mut col = first_col;
        #[allow(clippy::cast_precision_loss)]
        let mut x = rect.min.x + col as f32 * cell;
        while x < visible.max.x {
            let tile = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(cell, cell));
            let color = if (row + col).is_multiple_of(2) {
                light
            } else {
                dark
            };
            painter.rect_filled(tile.intersect(rect), 0.0, color);
            x += cell;
            col += 1;
        }
        y += cell;
        row += 1;
    }
}

/// 画像全体を表す正規化座標の矩形
const FULL_UV: egui::Rect = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));

/// トリミング矩形を元画像に対する正規化座標 (UV) で返す
///
/// プレビューテクスチャは縮小されている場合があるため、ピクセルではなく比率で表す
/// 範囲外へ拡張する余白も含めるため、0 未満や 1 を超える座標になりうる
#[allow(clippy::cast_precision_loss)]
fn trim_uv_rect(info: &TrimInfo) -> egui::Rect {
    if info.orig_w == 0 || info.orig_h == 0 {
        return FULL_UV;
    }
    let (w, h) = (info.orig_w as f32, info.orig_h as f32);
    let min = egui::pos2(
        (info.offset_x as f32 - info.extend_left as f32) / w,
        (info.offset_y as f32 - info.extend_top as f32) / h,
    );
    egui::Rect::from_min_size(
        min,
        egui::vec2(info.trim_w as f32 / w, info.trim_h as f32 / h),
    )
}

/// 元画像プレビューに、削られる領域の暗幕とトリミング矩形の枠線を重ねる
///
/// 矩形が画像全体を含むときは何も描かない
/// 範囲外へ拡張する余白は元画像上に描けないため、枠線は画像の表示領域内に収める
///
/// # Arguments
/// * `ui` - 描画先
/// * `image_rect` - 画面上の画像の表示領域
/// * `uv` - 正規化座標のトリミング矩形
fn paint_trim_overlay(ui: &egui::Ui, image_rect: egui::Rect, uv: egui::Rect) {
    if uv.contains_rect(FULL_UV) {
        return;
    }
    let to_screen = |p: egui::Pos2| image_rect.min + p.to_vec2() * image_rect.size();
    let crop = egui::Rect::from_min_max(to_screen(uv.min), to_screen(uv.max));
    let painter = ui.painter_at(image_rect);

    // 上下は全幅、左右は矩形の高さ分だけ塗り、暗幕が重ならないようにする
    let shades = [
        egui::Rect::from_min_max(image_rect.min, egui::pos2(image_rect.max.x, crop.min.y)),
        egui::Rect::from_min_max(egui::pos2(image_rect.min.x, crop.max.y), image_rect.max),
        egui::Rect::from_min_max(
            egui::pos2(image_rect.min.x, crop.min.y),
            egui::pos2(crop.min.x, crop.max.y),
        ),
        egui::Rect::from_min_max(
            egui::pos2(crop.max.x, crop.min.y),
            egui::pos2(image_rect.max.x, crop.max.y),
        ),
    ];
    for shade in shades.into_iter().filter(egui::Rect::is_positive) {
        painter.rect_filled(shade, 0.0, TRIM_SHADE_COLOR);
    }
    painter.rect_stroke(
        crop.intersect(image_rect),
        0.0,
        egui::Stroke::new(TRIM_FRAME_WIDTH, ui.visuals().selection.stroke.color),
        egui::StrokeKind::Inside,
    );
}

/// タイトルとスクロール可能な画像を 1 ペイン分描画する
///
/// `crop` が `Overlay` のときは削られる領域を重ねて表示し、
/// `Clip` のときはテクスチャから矩形部分だけを UV で切り出して表示し、
/// 元画像の範囲外へはみ出す部分は塗り色で描く
/// `pick_mode` が `true` のとき画像クリックでピクセル座標を返す
/// `pick_source_size` は元画像の寸法。プレビュー縮小時もスポイト座標を元解像度へ合わせる
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn show_panel(
    ui: &mut egui::Ui,
    title: &str,
    tex: Option<&egui::TextureHandle>,
    crop: Option<CropView>,
    max_w: f32,
    pick_mode: bool,
    pick_source_size: Option<[u32; 2]>,
) -> Option<(u32, u32)> {
    ui.strong(title);
    ui.separator();
    let avail_h = ui.available_height();
    let mut picked = None;
    egui::ScrollArea::both()
        .id_salt(title)
        .max_width(max_w)
        .max_height(avail_h)
        .show(ui, |ui| {
            if let Some(t) = tex {
                let (uv, fill) = match crop {
                    Some(CropView::Clip { uv, fill }) => (uv, Some(fill)),
                    _ => (FULL_UV, None),
                };
                let ts = t.size_vec2() * uv.size();
                let scale = ((max_w - 8.0) / ts.x).min((avail_h - 8.0) / ts.y).min(1.0);
                let disp = ts * scale;
                let sense = if pick_mode {
                    egui::Sense::click()
                } else {
                    egui::Sense::hover()
                };
                let (rect, response) = ui.allocate_exact_size(disp, sense);
                // 透明部分がパネルの背景に溶けないよう、画像の下に市松模様を敷く
                paint_checkerboard(ui.painter(), rect, PREVIEW_CHECKER_CELL);
                // 範囲外の余白を塗ってから、元画像と重なる部分だけをテクスチャから描く
                if let Some(fill) = fill.filter(|_| !FULL_UV.contains_rect(uv)) {
                    ui.painter().rect_filled(rect, 0.0, fill);
                }
                let inner_uv = uv.intersect(FULL_UV);
                let to_screen = |p: egui::Pos2| rect.min + (p - uv.min) / uv.size() * rect.size();
                let inner =
                    egui::Rect::from_min_max(to_screen(inner_uv.min), to_screen(inner_uv.max));
                egui::Image::new((t.id(), inner.size()))
                    .uv(inner_uv)
                    .corner_radius(egui::CornerRadius::ZERO)
                    .paint_at(ui, inner);
                if let Some(CropView::Overlay(trim_uv)) = crop {
                    paint_trim_overlay(ui, response.rect, trim_uv);
                }
                if pick_mode {
                    if response.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                    }
                    if let Some(pos) = response
                        .interact_pointer_pos()
                        .filter(|_| response.clicked())
                    {
                        let rel = pos - response.rect.min;
                        let [sw, sh] = pick_source_size.unwrap_or([ts.x as u32, ts.y as u32]);
                        #[allow(clippy::cast_precision_loss)]
                        {
                            let px = ((rel.x / response.rect.width()) * sw as f32)
                                .clamp(0.0, sw.saturating_sub(1) as f32)
                                .floor() as u32;
                            let py = ((rel.y / response.rect.height()) * sh as f32)
                                .clamp(0.0, sh.saturating_sub(1) as f32)
                                .floor() as u32;
                            picked = Some((px, py));
                        }
                    }
                }
            }
        });
    picked
}
