//! 設定サイドパネル (トリミング・背景色・出力) の描画

use eframe::egui;

use crate::app::state::App;
use crate::consts::{
    JPEG_QUALITY_MAX, JPEG_QUALITY_MIN, PADDING_MAX, PADDING_MIN, TOL_MAX, TOL_MIN,
};
use crate::image_output::OutputFormat;

/// 設定パネルの既定の幅
const PANEL_DEFAULT_WIDTH: f32 = 270.0;

/// 設定パネルの幅の範囲 (ドラッグで変更できる)
const PANEL_WIDTH_RANGE: std::ops::RangeInclusive<f32> = 220.0..=420.0;

/// スライダー右側の数値欄に確保する幅
const SLIDER_VALUE_WIDTH: f32 = 64.0;

/// セクション間の余白
const SECTION_SPACING: f32 = 10.0;

// ======================================================================
// 設定パネル
// ======================================================================
impl App {
    /// 右側の設定パネル
    ///
    /// ツールバーの「設定」で開閉し、境界のドラッグで幅を変えられる
    pub(super) fn render_settings_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let mut open = is_panel_open(ctx);
        egui::Panel::right("settings")
            .resizable(true)
            .default_size(PANEL_DEFAULT_WIDTH)
            .size_range(PANEL_WIDTH_RANGE)
            .show_collapsible(ui, &mut open, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add_space(6.0);
                        section_heading(ui, "トリミング");
                        self.render_trim_settings(ui, ctx);
                        ui.add_space(SECTION_SPACING);
                        ui.separator();
                        section_heading(ui, "背景色");
                        self.render_bg_color_controls(ui, ctx);
                        ui.add_space(SECTION_SPACING);
                        ui.separator();
                        section_heading(ui, "出力");
                        self.render_output_settings(ui, ctx);
                        ui.add_space(6.0);
                    });
            });
        // 境界のドラッグで閉じた場合も開閉状態へ反映する
        set_panel_open(ctx, open);
    }

    /// 許容度・余白のスライダー
    fn render_trim_settings(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        fit_slider_width(ui);

        ui.label("許容度");
        let tol_resp = ui.add(
            egui::Slider::new(&mut self.prefs.tolerance, TOL_MIN..=TOL_MAX)
                .clamping(egui::SliderClamping::Always),
        );
        if slider_needs_trim(&tol_resp, ctx, &mut self.prefs.tolerance, TOL_MIN, TOL_MAX) {
            let debounce = tol_resp.dragged() && !tol_resp.drag_stopped();
            self.schedule_trim(ctx, debounce, false, false);
        }
        tol_resp.on_hover_text("背景色との類似度。値が大きいほど広い範囲を余白とみなす");

        ui.add_space(4.0);
        ui.label("余白");
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
    }

    /// 背景色の表示と、手動指定・スポイト・自動検出への切替
    fn render_bg_color_controls(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // 背景色は読み込んだ画像から検出するため、画像がない間は操作を出さない
        let Some(bg) = self
            .background
            .bg_color
            .filter(|_| self.image.original.is_some())
        else {
            ui.label(egui::RichText::new("画像を開くと四辺から自動検出する").weak());
            return;
        };

        ui.horizontal(|ui| {
            let mut edit_color = bg;
            if ui
                .color_edit_button_srgba_unmultiplied(&mut edit_color)
                .on_hover_text("クリックして背景色を直接編集")
                .changed()
            {
                self.set_manual_bg(ctx, edit_color);
            }
            let mode = if self.prefs.bg_override.is_some() {
                "手動指定"
            } else {
                "自動検出"
            };
            ui.label(mode);
        });
        ui.label(
            egui::RichText::new(format!("RGBA({}, {}, {}, {})", bg[0], bg[1], bg[2], bg[3]))
                .monospace()
                .weak(),
        );

        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            let pick_label = if self.background.pick_mode {
                "💧 スポイト中"
            } else {
                "💧 スポイト"
            };
            // コンパクト表示では元画像のペインがなくクリック先がないため無効化する
            if ui
                .add_enabled(
                    !self.prefs.compact_mode,
                    egui::Button::selectable(self.background.pick_mode, pick_label),
                )
                .on_hover_text("元画像をクリックして背景色を取得する (Esc で解除)")
                .on_disabled_hover_text("コンパクト表示では元画像を表示しないため使用できない")
                .clicked()
            {
                self.background.pick_mode = !self.background.pick_mode;
            }

            if ui
                .add_enabled(
                    self.prefs.bg_override.is_some(),
                    egui::Button::new("↺ 自動に戻す"),
                )
                .on_hover_text("手動指定を解除し、四辺から背景色を自動検出する")
                .clicked()
            {
                self.reset_bg_to_auto(ctx);
            }
        });
    }

    /// 出力形式・JPEG 品質・自動保存
    fn render_output_settings(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.label("形式");
            self.render_output_format_control(ui);
        });

        // JPEG で出力されうる形式のときだけ品質を選べるようにする
        if matches!(
            self.prefs.output_format,
            OutputFormat::Jpeg | OutputFormat::Original
        ) {
            ui.add_space(4.0);
            fit_slider_width(ui);
            ui.label("JPEG 品質");
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

        ui.add_space(8.0);
        self.render_auto_save_controls(ui, ctx);
    }

    /// 保存・自動保存・一括トリミングで共通の出力形式を選ぶコンボボックス
    fn render_output_format_control(&mut self, ui: &mut egui::Ui) {
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
        ui.checkbox(&mut self.prefs.auto_save, "自動保存")
            .on_hover_text("画像を開いた直後にトリミング結果を保存先フォルダへ保存する");
        if self.prefs.auto_save && !prev_auto_save {
            self.request_auto_save(ctx);
        }

        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.label("保存先");
            if ui
                .button("📁 変更…")
                .on_hover_text("自動保存の保存先フォルダを変更")
                .clicked()
            {
                self.pick_auto_save_dir(ctx);
            }
        });
        // 長いパスはパネル幅に合わせて省略し、全体はホバーで表示する
        let full_path = self.prefs.auto_save_dir.to_string_lossy().into_owned();
        ui.add(egui::Label::new(egui::RichText::new(&full_path).weak().monospace()).truncate())
            .on_hover_text(&full_path);
    }
}

