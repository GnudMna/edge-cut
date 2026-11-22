//! 設定パネル下部の結果表示 (トリミング後のサイズ) と保存・コピーのボタン

use eframe::egui;

use super::icons::{self, Icon};
use crate::app::shortcuts;
use crate::app::state::App;
use crate::trim::TrimInfo;

/// 結果のサイズを表示する文字の大きさ
const RESULT_SIZE_TEXT: f32 = 17.0;

/// 下部の保存・コピーのボタンの高さ
const WIDE_BUTTON_HEIGHT: f32 = 32.0;

/// 下部の行で保存ボタンが占める幅の割合 (残りをコピーに充てる)
const SAVE_BUTTON_RATIO: f32 = 0.6;

// ======================================================================
// 結果と出力の操作
// ======================================================================
impl App {
    /// 設定パネル下部の結果のサイズと保存・コピー
    ///
    /// 設定をスクロールしても常に見えるよう、パネルの下端に固定して描画する
    pub(super) fn render_result_footer(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        match self.image.info.filter(|_| self.image.original.is_some()) {
            Some(info) => self.render_result_summary(ui, &info),
            None => {
                ui.label(egui::RichText::new("画像を開くとトリミング結果を表示する").weak());
            }
        }
        ui.add_space(8.0);
        self.render_output_buttons(ui, ctx, true);
    }

    /// トリミング後のサイズ・面積の増減・元画像上の位置
    fn render_result_summary(&self, ui: &mut egui::Ui, info: &TrimInfo) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("{} × {}", info.trim_w, info.trim_h))
                    .size(RESULT_SIZE_TEXT)
                    .strong(),
            );
            ui.label(egui::RichText::new("px").weak());
            // 設定変更後の再計算中は、表示中の値が古いことを示す
            if !self.has_current_trim() {
                ui.spinner();
            }
            let change =
                area_change_percent([info.orig_w, info.orig_h], [info.trim_w, info.trim_h]);
            if let Some(change) = change {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(format_change(change)).weak())
                        .on_hover_text("元画像に対する面積の増減");
                });
            }
        });
        ui.label(
            egui::RichText::new(format!(
                "元 {} × {}  ·  位置 x={} y={}",
                info.orig_w, info.orig_h, info.offset_x, info.offset_y,
            ))
            .small()
            .weak(),
        );
    }

    /// 保存・コピーのボタン
    ///
    /// 再計算の待ち中・処理中は前回の矩形が残っているため無効にする
    ///
    /// # Arguments
    /// * `ui` - 描画先
    /// * `ctx` - egui コンテキスト
    /// * `wide` - `true` のとき行の幅いっぱいに大きく並べる (設定パネル下部)。
    ///   `false` のときは通常の大きさで並べる (ツールバー)
    pub(super) fn render_output_buttons(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        wide: bool,
    ) {
        let enabled = self.has_current_trim();
        let (save_size, copy_size) = if wide {
            let width = ui.available_width() - ui.spacing().item_spacing.x;
            let save_w = (width * SAVE_BUTTON_RATIO).floor();
            (
                egui::vec2(save_w, WIDE_BUTTON_HEIGHT),
                egui::vec2(width - save_w, WIDE_BUTTON_HEIGHT),
            )
        } else {
            (egui::Vec2::ZERO, egui::Vec2::ZERO)
        };
        let mut row = |ui: &mut egui::Ui| {
            if ui
                .add_enabled(
                    enabled,
                    icons::primary_button(ui, Icon::Save, "保存").min_size(save_size),
                )
                .on_hover_text(shortcuts::with_shortcut(
                    ctx,
                    "トリミング後の画像を保存",
                    shortcuts::SAVE,
                ))
                .clicked()
            {
                self.save_image(ctx);
            }
            if ui
                .add_enabled(
                    enabled,
                    icons::button(Icon::Copy, "コピー").min_size(copy_size),
                )
                .on_hover_text(shortcuts::with_shortcut(
                    ctx,
                    "トリミング後の画像をクリップボードへコピー",
                    shortcuts::COPY,
                ))
                .clicked()
            {
                self.copy_trimmed_to_clipboard(ctx);
            }
        };
        if wide {
            ui.horizontal(row);
        } else {
            // ツールバーの右から左へ並ぶ行にそのまま置く
            row(ui);
        }
    }
}

/// 元画像に対するトリミング後の面積の増減を百分率で返す
///
/// 余白を足す設定では元画像より大きくなるため、正の値にもなる
///
/// # Arguments
/// * `orig` - 元画像の寸法
/// * `trim` - トリミング後の寸法
///
/// # Returns
/// * `Option<i64>` - 四捨五入した増減 (%)。元画像の面積が 0 のときは `None`
fn area_change_percent(orig: [u32; 2], trim: [u32; 2]) -> Option<i64> {
    let orig_area = u128::from(orig[0]) * u128::from(orig[1]);
    if orig_area == 0 {
        return None;
    }
    let trim_area = u128::from(trim[0]) * u128::from(trim[1]);
    // 比率 (%) を四捨五入してから 100 を引く
    let ratio = (trim_area * 200 + orig_area) / (orig_area * 2);
    i64::try_from(ratio).ok().map(|r| r - 100)
}

/// 面積の増減の表示 (例: `面積 −81%`)
fn format_change(change: i64) -> String {
    match change {
        0 => "面積 ±0%".to_owned(),
        c if c < 0 => format!("面積 −{}%", c.unsigned_abs()),
        c => format!("面積 +{c}%"),
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// 縮小・拡大・変化なしの面積の増減を四捨五入した百分率で返すこと
    #[test]
    fn area_change_percent_rounds_ratio() {
        assert_eq!(area_change_percent([900, 600], [394, 263]), Some(-81));
        assert_eq!(area_change_percent([100, 100], [110, 110]), Some(21));
        assert_eq!(area_change_percent([100, 100], [100, 100]), Some(0));
        // 0.5% は切り上げる
        assert_eq!(area_change_percent([200, 1], [1, 1]), Some(-99));
    }

    /// 元画像の面積が 0 のときは増減を返さないこと
    #[test]
    fn area_change_percent_none_for_empty_original() {
        assert_eq!(area_change_percent([0, 10], [5, 5]), None);
    }

    /// 減少は全角のマイナス記号、増加は `+` を付けること
    #[test]
    fn format_change_signs() {
        assert_eq!(format_change(-81), "面積 −81%");
        assert_eq!(format_change(21), "面積 +21%");
        assert_eq!(format_change(0), "面積 ±0%");
    }
}
