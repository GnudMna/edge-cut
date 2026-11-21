//! ボタンに添える線画アイコンとアイコン付きボタン
//!
//! カラー絵文字はテーマの文字色に揃わないため、16 単位の格子に線で描いたアイコンを使う
//! アイコンは隣の文字と同じ色で描き、無効化・エラー色にも追従する

use eframe::egui::{self, Color32, Pos2, Stroke};

use crate::app::theme;

/// ボタン内のアイコンの一辺の長さ
const ICON_SIZE: f32 = 16.0;

/// アイコンの線の太さ (16 単位の格子に対する値)
const ICON_STROKE: f32 = 1.4;

// ======================================================================
// アイコン
// ======================================================================
/// 線画アイコンの種類
#[derive(Clone, Copy)]
pub enum Icon {
    /// フォルダ (開く・保存先の変更)
    Folder,
    /// クリップボード (貼り付け)
    Paste,
    /// 重なった画像 (一括トリミング)
    Batch,
    /// 停止 (キャンセル)
    Stop,
    /// 下向き矢印とトレイ (保存)
    Save,
    /// 重なった四角 (コピー)
    Copy,
    /// ごみ箱 (クリア)
    Trash,
    /// スライダー (設定)
    Settings,
    /// 三本線 (メニュー)
    Menu,
    /// スポイト (背景色の取得)
    Picker,
    /// 回転矢印 (自動に戻す)
    Reset,
    /// 左右の矢印 (入れ替え)
    Swap,
    /// 山と太陽の画像 (画像の読み込み待ち)
    Image,
}

impl Icon {
    /// ボタンの文字の横に置くアトム
    ///
    /// # Arguments
    /// * `self` - 描くアイコン
    /// * `color` - 線の色。`None` のときはボタンの文字色に合わせる
    ///
    /// # Returns
    /// * `egui::Atom<'static>` - アイコンを描くアトム
    pub fn atom(self, color: Option<Color32>) -> egui::Atom<'static> {
        egui::Atom::paint(egui::Vec2::splat(ICON_SIZE), move |ui, args| {
            self.paint(
                ui.painter(),
                args.rect,
                color.unwrap_or(args.fallback_text_color),
            );
        })
    }

    /// `rect` に収まるようアイコンを描く
    ///
    /// # Arguments
    /// * `self` - 描くアイコン
    /// * `painter` - 描画先
    /// * `rect` - アイコンを収める正方形の領域
    /// * `color` - 線の色
    pub fn paint(self, painter: &egui::Painter, rect: egui::Rect, color: Color32) {
        let unit = rect.width().min(rect.height()) / 16.0;
        let pen = Pen {
            painter,
            origin: rect.center() - egui::Vec2::splat(8.0 * unit),
            unit,
            stroke: Stroke::new(ICON_STROKE * unit.max(1.0), color),
        };

        match self {
            Self::Folder => {
                let outline = [(1.5, 3.5), (6.0, 3.5), (7.5, 5.0), (14.5, 5.0)];
                pen.closed(&[outline.as_slice(), &[(14.5, 13.0), (1.5, 13.0)]].concat());
                pen.line(&[(1.5, 7.0), (14.5, 7.0)]);
            }
            Self::Paste => {
                pen.rect(3.0, 3.0, 13.0, 14.5, 1.5);
                pen.rect(5.5, 1.5, 10.5, 4.5, 1.0);
                pen.line(&[(5.5, 8.0), (10.5, 8.0)]);
                pen.line(&[(5.5, 11.0), (9.0, 11.0)]);
            }
            Self::Batch => {
                pen.rect(4.5, 4.5, 14.5, 12.5, 1.5);
                pen.line(&[(1.5, 10.0), (1.5, 2.0), (11.5, 2.0)]);
                pen.line(&[(6.5, 11.0), (9.0, 8.0), (10.5, 9.5)]);
                pen.line(&[(10.5, 9.5), (11.5, 8.5), (13.0, 11.0)]);
            }
            Self::Stop => pen.filled_rect(3.5, 3.5, 12.5, 12.5, 1.5),
            Self::Save => {
                pen.line(&[(8.0, 1.5), (8.0, 10.0)]);
                pen.line(&[(4.5, 6.5), (8.0, 10.0), (11.5, 6.5)]);
                pen.line(&[(2.0, 10.5), (2.0, 14.0), (14.0, 14.0), (14.0, 10.5)]);
            }
            Self::Copy => {
                pen.rect(5.5, 5.5, 14.0, 14.0, 1.5);
                pen.line(&[(10.5, 3.5), (10.5, 2.0), (2.0, 2.0)]);
                pen.line(&[(2.0, 2.0), (2.0, 10.5), (3.5, 10.5)]);
            }
            Self::Trash => {
                pen.line(&[(1.5, 4.0), (14.5, 4.0)]);
                pen.line(&[(6.0, 4.0), (6.0, 2.0), (10.0, 2.0), (10.0, 4.0)]);
                pen.line(&[(3.0, 4.0), (4.0, 14.5), (12.0, 14.5), (13.0, 4.0)]);
                pen.line(&[(6.5, 7.0), (6.5, 11.5)]);
                pen.line(&[(9.5, 7.0), (9.5, 11.5)]);
            }
            Self::Settings => {
                for (y, knob) in [(3.5, 10.5), (8.0, 5.0), (12.5, 9.0)] {
                    pen.line(&[(1.5, y), (knob - 2.0, y)]);
                    pen.line(&[(knob + 2.0, y), (14.5, y)]);
                    pen.circle(knob, y, 2.0);
                }
            }
            Self::Menu => {
                for y in [4.0, 8.0, 12.0] {
                    pen.line(&[(2.5, y), (13.5, y)]);
                }
            }
            Self::Picker => {
                pen.line(&[(9.0, 4.0), (12.0, 7.0)]);
                pen.line(&[(10.5, 5.5), (3.5, 12.5), (2.0, 14.0)]);
                pen.line(&[(9.5, 3.5), (11.5, 1.5), (14.5, 4.5), (12.5, 6.5)]);
            }
            Self::Reset => {
                pen.arc((8.0, 8.5), 5.5, 0.75);
                pen.line(&[(5.5, 1.0), (8.0, 3.0), (5.5, 5.0)]);
            }
            Self::Swap => {
                pen.line(&[(2.0, 5.5), (14.0, 5.5)]);
                pen.line(&[(11.0, 2.5), (14.0, 5.5), (11.0, 8.5)]);
                pen.line(&[(14.0, 10.5), (2.0, 10.5)]);
                pen.line(&[(5.0, 7.5), (2.0, 10.5), (5.0, 13.5)]);
            }
            Self::Image => {
                pen.rect(1.5, 2.5, 14.5, 13.5, 2.0);
                pen.circle(10.5, 6.0, 1.5);
                pen.line(&[(1.5, 12.0), (6.0, 7.5), (10.5, 12.0)]);
                pen.line(&[(9.0, 10.5), (11.0, 8.5), (14.5, 12.0)]);
            }
        }
    }
}

