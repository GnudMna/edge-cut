//! 一括トリミングのエラー一覧ウィンドウの描画

use eframe::egui;

use crate::app::state::App;

/// エラー一覧ウィンドウの既定の幅
const ERROR_WINDOW_WIDTH: f32 = 520.0;

/// エラー一覧ウィンドウの一覧部分の最大の高さ
const ERROR_LIST_MAX_HEIGHT: f32 = 320.0;

// ======================================================================
// エラー一覧ウィンドウ
// ======================================================================
impl App {
    /// 直近の一括トリミングで失敗したファイルの一覧を表示する
    ///
    /// 件数が多くてもスクロールで読めるようにし、全文をクリップボードへコピーできる
    ///
    /// # Arguments
    /// * `ctx` - egui のコンテキスト
    pub(super) fn render_batch_error_window(&mut self, ctx: &egui::Context) {
        if !self.batch.errors_open {
            return;
        }
        if self.batch.last_error_details.is_empty() {
            self.batch.errors_open = false;
            return;
        }

        let mut open = true;
        let mut close = false;
        let errors = &self.batch.last_error_details;

        egui::Window::new("一括トリミングのエラー")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(ERROR_WINDOW_WIDTH)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(format!("{} 件のファイルが失敗した", errors.len()))
                        .strong()
                        .color(ui.visuals().error_fg_color),
                );
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .max_height(ERROR_LIST_MAX_HEIGHT)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for error in errors {
                            ui.add(egui::Label::new(error).wrap());
                            ui.separator();
                        }
                    });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("📋 すべてコピー").clicked() {
                        ctx.copy_text(errors.join("\n"));
                    }
                    if ui.button("閉じる").clicked() {
                        close = true;
                    }
                });
            });

        if !open || close {
            self.batch.errors_open = false;
        }
    }
}
