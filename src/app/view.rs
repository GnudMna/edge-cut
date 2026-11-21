//! UI 描画 (`eframe::App` 実装)

use std::path::PathBuf;

use super::state::App;
use crate::consts::{PADDING_MAX, PADDING_MIN, TOL_MAX, TOL_MIN};

use eframe::egui;

// ======================================================================
// UI 描画
// ======================================================================
impl eframe::App for App {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.save_settings(storage);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_batch_result();
        self.poll_trim_work(&ctx);
        apply_theme(&ctx, self.prefs.dark_mode);
        handle_file_drop(self, &ctx);

        let sp = if self.prefs.compact_mode { 1.0 } else { 3.0 };
        self.render_bottom_panel(ui, sp);
        self.render_top_panel(ui, &ctx);
        self.render_central_panel(ui);
    }
}

impl App {
    fn render_bottom_panel(&self, ui: &mut egui::Ui, sp: f32) {
        egui::Panel::bottom("bottom").show_inside(ui, |ui| {
            ui.add_space(sp);
            ui.horizontal(|ui| {
                let status = if self.trim_worker.is_busy() {
                    format!("{} (トリミング中…)", self.status)
                } else {
                    self.status.clone()
                };
                let status_label =
                    ui.label(egui::RichText::new(status).color(ui.visuals().text_color()));
                if !self.batch.last_error_details.is_empty() {
                    status_label.on_hover_text(self.batch.last_error_details.join("\n"));
                }
                if let Some(ref info) = self.image.info {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let detail = if self.prefs.compact_mode {
                            format!(
                                "{}×{}→{}×{}",
                                info.orig_w, info.orig_h, info.trim_w, info.trim_h,
                            )
                        } else {
                            format!(
                                "元 {}×{}  →  トリム {}×{}  (x={} y={})",
                                info.orig_w,
                                info.orig_h,
                                info.trim_w,
                                info.trim_h,
                                info.offset_x,
                                info.offset_y,
                            )
                        };
                        ui.label(egui::RichText::new(detail).weak());
                    });
                }
            });
            ui.add_space(sp);
        });
    }

    fn render_top_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::Panel::top("top").show_inside(ui, |ui| {
            ui.add_space(if self.prefs.compact_mode { 2.0 } else { 6.0 });
            self.render_action_buttons(ui, ctx);

            if !self.prefs.compact_mode {
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(2.0);
            }

            self.render_trim_settings(ui, ctx);
            ui.add_space(if self.prefs.compact_mode { 2.0 } else { 6.0 });
        });
    }

    fn render_action_buttons(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            if ui
                .button(egui::RichText::new("📂 開く").strong())
                .on_hover_text("画像ファイルを開く")
                .clicked()
            {
                self.open_image(ctx);
            }

            if ui
                .button("📂 一括トリミング")
                .on_hover_text("複数の画像をまとめてトリミングして保存 (元のフォーマットを維持)")
                .clicked()
                && !self.batch.running
            {
                self.batch_trim(ctx);
            }

            if self.batch.running
                && ui
                    .button(
                        egui::RichText::new("⏹ キャンセル")
                            .color(egui::Color32::from_rgb(200, 80, 80)),
                    )
                    .on_hover_text("一括トリミングを中止")
                    .clicked()
            {
                self.cancel_batch();
            }

            if ui
                .button("📋 貼り付け")
                .on_hover_text("クリップボードの画像を使用")
                .clicked()
            {
                self.paste_from_clipboard(ctx);
            }

            ui.add_enabled_ui(self.image.trimmed.is_some(), |ui| {
                if ui
                    .button("💾 保存")
                    .on_hover_text("トリミング後の画像を保存")
                    .clicked()
                {
                    self.save_image();
                }
            });

            ui.add_enabled_ui(self.image.trimmed.is_some(), |ui| {
                if ui
                    .button("📋 コピー")
                    .on_hover_text("トリミング後の画像をクリップボードへコピー")
                    .clicked()
                {
                    self.copy_trimmed_to_clipboard();
                }
            });

            ui.add_enabled_ui(self.image.original.is_some(), |ui| {
                if ui
                    .button(egui::RichText::new("🗑 クリア").color(if ui.is_enabled() {
                        egui::Color32::from_rgb(200, 80, 80)
                    } else {
                        ui.visuals().weak_text_color()
                    }))
                    .on_hover_text("画像をクリア")
                    .clicked()
                {
                    self.clear_image();
                }
            });

            ui.add_space(if self.prefs.compact_mode { 8.0 } else { 16.0 });
            ui.separator();
            ui.add_space(if self.prefs.compact_mode { 2.0 } else { 4.0 });
            self.render_auto_save_controls(ui);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let icon = if self.prefs.dark_mode {
                    "☀ ライト"
                } else {
                    "🌙 ダーク"
                };
                if ui.button(icon).clicked() {
                    self.prefs.dark_mode = !self.prefs.dark_mode;
                }
                let compact_icon = if self.prefs.compact_mode {
                    "⊞ 標準"
                } else {
                    "⊟ コンパクト"
                };
                if ui
                    .button(compact_icon)
                    .on_hover_text("コンパクト UI モードの切替 (トリミング後のみ表示)")
                    .clicked()
                {
                    self.prefs.compact_mode = !self.prefs.compact_mode;
                }
            });
        });
    }

    fn render_trim_settings(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.label("許容度:");
            let tol_resp = ui.add(
                egui::Slider::new(&mut self.prefs.tolerance, TOL_MIN..=TOL_MAX)
                    .clamping(egui::SliderClamping::Always),
            );
            if slider_needs_trim(&tol_resp, ctx, &mut self.prefs.tolerance, TOL_MIN, TOL_MAX) {
                let debounce = tol_resp.dragged() && !tol_resp.drag_stopped();
                self.schedule_trim(ctx, debounce, false, false);
            }
            tol_resp.on_hover_text("背景色との類似度。値が大きいほど広い範囲を余白とみなす");

            ui.add_space(12.0);
            ui.label("余白:");
            let pad_resp = ui.add(
                egui::Slider::new(&mut self.prefs.padding, PADDING_MIN..=PADDING_MAX)
                    .suffix(" px")
                    .clamping(egui::SliderClamping::Always),
            );
            if slider_needs_trim(
                &pad_resp,
                ctx,
                &mut self.prefs.padding,
                PADDING_MIN,
                PADDING_MAX,
            ) {
                let debounce = pad_resp.dragged() && !pad_resp.drag_stopped();
                self.schedule_trim(ctx, debounce, false, false);
            }
            pad_resp.on_hover_text("トリミング後に四辺へ追加する余白ピクセル数");

            if self.image.original.is_some() {
                ui.add_space(if self.prefs.compact_mode { 8.0 } else { 16.0 });
                ui.separator();
                ui.add_space(if self.prefs.compact_mode { 2.0 } else { 4.0 });
                self.render_bg_color_controls(ui, ctx);
            }
        });
    }

    fn render_auto_save_controls(&mut self, ui: &mut egui::Ui) {
        let prev_auto_save = self.prefs.auto_save;
        ui.checkbox(&mut self.prefs.auto_save, "自動保存");
        if self.prefs.auto_save && !prev_auto_save {
            self.auto_save_if_enabled();
        }

        ui.add_space(4.0);
        let full_path = self.prefs.auto_save_dir.to_string_lossy().into_owned();
        let dir_btn = ui
            .button("📁 変更")
            .on_hover_text(if self.prefs.compact_mode {
                full_path.as_str()
            } else {
                "保存先フォルダを変更"
            });
        if dir_btn.clicked()
            && let Some(dir) = rfd::FileDialog::new()
                .set_directory(&self.prefs.auto_save_dir)
                .pick_folder()
        {
            self.prefs.auto_save_dir = dir;
        }
        if !self.prefs.compact_mode {
            ui.label(egui::RichText::new(&full_path).weak().monospace());
        }
    }

    fn render_bg_color_controls(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.label("背景色:");
        if let Some(bg) = self.background.bg_color {
            show_bg_swatch(ui, bg);
            let mode = if self.background.bg_override.is_some() {
                "手動"
            } else {
                "自動"
            };
            if !self.prefs.compact_mode {
                ui.label(
                    egui::RichText::new(format!(
                        "RGBA({}, {}, {}, {})",
                        bg[0], bg[1], bg[2], bg[3]
                    ))
                    .monospace()
                    .weak(),
                );
            }
            ui.label(egui::RichText::new(mode).small().weak());
        } else {
            ui.label(egui::RichText::new("—").weak());
        }

        let mut edit_color = self.background.bg_color.unwrap_or([255, 255, 255, 255]);
        if ui
            .color_edit_button_srgba_unmultiplied(&mut edit_color)
            .on_hover_text("背景色を直接編集")
            .changed()
        {
            self.set_manual_bg(ctx, edit_color);
        }

        let auto_btn = ui
            .add_enabled(
                self.background.bg_override.is_some(),
                egui::Button::new("自動"),
            )
            .on_hover_text("四辺から背景色を自動検出する");
        if auto_btn.clicked() {
            self.reset_bg_to_auto(ctx);
        }

        let pick_label = if self.background.pick_mode {
            "スポイト中"
        } else {
            "スポイト"
        };
        if ui
            .selectable_label(self.background.pick_mode, pick_label)
            .on_hover_text("元画像をクリックして背景色を取得する")
            .clicked()
        {
            self.background.pick_mode = !self.background.pick_mode;
        }
    }

    fn render_central_panel(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show_inside(ui, |ui| {
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

            if self.prefs.compact_mode {
                let col_w = avail.x - 8.0;
                let trim_label = trim_dims.map_or_else(
                    || "トリミング後".into(),
                    |[w, h]| format!("トリミング後  {w}×{h}"),
                );
                show_panel(
                    ui,
                    &trim_label,
                    self.image.trim_tex.as_ref(),
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
                        col_w,
                        self.background.pick_mode,
                        orig_dims,
                    ) {
                        self.pick_bg_from_pixel(&ctx, x, y);
                    }
                    show_panel(
                        &mut cols[1],
                        &trim_label,
                        self.image.trim_tex.as_ref(),
                        col_w,
                        false,
                        None,
                    );
                });
            }
        });
    }
}

