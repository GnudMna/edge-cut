//! UI 描画 (`eframe::App` 実装)
//!
//! パネルの組み立てを持ち、各部品の描画をサブモジュールへ分ける

mod input;
mod preview;
mod settings;
mod toolbar;
#[cfg(windows)]
mod update_dialog;

use super::state::App;

use eframe::egui;

/// エラー・取り消し系の操作を示す色
const ERROR_COLOR: egui::Color32 = egui::Color32::from_rgb(200, 80, 80);

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
        self.poll_io(&ctx);
        self.poll_trim_work(&ctx);
        #[cfg(windows)]
        self.poll_update(&ctx);
        apply_theme(&ctx, self.prefs.dark_mode);
        input::handle_file_drop(self, &ctx);
        self.handle_shortcuts(&ctx);

        let sp = if self.prefs.compact_mode { 1.0 } else { 3.0 };
        self.render_bottom_panel(ui, sp);
        self.render_top_panel(ui, &ctx);
        self.render_central_panel(ui);
        #[cfg(windows)]
        self.render_update_dialog(&ctx);
    }
}

impl App {
    fn render_bottom_panel(&self, ui: &mut egui::Ui, sp: f32) {
        egui::Panel::bottom("bottom").show(ui, |ui| {
            ui.add_space(sp);
            ui.horizontal(|ui| {
                let mut status = self.status.clone();
                if self.trim_worker.is_busy() {
                    status.push_str(" (トリミング中…)");
                }
                if self.io.is_saving() {
                    status.push_str(" (保存中…)");
                }
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
        egui::Panel::top("top").show(ui, |ui| {
            ui.add_space(if self.prefs.compact_mode { 2.0 } else { 6.0 });
            self.render_action_buttons(ui, ctx);
            ui.add_space(if self.prefs.compact_mode { 1.0 } else { 3.0 });
            self.render_output_settings(ui, ctx);

            if !self.prefs.compact_mode {
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(2.0);
            }

            self.render_trim_settings(ui, ctx);
            ui.add_space(if self.prefs.compact_mode { 2.0 } else { 6.0 });
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