/// 設定パネルの開閉状態を保持する egui メモリのキー
///
/// egui のメモリは eframe が終了時に永続化するため、次回起動時も開閉状態を引き継ぐ
const PANEL_OPEN_ID: &str = "settings_panel_open";

/// 設定パネルが開いているかどうかを返す (未保存のときは開いている扱い)
///
/// # Arguments
/// * `ctx` - 開閉状態を保持する egui コンテキスト
///
/// # Returns
/// * `bool` - 開いている場合は `true`
pub(super) fn is_panel_open(ctx: &egui::Context) -> bool {
    ctx.data_mut(|d| *d.get_persisted_mut_or(egui::Id::new(PANEL_OPEN_ID), true))
}

/// 設定パネルの開閉状態を保存する
///
/// # Arguments
/// * `ctx` - 開閉状態を保持する egui コンテキスト
/// * `open` - 開いている場合は `true`
pub(super) fn set_panel_open(ctx: &egui::Context, open: bool) {
    ctx.data_mut(|d| d.insert_persisted(egui::Id::new(PANEL_OPEN_ID), open));
}

/// セクションの見出し
fn section_heading(ui: &mut egui::Ui, title: &str) {
    ui.label(egui::RichText::new(title).strong().size(15.0));
    ui.add_space(4.0);
}

/// スライダーの幅を数値欄を除いたパネル幅に合わせる
fn fit_slider_width(ui: &mut egui::Ui) {
    let width = ui.available_width() - SLIDER_VALUE_WIDTH - ui.spacing().item_spacing.x;
    ui.spacing_mut().slider_width = width.max(80.0);
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
