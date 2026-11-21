//! UI 描画 (`eframe::App` 実装)

use std::path::PathBuf;

use super::state::App;
use crate::consts::{PADDING_MAX, PADDING_MIN, TOL_MAX, TOL_MIN};
#[cfg(windows)]
use crate::image_load::format_bytes;
use crate::image_output::OutputFormat;
use crate::trim::TrimInfo;

use eframe::egui;

/// 更新ダイアログの幅
#[cfg(windows)]
const UPDATE_DIALOG_WIDTH: f32 = 320.0;

/// エラー表示色
#[cfg(windows)]
const ERROR_COLOR: egui::Color32 = egui::Color32::from_rgb(200, 80, 80);

/// 元画像プレビューで削られる領域に重ねる暗幕の色
const TRIM_SHADE_COLOR: egui::Color32 = egui::Color32::from_black_alpha(140);

/// 元画像プレビューに重ねるトリミング矩形の枠線の太さ
const TRIM_FRAME_WIDTH: f32 = 1.5;

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

/// 更新ダイアログで押されたボタン
#[cfg(windows)]
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
        handle_file_drop(self, &ctx);

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

    /// 操作ボタンの行
    ///
    /// 右端の表示切替を先に配置し、残りの幅に操作ボタンを並べる
    /// (幅が足りない場合は操作ボタンを折り返し、右端と重ならないようにする)
    fn render_action_buttons(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // 1 行分の高さの領域に右端の表示切替を置き、その左側を操作ボタンの最大幅とする
        let mut row = ui.available_rect_before_wrap();
        row.max.y = row.min.y + ui.spacing().interact_size.y;
        let mut right = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(row)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        self.render_view_toggles(&mut right, ctx);
        let right_rect = right.min_rect();

        let left_max_x = (right_rect.min.x - ui.spacing().item_spacing.x * 4.0).max(row.min.x);
        let mut left = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(row.with_max_x(left_max_x))
                .layout(egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true)),
        );
        // ボタン内の文字ではなくボタン単位で折り返す
        left.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        self.render_image_buttons(&mut left, ctx);

        ui.advance_cursor_after_rect(left.min_rect() | right_rect);
    }

    /// 画像の読み込み・保存・クリアなどの操作ボタン
    fn render_image_buttons(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if ui
            .button(egui::RichText::new("📂 開く").strong())
            .on_hover_text("画像ファイルを開く")
            .clicked()
        {
            self.open_image(ctx);
        }

        if ui
            .button("📂 一括トリミング")
            .on_hover_text("複数の画像をまとめてトリミングして保存 (出力形式の設定に従う)")
            .clicked()
            && !self.batch.running
        {
            self.batch_trim(ctx);
        }

        if self.batch.running
            && ui
                .button(
                    egui::RichText::new("⏹ キャンセル").color(egui::Color32::from_rgb(200, 80, 80)),
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

        // 子 UI で囲むと折り返しが効かないため、`add_enabled` で直接配置する
        let has_trimmed = self.image.info.is_some();
        if ui
            .add_enabled(has_trimmed, egui::Button::new("💾 保存"))
            .on_hover_text("トリミング後の画像を保存")
            .clicked()
        {
            self.save_image(ctx);
        }

        if ui
            .add_enabled(has_trimmed, egui::Button::new("📋 コピー"))
            .on_hover_text("トリミング後の画像をクリップボードへコピー")
            .clicked()
        {
            self.copy_trimmed_to_clipboard(ctx);
        }

        let has_original = self.image.original.is_some();
        let clear_color = if has_original {
            egui::Color32::from_rgb(200, 80, 80)
        } else {
            ui.visuals().weak_text_color()
        };
        if ui
            .add_enabled(
                has_original,
                egui::Button::new(egui::RichText::new("🗑 クリア").color(clear_color)),
            )
            .on_hover_text("画像をクリア")
            .clicked()
        {
            self.clear_image();
        }
    }

    /// 右端の表示切替 (テーマ・コンパクト・ヘルプ)
    ///
    /// 右から左へ並ぶレイアウトの `ui` に右端から順に配置する
    fn render_view_toggles(
        &mut self,
        ui: &mut egui::Ui,
        #[cfg_attr(not(windows), allow(unused_variables))] ctx: &egui::Context,
    ) {
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
        #[cfg(windows)]
        self.render_help_menu(ui, ctx);
    }

    /// ヘルプメニュー (バージョン・更新確認)
    #[cfg(windows)]
    fn render_help_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let mut check_clicked = false;

        ui.menu_button("⋯", |ui| {
            ui.label(format!("EdgeCut v{}", env!("CARGO_PKG_VERSION")));
            ui.separator();
            if ui.button("更新を確認…").clicked() {
                check_clicked = true;
                ui.close();
            }
        });

        if check_clicked {
            self.open_update_dialog(ctx);
        }
    }

    /// 更新ダイアログ (確認 → 結果 → インストールを一画面で行う)
    #[cfg(windows)]
    fn render_update_dialog(&mut self, ctx: &egui::Context) {
        use super::state::UpdateUiPhase;

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
    fn render_output_settings(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            self.render_output_format_control(ui);
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
            self.auto_save_if_enabled(ctx);
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
        if ui
            .selectable_label(self.background.pick_mode, pick_label)
            .on_hover_text("元画像をクリックして背景色を取得する")
            .clicked()
        {
            self.background.pick_mode = !self.background.pick_mode;
        }
    }

    fn render_central_panel(&mut self, ui: &mut egui::Ui) {
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

/// 更新ダイアログの本文をフェーズに応じて描画する
///
/// # Arguments
/// * `ui` - 描画先
/// * `phase` - 現在の更新フェーズ
///
/// # Returns
/// * `UpdateDialogAction` - 押されたボタンに対応する操作 (押されていなければ `None`)
#[cfg(windows)]
fn update_dialog_body(
    ui: &mut egui::Ui,
    phase: &super::state::UpdateUiPhase,
) -> UpdateDialogAction {
    use super::state::UpdateUiPhase;

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
            .map(|f| f.path().to_path_buf())
            .collect()
    });
    app.handle_dropped_paths(ctx, dropped);
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
