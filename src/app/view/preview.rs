//! 元画像・トリミング後画像のプレビュー描画

use eframe::egui;

use super::icons::{self, Icon};
use crate::app::shortcuts;
use crate::app::state::{App, PaneView};
use crate::app::theme;
use crate::trim::TrimInfo;

/// 元画像プレビューで削られる領域に重ねる暗幕の色
const TRIM_SHADE_COLOR: egui::Color32 = egui::Color32::from_black_alpha(140);

/// 元画像プレビューに重ねるトリミング矩形の枠線の太さ
const TRIM_FRAME_WIDTH: f32 = 1.5;

/// 画像プレビューに敷く市松模様の 1 マスの大きさ
const PREVIEW_CHECKER_CELL: f32 = 8.0;

/// プレビューのズーム倍率の上限 (画像 1 px あたりの画面 px 数)
const MAX_ZOOM: f32 = 16.0;

/// スクロール 1 ポイントあたりのズーム量 (指数)
const ZOOM_PER_SCROLL: f32 = 0.002;

/// ペインに収める際に画像の周囲へ空ける余白
const PANE_MARGIN: f32 = 8.0;

/// ペインに収める倍率の下限 (極端に小さいペインでの倍率 0 を避ける)
const MIN_FIT_SCALE: f32 = 0.001;

/// 画像がないときのドロップ領域の中身の高さ (縦方向の中央寄せに使う)
const EMPTY_CONTENT_HEIGHT: f32 = 210.0;

/// 画像がないときのアイコンを囲む円の半径
const EMPTY_ICON_RADIUS: f32 = 34.0;

