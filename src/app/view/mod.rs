//! UI 描画 (`eframe::App` 実装)
//!
//! パネルの組み立てを持ち、各部品の描画をサブモジュールへ分ける

mod batch_errors;
mod input;
mod preview;
mod settings;
mod toolbar;
#[cfg(windows)]
mod update_dialog;

use eframe::egui;

use super::state::App;

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
        self.render_settings_panel(ui, &ctx);
        self.render_central_panel(ui);
        self.render_batch_error_window(&ctx);
        #[cfg(windows)]
        self.render_update_dialog(&ctx);
    }
}

impl App {
    fn render_bottom_panel(&mut self, ui: &mut egui::Ui, sp: f32) {
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
                ui.label(egui::RichText::new(status).color(ui.visuals().text_color()));
                let error_count = self.batch.last_error_details.len();
                if error_count > 0
                    && ui
                        .small_button(format!("エラー一覧 ({error_count})"))
                        .clicked()
                {
                    self.batch.errors_open = true;
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
        let sp = if self.prefs.compact_mode { 2.0 } else { 5.0 };
        egui::Panel::top("top").show(ui, |ui| {
            ui.add_space(sp);
            self.render_toolbar(ui, ctx);
            ui.add_space(sp);
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
