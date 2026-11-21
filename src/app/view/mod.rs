//! UI 描画 (`eframe::App` 実装)
//!
//! パネルの組み立てを持ち、各部品の描画をサブモジュールへ分ける

mod batch_errors;
mod icons;
mod input;
mod preview;
mod settings;
mod toolbar;
mod update_dialog;

use eframe::egui;

use super::state::App;
use super::theme;

// ======================================================================
// UI 描画
// ======================================================================
impl eframe::App for App {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.save_settings(storage);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        hide_window_on_close(&ctx);
        self.poll_batch_result();
        self.poll_io(&ctx);
        self.poll_trim_work(&ctx);
        self.poll_update(&ctx);
        apply_theme(&ctx, self.prefs.dark_mode);
        apply_title_bar_color(&ctx, frame, self.prefs.dark_mode);
        input::handle_file_drop(self, &ctx);
        self.handle_shortcuts(&ctx);

        let sp = if self.prefs.compact_mode { 2.0 } else { 5.0 };
        self.render_bottom_panel(ui, sp);
        self.render_top_panel(ui, &ctx);
        self.render_settings_panel(ui, &ctx);
        self.render_central_panel(ui);
        self.render_batch_error_window(&ctx);
        self.render_update_dialog(&ctx);
    }
}

impl App {
    fn render_bottom_panel(&mut self, ui: &mut egui::Ui, sp: f32) {
        let frame = panel_frame(ui, 12, 0);
        egui::Panel::bottom("bottom").frame(frame).show(ui, |ui| {
            ui.add_space(sp);
            ui.horizontal(|ui| {
                let mut status = self.status.clone();
                if self.trim_worker.is_busy() {
                    status.push_str(" (トリミング中…)");
                }
                if self.io.is_saving() {
                    status.push_str(" (保存中…)");
                }
                ui.label(
                    egui::RichText::new(status)
                        .small()
                        .color(ui.visuals().text_color()),
                );
                let error_count = self.batch.last_error_details.len();
                if error_count > 0
                    && ui
                        .small_button(
                            egui::RichText::new(format!("エラー一覧 ({error_count})"))
                                .color(ui.visuals().error_fg_color),
                        )
                        .clicked()
                {
                    self.batch.errors_open = true;
                }
                // 手動指定の解除忘れを防ぐため、指定中は常に表示する
                if let Some(c) = self.prefs.bg_override {
                    ui.separator();
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                    ui.painter().rect_filled(
                        rect,
                        3.0,
                        egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]),
                    );
                    ui.painter().rect_stroke(
                        rect,
                        3.0,
                        ui.visuals().widgets.noninteractive.bg_stroke,
                        egui::StrokeKind::Outside,
                    );
                    ui.label(
                        egui::RichText::new("背景色: 手動指定")
                            .small()
                            .color(ui.visuals().warn_fg_color),
                    )
                    .on_hover_text("設定パネルの「自動に戻す」で解除できる");
                }
                if let Some(ref info) = self.image.info {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let detail = if self.prefs.compact_mode {
                            format!(
                                "{}×{} → {}×{}",
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
                        ui.label(egui::RichText::new(detail).small().weak());
                    });
                }
            });
            ui.add_space(sp);
        });
    }

    fn render_top_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let sp = if self.prefs.compact_mode { 0.0 } else { 3.0 };
        let frame = panel_frame(ui, 10, 5);
        egui::Panel::top("top").frame(frame).show(ui, |ui| {
            ui.add_space(sp);
            self.render_toolbar(ui, ctx);
            ui.add_space(sp);
        });
    }
}

/// ツールバー・ステータスバー・設定パネルの枠
///
/// # Arguments
/// * `ui` - 描画先 (配色の取得に使う)
/// * `x` - 左右の余白
/// * `y` - 上下の余白
///
/// # Returns
/// * `egui::Frame` - パネルの枠
fn panel_frame(ui: &egui::Ui, x: i8, y: i8) -> egui::Frame {
    egui::Frame::new()
        .fill(theme::palette_of(ui).panel)
        .inner_margin(egui::Margin::symmetric(x, y))
}

// ======================================================================
// UI ヘルパー
// ======================================================================
/// ウィンドウを閉じる要求があったフレームで、ウィンドウを非表示にする
///
/// 終了時は `IoState` / `BatchState` の drop が実行中の保存の完了を待つ
/// その間イベントループが止まりウィンドウが応答なしに見えるため、待つ前に画面から消す
///
/// # Arguments
/// * `ctx` - egui のコンテキスト
fn hide_window_on_close(ctx: &egui::Context) {
    if ctx.input(|i| i.viewport().close_requested()) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
    }
}

/// アプリのテーマを egui とネイティブウィンドウ (タイトルバー) へ反映する
///
/// `set_theme` は egui の配色に加え、`ViewportCommand::SetTheme` でタイトルバーの色も切り替える
///
/// # Arguments
/// * `ctx` - egui のコンテキスト
/// * `dark_mode` - ダークテーマを使うか
fn apply_theme(ctx: &egui::Context, dark_mode: bool) {
    ctx.set_theme(if dark_mode {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
}

/// タイトルバーの色を、テーマが変わったときだけ更新する
///
/// # Arguments
/// * `ctx` - egui のコンテキスト (適用済みのテーマの記録に使う)
/// * `frame` - タイトルバーを持つウィンドウ
/// * `dark_mode` - ダークテーマか
fn apply_title_bar_color(ctx: &egui::Context, frame: &eframe::Frame, dark_mode: bool) {
    let id = egui::Id::unique("title_bar_dark_mode");
    if ctx.data(|d| d.get_temp::<bool>(id)) != Some(dark_mode) {
        super::title_bar::apply(frame, dark_mode);
        ctx.data_mut(|d| d.insert_temp(id, dark_mode));
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// 1 フレーム実行し、ウィンドウを非表示にするコマンドが送られたかを返す
    fn hidden_after_frame(close_requested: bool) -> bool {
        let ctx = egui::Context::default();
        let mut input = egui::RawInput::default();
        if close_requested {
            let mut info = egui::ViewportInfo::default();
            info.events.push(egui::ViewportEvent::Close);
            input.viewports.insert(egui::ViewportId::ROOT, info);
        }
        let mut output = ctx.run_ui(input, |ui| hide_window_on_close(ui.ctx()));
        // 描画しないため、未適用のテクスチャ差分は破棄する
        output.textures_delta.clear();
        output.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .iter()
            .any(|c| matches!(c, egui::ViewportCommand::Visible(false)))
    }

    /// 閉じる要求があったフレームでウィンドウを非表示にすること
    #[test]
    fn hide_window_on_close_hides_when_close_requested() {
        assert!(hidden_after_frame(true));
    }

    /// 閉じる要求がないフレームではウィンドウを非表示にしないこと
    #[test]
    fn hide_window_on_close_does_nothing_without_request() {
        assert!(!hidden_after_frame(false));
    }
}