// ======================================================================
// UI ヘルパー
// ======================================================================
fn apply_theme(ctx: &egui::Context, dark_mode: bool) {
    if dark_mode {
        ctx.set_visuals(egui::Visuals::dark());
    } else {
        ctx.set_visuals(egui::Visuals::light());
    }
}

fn handle_file_drop(app: &mut App, ctx: &egui::Context) {
    let dropped: Vec<PathBuf> = ctx.input(|i| {
        i.raw
            .dropped_files
            .iter()
            .filter_map(|f| f.path.clone())
            .collect()
    });
    if let Some(path) = dropped.into_iter().next() {
        app.load_image_from_path(ctx, &path);
    }
}

/// スライダーのレスポンスを受け取り、トリミングを再実行すべきかどうかを返す
///
/// 以下のいずれかの条件を満たす場合に `true` を返し、`value` を更新する:
/// - 値が変化した (ドラッグ中を含む)
/// - ドラッグ操作が完了した
/// - ホバー中かつ未フォーカス状態で左右矢印キーが押された
fn slider_needs_trim(
    resp: &egui::Response,
    ctx: &egui::Context,
    value: &mut u32,
    min: u32,
    max: u32,
) -> bool {
    if resp.changed() || resp.drag_stopped() {
        return true;
    }
    if resp.hovered() && !resp.has_focus() {
        let delta: i32 = ctx.input(|i| {
            i32::from(i.key_pressed(egui::Key::ArrowRight))
                - i32::from(i.key_pressed(egui::Key::ArrowLeft))
        });
        if delta != 0 {
            let current = i64::from(*value);
            let new_val = (current + i64::from(delta)).clamp(i64::from(min), i64::from(max));
            *value = u32::try_from(new_val).unwrap_or(min);
            return true;
        }
    }
    false
}