/// 1 ペイン分の表示内容
#[derive(Clone, Copy)]
struct PaneSpec<'a> {
    /// 見出し
    title: &'a str,
    /// 見出しの横に添える寸法
    dims: Option<[u32; 2]>,
    /// 表示するテクスチャ (`None` のときは見出しと空の領域だけを描く)
    tex: Option<&'a egui::TextureHandle>,
    /// トリミング矩形の見せ方
    crop: Option<CropView>,
    /// ペインの最大幅
    max_w: f32,
    /// `true` のとき画像クリックでピクセル座標を返す
    pick_mode: bool,
    /// 元画像の寸法。プレビュー縮小時も倍率とスポイト座標を元解像度へ合わせる
    source_size: Option<[u32; 2]>,
}

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
        let frame = egui::Frame::new()
            .fill(theme::palette_of(ui).canvas)
            .inner_margin(egui::Margin::same(10));
        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            if self.image.original.is_none() {
                self.render_empty_state(ui);
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

            // テクスチャは描画の直前に差し込む (スポイトで状態が変わるため借用を先に作らない)
            let trim_pane = PaneSpec {
                title: "トリミング後",
                dims: trim_dims,
                tex: None,
                crop: trim_clip,
                max_w: avail.x,
                pick_mode: false,
                source_size: orig_dims,
            };

            if self.prefs.compact_mode {
                let pane = PaneSpec {
                    tex: self.image.orig_tex.as_ref().filter(|_| trim_uv.is_some()),
                    ..trim_pane
                };
                show_panel(ui, &pane, &mut self.image.views[1]);
            } else {
                let col_w = (avail.x - 16.0) / 2.0;
                let ctx = ui.ctx().clone();
                ui.columns(2, |cols| {
                    let orig_pane = PaneSpec {
                        title: if self.background.pick_mode {
                            "元画像 (クリックで背景色を選択)"
                        } else {
                            "元画像"
                        },
                        dims: orig_dims,
                        tex: self.image.orig_tex.as_ref(),
                        crop: trim_uv.map(CropView::Overlay),
                        max_w: col_w,
                        pick_mode: self.background.pick_mode,
                        source_size: orig_dims,
                    };
                    if let Some((x, y)) =
                        show_panel(&mut cols[0], &orig_pane, &mut self.image.views[0])
                    {
                        self.pick_bg_from_pixel(&ctx, x, y);
                    }
                    let pane = PaneSpec {
                        tex: self.image.orig_tex.as_ref().filter(|_| trim_uv.is_some()),
                        max_w: col_w,
                        ..trim_pane
                    };
                    show_panel(&mut cols[1], &pane, &mut self.image.views[1]);
                });
            }
        });
    }

    /// 画像がないときのドロップ領域と、開く・貼り付けのボタン
    ///
    /// ファイルをウィンドウ上へドラッグしている間は、枠をアクセント色にしてドロップできることを示す
    fn render_empty_state(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let p = theme::palette_of(ui);
        let zone = ui.available_rect_before_wrap();
        let dragging = ctx.input(|i| !i.raw.hovered_files.is_empty());

        let (fill, border) = if dragging {
            (p.accent.gamma_multiply(0.08), p.accent)
        } else {
            (p.panel, p.border.gamma_multiply(1.6))
        };
        let painter = ui.painter();
        painter.rect_filled(zone, theme::CARD_RADIUS, fill);
        let corners = [
            zone.left_top(),
            zone.right_top(),
            zone.right_bottom(),
            zone.left_bottom(),
            zone.left_top(),
        ];
        painter.extend(egui::Shape::dashed_line(
            &corners,
            egui::Stroke::new(1.5, border),
            8.0,
            6.0,
        ));

        let content = egui::Rect::from_center_size(
            zone.center(),
            egui::vec2(zone.width(), EMPTY_CONTENT_HEIGHT),
        );
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(content)
                .layout(egui::Layout::top_down(egui::Align::Center)),
            |ui| {
                let (icon_rect, _) = ui.allocate_exact_size(
                    egui::Vec2::splat(EMPTY_ICON_RADIUS * 2.0),
                    egui::Sense::hover(),
                );
                ui.painter().circle_filled(
                    icon_rect.center(),
                    EMPTY_ICON_RADIUS,
                    p.accent.gamma_multiply(0.15),
                );
                Icon::Image.paint(
                    ui.painter(),
                    egui::Rect::from_center_size(icon_rect.center(), egui::Vec2::splat(32.0)),
                    p.accent,
                );
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new("画像をここにドロップ")
                        .size(19.0)
                        .strong(),
                );
                ui.add_space(2.0);
                ui.label(egui::RichText::new("背景の余白を自動で検出してトリミングします").weak());
                ui.add_space(14.0);
                self.render_empty_actions(ui, &ctx);
            },
        );
        ui.allocate_rect(zone, egui::Sense::hover());
    }

    /// 画像がないときの開く・貼り付けボタンを横に並べて中央に置く
    ///
    /// 横並びの中央寄せには行の幅が要るため、前のフレームで測った幅を使う
    fn render_empty_actions(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let width_id = egui::Id::unique("empty_actions_width");
        let width = ctx.data(|d| d.get_temp::<f32>(width_id)).unwrap_or(0.0);
        ui.horizontal(|ui| {
            ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
            let start = ui.cursor().min.x;
            let size = egui::vec2(0.0, 32.0);
            if ui
                .add(icons::primary_button(ui, Icon::Folder, "画像を開く").min_size(size))
                .on_hover_text(shortcuts::with_shortcut(
                    ctx,
                    "画像ファイルを開く",
                    shortcuts::OPEN,
                ))
                .clicked()
            {
                self.open_image(ctx);
            }
            if ui
                .add(icons::button(Icon::Paste, "クリップボードから貼り付け").min_size(size))
                .on_hover_text(shortcuts::with_shortcut(
                    ctx,
                    "クリップボードの画像を使用",
                    shortcuts::PASTE,
                ))
                .clicked()
            {
                self.paste_from_clipboard(ctx);
            }
            let used = ui.min_rect().max.x - start;
            if (used - width).abs() > 0.5 {
                ctx.data_mut(|d| d.insert_temp(width_id, used));
                ctx.request_repaint();
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
fn paint_checkerboard(painter: &egui::Painter, rect: egui::Rect, cell: f32) {
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
/// * `painter` - ペインにクリップした描画先
/// * `frame_color` - 枠線の色
/// * `image_rect` - 画面上の画像の表示領域 (ペインからはみ出していてもよい)
/// * `uv` - 正規化座標のトリミング矩形
fn paint_trim_overlay(
    painter: &egui::Painter,
    frame_color: egui::Color32,
    image_rect: egui::Rect,
    uv: egui::Rect,
) {
    if uv.contains_rect(FULL_UV) {
        return;
    }
    let to_screen = |p: egui::Pos2| image_rect.min + p.to_vec2() * image_rect.size();
    let crop = egui::Rect::from_min_max(to_screen(uv.min), to_screen(uv.max));

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
        egui::Stroke::new(TRIM_FRAME_WIDTH, frame_color),
        egui::StrokeKind::Inside,
    );
}

/// タイトルとズーム・パン可能な画像を 1 ペイン分、カードに収めて描画する
///
/// ホイールでカーソル位置を中心にズームし、ドラッグでパンする
/// タイトル行の「フィット」「100%」で倍率を戻せる
/// `pane.crop` が `Overlay` のときは削られる領域を重ねて表示し、
/// `Clip` のときはテクスチャから矩形部分だけを UV で切り出して表示し、
/// 元画像の範囲外へはみ出す部分は塗り色で描く
///
/// # Arguments
/// * `ui` - 描画先
/// * `pane` - ペインの表示内容
/// * `view` - ペインのズームとパンの状態
///
/// # Returns
/// * `Option<(u32, u32)>` - スポイト中に画像がクリックされたときの元画像上のピクセル座標
fn show_panel(ui: &mut egui::Ui, pane: &PaneSpec, view: &mut PaneView) -> Option<(u32, u32)> {
    theme::card_frame(ui, 6)
        .show(ui, |ui| show_pane_contents(ui, pane, view))
        .inner
}

/// ペインの中身 (見出し行と画像) を描画する
///
/// 引数と戻り値は `show_panel` と同じ
fn show_pane_contents(
    ui: &mut egui::Ui,
    pane: &PaneSpec,
    view: &mut PaneView,
) -> Option<(u32, u32)> {
    let PaneSpec {
        title,
        dims,
        tex,
        crop,
        max_w,
        pick_mode,
        source_size,
    } = *pane;
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.strong(title);
        if let Some([w, h]) = dims {
            ui.label(egui::RichText::new(format!("{w} × {h}")).small().weak());
        }
        if tex.is_some() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("100%").clicked() {
                    *view = PaneView {
                        zoom: Some(1.0),
                        offset: egui::Vec2::ZERO,
                    };
                }
                if ui.small_button("フィット").clicked() {
                    *view = PaneView::default();
                }
            });
        }
    });
    ui.add_space(2.0);
    let viewport_size = egui::vec2(
        max_w.min(ui.available_width()).max(1.0),
        ui.available_height().max(1.0),
    );
    let (viewport, response) = ui.allocate_exact_size(viewport_size, egui::Sense::click_and_drag());
    let t = tex?;

    let (uv, fill) = match crop {
        Some(CropView::Clip { uv, fill }) => (uv, Some(fill)),
        _ => (FULL_UV, None),
    };
    let [sw, sh] = source_size.unwrap_or_else(|| tex_size_px(t.size_vec2()));
    #[allow(clippy::cast_precision_loss)]
    let src = egui::vec2(sw as f32, sh as f32) * uv.size();
    let fit = fit_scale(src, viewport.size());

    // ズームとパンの入力を反映する
    let mut scale = view.zoom.unwrap_or(fit).clamp(fit, MAX_ZOOM);
    if response.hovered() {
        let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta().y, i.zoom_delta()));
        let factor = pinch * (scroll * ZOOM_PER_SCROLL).exp();
        if let Some(pointer) = response.hover_pos().filter(|_| (factor - 1.0).abs() > 1e-4) {
            let new_scale = (scale * factor).clamp(fit, MAX_ZOOM);
            view.offset = zoom_about(view.offset, pointer - viewport.center(), scale, new_scale);
            scale = new_scale;
        }
    }
    if response.dragged_by(egui::PointerButton::Primary) {
        view.offset += response.drag_delta();
    }
    // 収まる倍率まで戻したら、ウィンドウサイズへの追従に戻す
    view.zoom = (scale > fit + 1e-3).then_some(scale);
    let disp = src * scale;
    view.offset = clamp_offset(view.offset, disp, viewport.size());

    let painter = ui.painter_at(viewport);
    let palette = theme::palette_of(ui);
    painter.rect_filled(viewport, theme::CARD_RADIUS - 4, palette.canvas);
    // 等倍付近で画素がにじまないよう、画像の位置を物理ピクセルに揃える
    let ppp = painter.pixels_per_point();
    let min = (viewport.center() + view.offset - disp / 2.0).to_vec2();
    let min = egui::Pos2::ZERO + (min * ppp).round() / ppp;
    let rect = egui::Rect::from_min_size(min, disp);

    // 透明部分がパネルの背景に溶けないよう、画像の下に市松模様を敷く
    paint_checkerboard(&painter, rect, PREVIEW_CHECKER_CELL);
    // 範囲外の余白を塗ってから、元画像と重なる部分だけをテクスチャから描く
    if let Some(fill) = fill.filter(|_| !FULL_UV.contains_rect(uv)) {
        painter.rect_filled(rect, 0.0, fill);
    }
    let inner_uv = uv.intersect(FULL_UV);
    let to_screen = |p: egui::Pos2| rect.min + (p - uv.min) / uv.size() * rect.size();
    let inner = egui::Rect::from_min_max(to_screen(inner_uv.min), to_screen(inner_uv.max));
    painter.image(t.id(), inner, inner_uv, egui::Color32::WHITE);
    if let Some(CropView::Overlay(trim_uv)) = crop {
        paint_trim_overlay(&painter, palette.accent, rect, trim_uv);
    }
    paint_zoom_badge(&painter, viewport, scale);

    // カーソルの形でパン可能なことと、スポイト中であることを示す
    if response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if response.hovered() {
        if pick_mode {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        } else if disp.x > viewport.width() || disp.y > viewport.height() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
    }

    pick_mode
        .then(|| {
            response
                .interact_pointer_pos()
                .filter(|_| response.clicked())
        })
        .flatten()
        .filter(|pos| rect.contains(*pos))
        .map(|pos| pixel_at(pos - rect.min, rect.size(), [sw, sh]))
}

