//! 一括トリミング中にウィンドウを閉じようとしたときの終了確認ダイアログ

use eframe::egui;

use crate::app::state::{App, CloseConfirm};

/// 終了確認ダイアログの幅
const CLOSE_DIALOG_WIDTH: f32 = 320.0;

// ======================================================================
// 終了確認ダイアログ
// ======================================================================
impl App {
    /// 一括トリミングの実行中に閉じる要求があったとき、終了を確認する
    pub(super) fn render_close_confirm_dialog(&mut self, ctx: &egui::Context) {
        if self.batch.close_confirm != CloseConfirm::Asking {
            return;
        }
        // 確認中に処理が完了した場合は、確認なしで閉じられるため不要
        if !self.batch.running {
            self.batch.close_confirm = CloseConfirm::None;
            return;
        }

        let mut quit = false;
        let mut keep = false;
        egui::Window::new("終了の確認")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .min_width(CLOSE_DIALOG_WIDTH)
            .show(ctx, |ui| {
                ui.set_max_width(CLOSE_DIALOG_WIDTH);
                ui.label(egui::RichText::new("一括トリミングを中止して終了しますか？").strong());
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(
                        "処理中のファイルは完了させ、未着手のファイルは処理されない",
                    )
                    .weak(),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("中止して終了").clicked() {
                        quit = true;
                    }
                    if ui.button("続ける").clicked() {
                        keep = true;
                    }
                });
            });

        if quit {
            self.batch.close_confirm = CloseConfirm::Confirmed;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if keep {
            self.batch.close_confirm = CloseConfirm::None;
        }
    }
}
