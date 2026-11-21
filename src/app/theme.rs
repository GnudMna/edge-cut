//! アプリ全体の配色・余白・角丸 (ライト / ダーク)
//!
//! egui 標準の見た目を、面の階層 (キャンバス・パネル・カード) とアクセント色で整理したものへ置き換える
//! 部品ごとの描画は `view/` に置き、ここでは共通の色と `egui::Style` だけを持つ

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Margin, Stroke, TextStyle};

/// ボタン・入力欄など小さな部品の角丸
const WIDGET_RADIUS: u8 = 6;

/// カード (設定のセクション・プレビューのペイン) の角丸
pub const CARD_RADIUS: u8 = 10;

/// ウィンドウ・メニューの角丸
const WINDOW_RADIUS: u8 = 10;

// ======================================================================
// 配色
// ======================================================================
/// テーマごとの色
///
/// 面は暗い (ライトでは濃い) 順にキャンバス → パネル → カード → 部品と重ねる
#[derive(Clone, Copy)]
pub struct Palette {
    /// プレビューの背景 (最も奥の面)
    pub canvas: Color32,
    /// ツールバー・設定パネル・ステータスバーの背景
    pub panel: Color32,
    /// パネル上に置くカードの背景
    pub card: Color32,
    /// ボタンなどの部品の背景
    widget: Color32,
    /// ホバー中の部品の背景
    widget_hovered: Color32,
    /// 押下中の部品の背景
    widget_active: Color32,
    /// 面の境界線
    pub border: Color32,
    /// 通常の文字色
    text: Color32,
    /// 強調する文字色
    text_strong: Color32,
    /// 選択状態・主要な操作を示すアクセント色
    pub accent: Color32,
    /// アクセント色の面に載せる文字色
    pub on_accent: Color32,
    /// 警告の文字色
    warn: Color32,
    /// エラー・取り消し系の操作の文字色
    pub error: Color32,
}

/// ダークテーマの配色
const DARK: Palette = Palette {
    canvas: Color32::from_rgb(17, 18, 21),
    panel: Color32::from_rgb(25, 26, 30),
    card: Color32::from_rgb(32, 33, 38),
    widget: Color32::from_rgb(42, 44, 50),
    widget_hovered: Color32::from_rgb(52, 54, 61),
    widget_active: Color32::from_rgb(62, 65, 73),
    border: Color32::from_rgb(44, 46, 52),
    text: Color32::from_rgb(200, 203, 209),
    text_strong: Color32::from_rgb(240, 241, 244),
    accent: Color32::from_rgb(92, 139, 255),
    on_accent: Color32::WHITE,
    warn: Color32::from_rgb(240, 180, 80),
    error: Color32::from_rgb(240, 110, 110),
};

/// ライトテーマの配色
const LIGHT: Palette = Palette {
    canvas: Color32::from_rgb(232, 234, 238),
    panel: Color32::from_rgb(246, 247, 249),
    card: Color32::WHITE,
    widget: Color32::from_rgb(234, 236, 240),
    widget_hovered: Color32::from_rgb(222, 225, 231),
    widget_active: Color32::from_rgb(208, 212, 220),
    border: Color32::from_rgb(218, 221, 227),
    text: Color32::from_rgb(55, 60, 68),
    text_strong: Color32::from_rgb(20, 22, 26),
    accent: Color32::from_rgb(52, 104, 235),
    on_accent: Color32::WHITE,
    warn: Color32::from_rgb(176, 110, 0),
    error: Color32::from_rgb(200, 55, 55),
};

/// テーマの配色を返す
///
/// # Arguments
/// * `dark_mode` - ダークテーマか
///
/// # Returns
/// * `Palette` - テーマの配色
pub fn palette(dark_mode: bool) -> Palette {
    if dark_mode { DARK } else { LIGHT }
}

/// 描画中の `ui` のテーマの配色を返す
///
/// # Arguments
/// * `ui` - 描画先
///
/// # Returns
/// * `Palette` - テーマの配色
pub fn palette_of(ui: &egui::Ui) -> Palette {
    palette(ui.visuals().dark_mode)
}