/// テクスチャの寸法を整数のピクセル数へ変換する
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn tex_size_px(size: egui::Vec2) -> [u32; 2] {
    [size.x as u32, size.y as u32]
}

/// 画像内の相対位置を元画像のピクセル座標へ変換する
///
/// # Arguments
/// * `rel` - 画像表示領域の左上からの位置 (画面 px)
/// * `disp` - 画像表示領域の大きさ (画面 px)
/// * `source` - 元画像の寸法
///
/// # Returns
/// * `(u32, u32)` - 画像内へクランプしたピクセル座標
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
fn pixel_at(rel: egui::Vec2, disp: egui::Vec2, source: [u32; 2]) -> (u32, u32) {
    let [sw, sh] = source;
    let px = ((rel.x / disp.x) * sw as f32)
        .clamp(0.0, sw.saturating_sub(1) as f32)
        .floor() as u32;
    let py = ((rel.y / disp.y) * sh as f32)
        .clamp(0.0, sh.saturating_sub(1) as f32)
        .floor() as u32;
    (px, py)
}

// ======================================================================
// ズームとパンの計算
// ======================================================================
/// ペインに収まる倍率を返す (拡大はしないため最大 1.0)
///
/// # Arguments
/// * `src` - 表示する範囲の元画像上の大きさ (px)
/// * `viewport` - ペインの大きさ (画面 px)
///
/// # Returns
/// * `f32` - 画像 1 px あたりの画面 px 数
fn fit_scale(src: egui::Vec2, viewport: egui::Vec2) -> f32 {
    let src = src.max(egui::Vec2::splat(1.0));
    ((viewport.x - PANE_MARGIN) / src.x)
        .min((viewport.y - PANE_MARGIN) / src.y)
        .clamp(MIN_FIT_SCALE, 1.0)
}

