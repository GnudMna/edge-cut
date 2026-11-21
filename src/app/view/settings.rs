//! トリミング・出力・背景色の設定コントロールの描画

use super::preview::paint_checkerboard;
use crate::app::state::App;
use crate::consts::{
    JPEG_QUALITY_MAX, JPEG_QUALITY_MIN, PADDING_MAX, PADDING_MIN, TOL_MAX, TOL_MIN,
};
use crate::image_output::OutputFormat;

use eframe::egui;

/// 背景色スウォッチに敷く市松模様の 1 マスの大きさ
const SWATCH_CHECKER_CELL: f32 = 5.0;

// ======================================================================
// 設定コントロール
// ======================================================================
impl App {
    pub(super) fn render_trim_settings(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
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
            pad_resp.on_hover_text(
                "トリミング後に四辺へ追加する余白ピクセル数。画像の端を越える分は背景色で塗って追加する",
            );

            if self.image.original.is_some() {
                ui.add_space(if self.prefs.compact_mode { 8.0 } else { 16.0 });
                ui.separator();
                ui.add_space(if self.prefs.compact_mode { 2.0 } else { 4.0 });
                self.render_bg_color_controls(ui, ctx);
            }
        });
    }

    /// 出力設定の行 (出力形式・自動保存)
    pub(super) fn render_output_settings(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            self.render_output_format_control(ui);
            // JPEG で出力されうる形式のときだけ品質を選べるようにする
            if matches!(
                self.prefs.output_format,
                OutputFormat::Jpeg | OutputFormat::Original
            ) {
                ui.add_space(4.0);
                ui.label("JPEG 品質:");
                ui.add(
                    egui::Slider::new(
                        &mut self.prefs.jpeg_quality,
                        JPEG_QUALITY_MIN..=JPEG_QUALITY_MAX,
                    )
                    .clamping(egui::SliderClamping::Always),
                )
                .on_hover_text(
                    "JPEG で保存するときの品質 (1-100)\n\
                     大きいほど高画質でファイルサイズも大きくなる",
                );
            }
            ui.add_space(if self.prefs.compact_mode { 4.0 } else { 8.0 });
            ui.separator();
            ui.add_space(if self.prefs.compact_mode { 2.0 } else { 4.0 });
            self.render_auto_save_controls(ui, ctx);
        });
    }

    /// 保存・自動保存・一括トリミングで共通の出力形式を選ぶコンボボックス
    fn render_output_format_control(&mut self, ui: &mut egui::Ui) {
        ui.label("形式:");
        egui::ComboBox::from_id_salt("output_format")
            .selected_text(self.prefs.output_format.label())
            .show_ui(ui, |ui| {
                for format in OutputFormat::ALL {
                    ui.selectable_value(&mut self.prefs.output_format, format, format.label());
                }
            })
            .response
            .on_hover_text(
                "保存・自動保存・一括トリミングの出力形式\n\
                 「元の形式」は元画像と同じ形式 (クリップボードの画像などは PNG)",
            );
    }

    fn render_auto_save_controls(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let prev_auto_save = self.prefs.auto_save;
        ui.checkbox(&mut self.prefs.auto_save, "自動保存");
        if self.prefs.auto_save && !prev_auto_save {
            self.request_auto_save(ctx);
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
        if dir_btn.clicked() {
            self.pick_auto_save_dir(ctx);
        }
        if !self.prefs.compact_mode {
            // 長いパスは行幅に合わせて省略し、全体はホバーで表示する
            ui.add(egui::Label::new(egui::RichText::new(&full_path).weak().monospace()).truncate())
                .on_hover_text(&full_path);
        }
    }

    fn render_bg_color_controls(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.label("背景色:");
        if let Some(bg) = self.background.bg_color {
            show_bg_swatch(ui, bg);
            let mode = if self.prefs.bg_override.is_some() {
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
            .add_enabled(self.prefs.bg_override.is_some(), egui::Button::new("自動"))
            .on_hover_text("四辺から背景色を自動検出する");
        if auto_btn.clicked() {
            self.reset_bg_to_auto(ctx);
        }

        let pick_label = if self.background.pick_mode {
            "スポイト中"
        } else {
            "スポイト"
        };
        // コンパクトモードでは元画像のペインがなくクリック先がないため無効化する
        if ui
            .add_enabled(
                !self.prefs.compact_mode,
                egui::Button::selectable(self.background.pick_mode, pick_label),
            )
            .on_hover_text("元画像をクリックして背景色を取得する (Esc で解除)")
            .on_disabled_hover_text("コンパクトモードでは元画像を表示しないため使用できない")
            .clicked()
        {
            self.background.pick_mode = !self.background.pick_mode;
        }
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
        paint_checkerboard(painter, rect, SWATCH_CHECKER_CELL);
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
