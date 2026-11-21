//! 更新ダイアログの描画 (Windows)

use super::ERROR_COLOR;
use crate::app::state::{App, UpdateUiPhase};
use crate::image_load::format_bytes;

use eframe::egui;

/// 更新ダイアログの幅
const UPDATE_DIALOG_WIDTH: f32 = 320.0;

/// 更新ダイアログで押されたボタン
enum UpdateDialogAction {
    /// 操作なし
    None,
    /// 更新確認を (再) 実行する
    Check,
    /// 更新をダウンロードしてインストーラーを起動する
    Apply,
    /// ダイアログを閉じる
    Close,
}

// ======================================================================
// 更新ダイアログ
// ======================================================================
impl App {
    /// 更新ダイアログ (確認 → 結果 → インストールを一画面で行う)
    pub(super) fn render_update_dialog(&mut self, ctx: &egui::Context) {
        if !self.update.dialog_open {
            return;
        }

        let mut open = true;
        let mut action = UpdateDialogAction::None;
        // ダウンロード中はアプリ終了まで進むため閉じるボタンを出さない
        let closable = !matches!(self.update.phase, UpdateUiPhase::Downloading { .. });

        let mut window = egui::Window::new("アップデート")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .min_width(UPDATE_DIALOG_WIDTH);
        if closable {
            window = window.open(&mut open);
        }

        window.show(ctx, |ui| {
            ui.set_max_width(UPDATE_DIALOG_WIDTH);
            action = update_dialog_body(ui, &self.update.phase);
        });

        match action {
            UpdateDialogAction::None => {}
            UpdateDialogAction::Check => self.start_update_check(ctx),
            UpdateDialogAction::Apply => self.start_update_apply(ctx),
            UpdateDialogAction::Close => open = false,
        }
        if !open {
            self.update.dialog_open = false;
        }
    }
}

/// 更新ダイアログの本文をフェーズに応じて描画する
///
/// # Arguments
/// * `ui` - 描画先
/// * `phase` - 現在の更新フェーズ
///
/// # Returns
/// * `UpdateDialogAction` - 押されたボタンに対応する操作 (押されていなければ `None`)
fn update_dialog_body(ui: &mut egui::Ui, phase: &UpdateUiPhase) -> UpdateDialogAction {
    let mut action = UpdateDialogAction::None;
    match phase {
        UpdateUiPhase::Idle | UpdateUiPhase::Checking => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("更新を確認しています…");
            });
        }
        UpdateUiPhase::UpToDate { current_version } => {
            ui.label(egui::RichText::new("✔ 最新版を使用しています").strong());
            ui.add_space(4.0);
            ui.label(format!("現在のバージョン: v{current_version}"));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("閉じる").clicked() {
                    action = UpdateDialogAction::Close;
                }
                if ui.button("再確認").clicked() {
                    action = UpdateDialogAction::Check;
                }
            });
        }
        UpdateUiPhase::Available(update) => {
            ui.label(egui::RichText::new("新しいバージョンがあります").strong());
            ui.add_space(4.0);
            egui::Grid::new("update_info")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.label("現在:");
                    ui.label(format!("v{}", update.current_version));
                    ui.end_row();
                    ui.label("最新:");
                    ui.label(egui::RichText::new(format!("v{}", update.latest_version)).strong());
                    ui.end_row();
                    ui.label("サイズ:");
                    ui.label(format_bytes(update.size));
                    ui.end_row();
                });
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(
                    "インストーラーの起動後、EdgeCut は終了します。\n\
                     未保存の画像は先に保存してください",
                )
                .weak(),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .button(egui::RichText::new("⬇ ダウンロードしてインストール").strong())
                    .clicked()
                {
                    action = UpdateDialogAction::Apply;
                }
                if ui.button("後で").clicked() {
                    action = UpdateDialogAction::Close;
                }
            });
        }
        UpdateUiPhase::Downloading { update } => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!(
                    "v{} をダウンロード・検証しています… ({})",
                    update.latest_version,
                    format_bytes(update.size)
                ));
            });
            ui.add_space(4.0);
            ui.label(egui::RichText::new("完了するとインストーラーが起動します").weak());
        }
        UpdateUiPhase::Error { message } => {
            ui.label(
                egui::RichText::new("更新に失敗しました")
                    .strong()
                    .color(ERROR_COLOR),
            );
            ui.add_space(4.0);
            ui.label(egui::RichText::new(message).color(ERROR_COLOR));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("再試行").clicked() {
                    action = UpdateDialogAction::Check;
                }
                if ui.button("閉じる").clicked() {
                    action = UpdateDialogAction::Close;
                }
            });
        }
        UpdateUiPhase::MsiStarted => {
            ui.label("インストーラーを起動しました。EdgeCut を終了します");
        }
    }
    action
}