// ======================================================================
// スタイルの適用
// ======================================================================
/// ライト・ダーク両方のスタイルをコンテキストへ登録する
///
/// テーマの切り替えは `Context::set_theme` に任せ、ここでは起動時に一度だけ呼ぶ
///
/// # Arguments
/// * `ctx` - 登録先の egui コンテキスト
pub fn install(ctx: &egui::Context) {
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.style_mut_of(theme, |style| apply(style, theme == egui::Theme::Dark));
    }
}

/// テーマの `Visuals` を返す
///
/// # Arguments
/// * `dark_mode` - ダークテーマか
///
/// # Returns
/// * `egui::Visuals` - テーマの配色を反映した `Visuals`
pub fn visuals(dark_mode: bool) -> egui::Visuals {
    let p = palette(dark_mode);
    let mut v = if dark_mode {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };

    v.panel_fill = p.panel;
    v.window_fill = p.card;
    v.faint_bg_color = p.card;
    v.extreme_bg_color = p.canvas;
    v.code_bg_color = p.widget;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(WINDOW_RADIUS);
    v.menu_corner_radius = CornerRadius::same(WIDGET_RADIUS + 2);
    let shadow = egui::Shadow {
        offset: [0, 6],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(if dark_mode { 110 } else { 40 }),
    };
    v.window_shadow = shadow;
    v.popup_shadow = shadow;
    v.warn_fg_color = p.warn;
    v.error_fg_color = p.error;
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent;
    v.selection.stroke = Stroke::new(1.0, p.on_accent);
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Circle;

    let radius = CornerRadius::same(WIDGET_RADIUS);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.card;
    w.noninteractive.weak_bg_fill = p.card;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = radius;
    // 部品の輪郭は塗りよりわずかに明るい (ライトでは暗い) 線で示し、カード上でも埋もれないようにする
    for (visual, fill, outline, fg) in [
        (&mut w.inactive, p.widget, p.widget_hovered, p.text),
        (
            &mut w.hovered,
            p.widget_hovered,
            p.widget_active,
            p.text_strong,
        ),
        (
            &mut w.active,
            p.widget_active,
            p.widget_active,
            p.text_strong,
        ),
        (
            &mut w.open,
            p.widget_hovered,
            p.widget_active,
            p.text_strong,
        ),
    ] {
        visual.bg_fill = fill;
        visual.weak_bg_fill = fill;
        visual.bg_stroke = Stroke::new(1.0, outline);
        visual.fg_stroke = Stroke::new(1.5, fg);
        visual.corner_radius = radius;
        visual.expansion = 0.0;
    }
    v
}

/// `Style` へテーマの配色・余白・文字サイズを反映する
///
/// # Arguments
/// * `style` - 書き換える `Style`
/// * `dark_mode` - ダークテーマか
fn apply(style: &mut egui::Style, dark_mode: bool) {
    style.visuals = visuals(dark_mode);

    let s = &mut style.spacing;
    s.item_spacing = egui::vec2(8.0, 6.0);
    s.button_padding = egui::vec2(10.0, 4.0);
    s.interact_size.y = 26.0;
    s.menu_margin = Margin::same(6);
    s.window_margin = Margin::same(14);
    s.slider_rail_height = 4.0;
    s.icon_width = 16.0;
    s.icon_width_inner = 9.0;
    s.combo_height = 240.0;

    style.text_styles = [
        (
            TextStyle::Small,
            FontId::new(11.0, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(13.5, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(13.5, FontFamily::Proportional),
        ),
        (
            TextStyle::Heading,
            FontId::new(18.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(12.5, FontFamily::Monospace),
        ),
    ]
    .into();
}

// ======================================================================
// 共通の部品
// ======================================================================
/// 設定のセクションやプレビューのペインを囲むカードの枠
///
/// # Arguments
/// * `ui` - 描画先 (配色の取得に使う)
/// * `margin` - 枠の内側の余白
///
/// # Returns
/// * `egui::Frame` - カードの枠
pub fn card_frame(ui: &egui::Ui, margin: i8) -> egui::Frame {
    let p = palette_of(ui);
    egui::Frame::new()
        .fill(p.card)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(CARD_RADIUS))
        .inner_margin(Margin::same(margin))
}