/// 画像がペインからはみ出さない範囲へパンの移動量を収める
///
/// 画像がペインより小さい軸は中央に固定する
///
/// # Arguments
/// * `offset` - 画像の中心のずれ (画面 px)
/// * `disp` - 画像の表示サイズ (画面 px)
/// * `viewport` - ペインの大きさ (画面 px)
///
/// # Returns
/// * `egui::Vec2` - 収めた後の画像の中心のずれ
fn clamp_offset(offset: egui::Vec2, disp: egui::Vec2, viewport: egui::Vec2) -> egui::Vec2 {
    let limit = ((disp - viewport) / 2.0).max(egui::Vec2::ZERO);
    offset.clamp(-limit, limit)
}

/// カーソル下の画像上の点を動かさずに倍率を変えたときのパンの移動量を返す
///
/// # Arguments
/// * `offset` - 変更前の画像の中心のずれ (画面 px)
/// * `pointer` - ペインの中心からのカーソル位置 (画面 px)
/// * `old_scale` - 変更前の倍率
/// * `new_scale` - 変更後の倍率
///
/// # Returns
/// * `egui::Vec2` - 変更後の画像の中心のずれ
fn zoom_about(
    offset: egui::Vec2,
    pointer: egui::Vec2,
    old_scale: f32,
    new_scale: f32,
) -> egui::Vec2 {
    pointer - (pointer - offset) * (new_scale / old_scale)
}