/// アイコンの格子 (16 単位) の座標で線を引く描画先
struct Pen<'a> {
    /// 描画先
    painter: &'a egui::Painter,
    /// 格子の原点 (左上) の画面座標
    origin: Pos2,
    /// 格子 1 単位あたりの画面 px 数
    unit: f32,
    /// 線の太さと色
    stroke: Stroke,
}

impl Pen<'_> {
    /// 格子の座標を画面座標へ変換する
    fn pos(&self, (x, y): (f32, f32)) -> Pos2 {
        self.origin + egui::vec2(x, y) * self.unit
    }

    /// 点を順に結ぶ折れ線
    fn line(&self, points: &[(f32, f32)]) {
        let points = points.iter().map(|&p| self.pos(p)).collect();
        self.painter.line(points, self.stroke);
    }

    /// 点を順に結び、始点へ戻る閉じた折れ線
    fn closed(&self, points: &[(f32, f32)]) {
        let points = points.iter().map(|&p| self.pos(p)).collect();
        self.painter
            .add(egui::Shape::closed_line(points, self.stroke));
    }

    /// 角丸の四角形の輪郭
    fn rect(&self, x0: f32, y0: f32, x1: f32, y1: f32, radius: f32) {
        let rect = egui::Rect::from_min_max(self.pos((x0, y0)), self.pos((x1, y1)));
        self.painter.rect_stroke(
            rect,
            radius * self.unit,
            self.stroke,
            egui::StrokeKind::Middle,
        );
    }

    /// 塗りつぶした角丸の四角形
    fn filled_rect(&self, x0: f32, y0: f32, x1: f32, y1: f32, radius: f32) {
        let rect = egui::Rect::from_min_max(self.pos((x0, y0)), self.pos((x1, y1)));
        self.painter
            .rect_filled(rect, radius * self.unit, self.stroke.color);
    }

    /// 円の輪郭
    fn circle(&self, x: f32, y: f32, radius: f32) {
        self.painter
            .circle_stroke(self.pos((x, y)), radius * self.unit, self.stroke);
    }

    /// 上端から反時計回りに `turns` 周分の円弧
    #[allow(clippy::cast_precision_loss)]
    fn arc(&self, center: (f32, f32), radius: f32, turns: f32) {
        const SEGMENTS: usize = 24;
        let center = self.pos(center);
        let points = (0..=SEGMENTS)
            .map(|i| {
                let t = -std::f32::consts::FRAC_PI_2
                    - std::f32::consts::TAU * turns * (i as f32 / SEGMENTS as f32);
                center + radius * self.unit * egui::vec2(t.cos(), t.sin())
            })
            .collect();
        self.painter.line(points, self.stroke);
    }
}

// ======================================================================
// アイコン付きボタン
// ======================================================================
/// アイコンと文字を並べた通常のボタン
///
/// # Arguments
/// * `icon` - 文字の左に置くアイコン
/// * `text` - ボタンの文字
///
/// # Returns
/// * `egui::Button<'a>` - ボタン
pub fn button<'a>(icon: Icon, text: impl Into<egui::WidgetText>) -> egui::Button<'a> {
    egui::Button::new((icon.atom(None), text.into()))
}

/// 主要な操作のボタン (アクセント色で塗る)
///
/// # Arguments
/// * `ui` - 描画先 (配色の取得に使う)
/// * `icon` - 文字の左に置くアイコン
/// * `text` - ボタンの文字
///
/// # Returns
/// * `egui::Button<'a>` - アクセント色のボタン
pub fn primary_button<'a>(ui: &egui::Ui, icon: Icon, text: &str) -> egui::Button<'a> {
    let p = theme::palette_of(ui);
    egui::Button::new((
        icon.atom(Some(p.on_accent)),
        egui::RichText::new(text).color(p.on_accent),
    ))
    .fill(p.accent)
}