/// 背景色スウォッチを描画する (半透明のときは市松模様を敷く)
fn show_bg_swatch(ui: &mut egui::Ui, rgba: [u8; 4]) {
    let size = egui::vec2(22.0, 22.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter();
    if rgba[3] < 255 {
        let cell = 5.0;
        let light = egui::Color32::from_gray(210);
        let dark = egui::Color32::from_gray(170);
        let mut y = rect.min.y;
        let mut row = 0;
        while y < rect.max.y {
            let mut x = rect.min.x;
            let mut col = 0;
            while x < rect.max.x {
                let w = (x + cell).min(rect.max.x) - x;
                let h = (y + cell).min(rect.max.y) - y;
                let color = if (row + col) % 2 == 0 { light } else { dark };
                painter.rect_filled(
                    egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h)),
                    0.0,
                    color,
                );
                x += cell;
                col += 1;
            }
            y += cell;
            row += 1;
        }
    }
    painter.rect_filled(
        rect,
        3.0,
        egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]),
    );
    painter.rect_stroke(
        rect,
        3.0,
        egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        egui::StrokeKind::Inside,
    );
}

/// タイトルとスクロール可能な画像を 1 ペイン分描画する
///
/// `pick_mode` が `true` のとき画像クリックでピクセル座標を返す
/// `pick_source_size` は元画像の寸法。プレビュー縮小時もスポイト座標を元解像度へ合わせる
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn show_panel(
    ui: &mut egui::Ui,
    title: &str,
    tex: Option<&egui::TextureHandle>,
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
                let ts = t.size_vec2();
                let scale = ((max_w - 8.0) / ts.x).min((avail_h - 8.0) / ts.y).min(1.0);
                let disp = ts * scale;
                let sense = if pick_mode {
                    egui::Sense::click()
                } else {
                    egui::Sense::hover()
                };
                let response = ui.add(
                    egui::Image::new((t.id(), disp))
                        .sense(sense)
                        .corner_radius(egui::CornerRadius::ZERO),
                );
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