/// ペインの右下に現在の倍率を表示する
///
/// # Arguments
/// * `painter` - ペインにクリップした描画先
/// * `viewport` - ペインの領域
/// * `scale` - 画像 1 px あたりの画面 px 数
fn paint_zoom_badge(painter: &egui::Painter, viewport: egui::Rect, scale: f32) {
    let galley = painter.layout_no_wrap(
        format!("{:.0}%", scale * 100.0),
        egui::FontId::proportional(12.0),
        egui::Color32::WHITE,
    );
    let pos = viewport.right_bottom() - galley.size() - egui::vec2(10.0, 8.0);
    painter.rect_filled(
        egui::Rect::from_min_size(pos, galley.size()).expand2(egui::vec2(4.0, 2.0)),
        4.0,
        egui::Color32::from_black_alpha(160),
    );
    painter.galley(pos, galley, egui::Color32::WHITE);
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// 大きな画像はペインに収まる倍率になり、小さな画像は 1.0 を超えないこと
    #[test]
    fn fit_scale_shrinks_large_and_never_enlarges() {
        let viewport = egui::vec2(408.0, 308.0);
        let large = fit_scale(egui::vec2(4000.0, 1000.0), viewport);
        assert!((large - 0.1).abs() < 1e-6);
        assert!((fit_scale(egui::vec2(100.0, 100.0), viewport) - 1.0).abs() < f32::EPSILON);
    }

    /// 寸法 0 の範囲やペインが極端に小さい場合でも正の倍率を返すこと
    #[test]
    fn fit_scale_stays_positive_for_degenerate_input() {
        assert!(fit_scale(egui::Vec2::ZERO, egui::vec2(100.0, 100.0)) > 0.0);
        assert!(fit_scale(egui::vec2(500.0, 500.0), egui::vec2(4.0, 4.0)) > 0.0);
    }

    /// 画像がペインより小さい軸は中央に固定し、大きい軸ははみ出さない範囲に収めること
    #[test]
    fn clamp_offset_keeps_image_covering_pane() {
        let viewport = egui::vec2(400.0, 300.0);
        // 横だけはみ出す画像: 縦は中央固定、横は (800 - 400) / 2 まで
        let disp = egui::vec2(800.0, 200.0);
        let clamped = clamp_offset(egui::vec2(999.0, 50.0), disp, viewport);
        assert_eq!(clamped, egui::vec2(200.0, 0.0));
        let clamped = clamp_offset(egui::vec2(-999.0, -50.0), disp, viewport);
        assert_eq!(clamped, egui::vec2(-200.0, 0.0));
    }

    /// 倍率を変えてもカーソル下の画像上の点が動かないこと
    #[test]
    fn zoom_about_keeps_point_under_pointer() {
        let offset = egui::vec2(30.0, -20.0);
        let pointer = egui::vec2(70.0, 40.0);
        let (old, new) = (0.5, 2.0);
        // 拡大前にカーソル下にあった画像上の点が、拡大後も同じ画面位置に来ること
        let image_point = (pointer - offset) / old;
        let new_offset = zoom_about(offset, pointer, old, new);
        let after = new_offset + image_point * new;
        assert!((after - pointer).length() < 1e-4);
    }

    /// 表示領域外の位置は画像内へクランプし、縮小表示でも元解像度の座標を返すこと
    #[test]
    fn pixel_at_maps_to_source_resolution() {
        let disp = egui::vec2(100.0, 50.0);
        assert_eq!(
            pixel_at(egui::vec2(50.0, 25.0), disp, [1000, 500]),
            (500, 250)
        );
        assert_eq!(
            pixel_at(egui::vec2(-5.0, 999.0), disp, [1000, 500]),
            (0, 499)
        );
    }
}
