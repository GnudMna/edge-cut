//! 設定サイドパネル (トリミング・背景色・出力) の描画

use eframe::egui;

use crate::app::state::App;
use crate::consts::{
    ASPECT_PART_MAX, ASPECT_PART_MIN, JPEG_QUALITY_MAX, JPEG_QUALITY_MIN, PADDING_MAX, PADDING_MIN,
    TOL_MAX, TOL_MIN, WEBP_QUALITY_MAX, WEBP_QUALITY_MIN,
};
use crate::image_output::OutputFormat;
use crate::trim::{AspectFit, Margins};

/// 設定パネルの既定の幅
const PANEL_DEFAULT_WIDTH: f32 = 270.0;

/// 設定パネルの幅の範囲 (ドラッグで変更できる)
const PANEL_WIDTH_RANGE: std::ops::RangeInclusive<f32> = 220.0..=420.0;

/// スライダー右側の数値欄に確保する幅
const SLIDER_VALUE_WIDTH: f32 = 64.0;

/// セクション間の余白
const SECTION_SPACING: f32 = 10.0;

/// アスペクト比のプリセット (幅, 高さ)
const ASPECT_PRESETS: [(u32, u32); 7] = [(1, 1), (4, 3), (3, 2), (16, 9), (3, 4), (2, 3), (9, 16)];

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

    /// 許容度・除去する辺・余白・アスペクト比
    fn render_trim_settings(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // 値が変わったときの再実行要求。`Some(true)` はドラッグ中などでデバウンスする
        let mut request = None;

        ui.label("許容度");
        fit_slider_width(ui);
        request = trim_slider(
            ui,
            ctx,
            &mut self.prefs.tolerance,
            (TOL_MIN, TOL_MAX),
            "",
            "背景色との類似度。値が大きいほど広い範囲を余白とみなす",
        )
        .or(request);

        ui.add_space(6.0);
        request = self.render_side_controls(ui).or(request);
        ui.add_space(6.0);
        request = self.render_margin_controls(ui, ctx).or(request);
        ui.add_space(6.0);
        request = self.render_aspect_controls(ui).or(request);

        if let Some(debounce) = request {
            self.schedule_trim(ctx, debounce, false, false);
        }
    }

    /// 余白を除去する辺の選択
    fn render_side_controls(&mut self, ui: &mut egui::Ui) -> Option<bool> {
        let mut changed = false;
        ui.label("除去する辺")
            .on_hover_text("チェックを外した辺は元画像の端を維持する (その辺の余白も加えない)");
        ui.horizontal(|ui| {
            let sides = &mut self.prefs.sides;
            for (label, enabled) in [
                ("上", &mut sides.top),
                ("右", &mut sides.right),
                ("下", &mut sides.bottom),
                ("左", &mut sides.left),
            ] {
                changed |= ui.checkbox(enabled, label).changed();
            }
        });
        changed.then_some(false)
    }

    /// 余白のスライダー (四辺連動 / 辺ごと)
    fn render_margin_controls(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> Option<bool> {
        let mut request = None;
        ui.horizontal(|ui| {
            ui.label("余白");
            let resp = ui
                .checkbox(&mut self.prefs.link_margins, "四辺を連動")
                .on_hover_text("チェックを外すと、辺ごとに別々の余白を指定できる");
            if resp.changed() {
                if !self.prefs.link_margins {
                    // 連動を外した直後は、見た目が変わらないよう現在の余白を引き継ぐ
                    self.prefs.margins = Margins::uniform(self.prefs.padding);
                }
                request = Some(false);
            }
        });

        let hover = "トリミング後に残す余白ピクセル数。画像の端を越える分は背景色で塗って追加する";
        let range = (PADDING_MIN, PADDING_MAX);
        if self.prefs.link_margins {
            fit_slider_width(ui);
            return trim_slider(ui, ctx, &mut self.prefs.padding, range, " px", hover).or(request);
        }

        let sides = self.prefs.sides;
        let margins = &mut self.prefs.margins;
        for (label, value, enabled) in [
            ("上", &mut margins.top, sides.top),
            ("右", &mut margins.right, sides.right),
            ("下", &mut margins.bottom, sides.bottom),
            ("左", &mut margins.left, sides.left),
        ] {
            ui.add_enabled_ui(enabled, |ui| {
                ui.horizontal(|ui| {
                    ui.label(label);
                    fit_slider_width(ui);
                    request = trim_slider(ui, ctx, value, range, " px", hover).or(request);
                });
            })
            .response
            .on_disabled_hover_text("除去しない辺には余白を加えない");
        }
        request
    }

    /// アスペクト比の固定
    fn render_aspect_controls(&mut self, ui: &mut egui::Ui) -> Option<bool> {
        let mut request = None;
        let prefs = &mut self.prefs;
        if ui
            .checkbox(&mut prefs.aspect_enabled, "アスペクト比を固定")
            .on_hover_text(
                "余白を加えた後の範囲を、中央を基準に指定の比率へ合わせる\n\
                 結果が上限サイズを超える場合は比率を合わせない",
            )
            .changed()
        {
            request = Some(false);
        }
        if !prefs.aspect_enabled {
            return request;
        }

        ui.horizontal(|ui| {
            let range = ASPECT_PART_MIN..=ASPECT_PART_MAX;
            let w_changed = ui
                .add(egui::DragValue::new(&mut prefs.aspect_w).range(range.clone()))
                .changed();
            ui.label(":");
            let h_changed = ui
                .add(egui::DragValue::new(&mut prefs.aspect_h).range(range))
                .changed();
            if w_changed || h_changed {
                request = Some(true);
            }
            if ui
                .button("↔ 入替")
                .on_hover_text("幅と高さの比を入れ替える")
                .clicked()
            {
                std::mem::swap(&mut prefs.aspect_w, &mut prefs.aspect_h);
                request = Some(false);
            }
        });

        ui.horizontal(|ui| {
            ui.label("比率");
            let current = (prefs.aspect_w, prefs.aspect_h);
            egui::ComboBox::from_id_salt("aspect_preset")
                .selected_text(aspect_preset_label(current))
                .show_ui(ui, |ui| {
                    for preset in ASPECT_PRESETS {
                        if ui
                            .selectable_label(current == preset, aspect_label(preset))
                            .clicked()
                        {
                            (prefs.aspect_w, prefs.aspect_h) = preset;
                            request = Some(false);
                        }
                    }
                });
        });

        ui.horizontal(|ui| {
            ui.label("合わせ方");
            egui::ComboBox::from_id_salt("aspect_fit")
                .selected_text(prefs.aspect_fit.label())
                .show_ui(ui, |ui| {
                    for fit in AspectFit::ALL {
                        if ui
                            .selectable_value(&mut prefs.aspect_fit, fit, fit.label())
                            .changed()
                        {
                            request = Some(false);
                        }
                    }
                })
                .response
                .on_hover_text(
                    "「余白を足す」は内容を削らず、背景色で塗って足す\n\
                     「切り詰める」は長い方の辺を中央から削る (内容が欠ける場合がある)",
                );
        });
        request
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

    /// 出力形式・JPEG / WebP 品質・自動保存
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

        // WebP で出力されうる形式のときだけ品質を選べるようにする
        if matches!(
            self.prefs.output_format,
            OutputFormat::Webp | OutputFormat::Original
        ) {
            ui.add_space(4.0);
            fit_slider_width(ui);
            ui.label("WebP 品質");
            ui.add(
                egui::Slider::new(
                    &mut self.prefs.webp_quality,
                    WEBP_QUALITY_MIN..=WEBP_QUALITY_MAX,
                )
                .clamping(egui::SliderClamping::Always),
            )
            .on_hover_text(
                "WebP で保存するときの品質 (1-100)\n\
                 大きいほど高画質でファイルサイズも大きくなる\n\
                 100 のときだけロスレスで保存する",
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
        // 長いパスはパネル幅に合わせて省略する。省略時の全体表示は Label 標準のツールチップに任せる
        // (on_hover_text を重ねるとツールチップが2個出る)
        let full_path = self.prefs.auto_save_dir.to_string_lossy();
        ui.add(egui::Label::new(egui::RichText::new(full_path).weak().monospace()).truncate());
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
    ctx.data_mut(|d| *d.get_persisted_mut_or(egui::Id::unique(PANEL_OPEN_ID), true))
}

/// 設定パネルの開閉状態を保存する
///
/// # Arguments
/// * `ctx` - 開閉状態を保持する egui コンテキスト
/// * `open` - 開いている場合は `true`
pub(super) fn set_panel_open(ctx: &egui::Context, open: bool) {
    ctx.data_mut(|d| d.insert_persisted(egui::Id::unique(PANEL_OPEN_ID), open));
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

/// アスペクト比の表示名 (例: `16:9`)
fn aspect_label((w, h): (u32, u32)) -> String {
    format!("{w}:{h}")
}

/// 現在の比率に一致するプリセットの表示名。一致しなければ「カスタム」
fn aspect_preset_label(current: (u32, u32)) -> String {
    if ASPECT_PRESETS.contains(&current) {
        aspect_label(current)
    } else {
        "カスタム".to_owned()
    }
}

/// 値のスライダーを描画し、トリミングの再実行が必要かを返す
///
/// # Arguments
/// * `ui`     - 描画先
/// * `ctx`    - egui コンテキスト
/// * `value`  - 編集する値
/// * `range`  - 値の範囲 `(最小, 最大)`
/// * `suffix` - 値の後ろに付ける単位
/// * `hover`  - ホバー時の説明
///
/// # Returns
/// * `Option<bool>` - 再実行が必要なとき `Some(デバウンスするか)`。不要なら `None`
fn trim_slider(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    value: &mut u32,
    range: (u32, u32),
    suffix: &str,
    hover: &str,
) -> Option<bool> {
    let resp = ui.add(
        egui::Slider::new(value, range.0..=range.1)
            .suffix(suffix)
            .clamping(egui::SliderClamping::Always),
    );
    let needs_trim = slider_needs_trim(&resp, ctx, value, range.0, range.1);
    let debounce = resp.dragged() && !resp.drag_stopped();
    resp.on_hover_text(hover);
    needs_trim.then_some(debounce)
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
