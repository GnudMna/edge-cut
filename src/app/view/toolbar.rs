//! ツールバー (画像の取り込み・設定パネルの開閉・表示メニュー) の描画

use eframe::egui;

use super::icons::{self, Icon};
use super::settings;
use crate::app::bundled::Bundled;
use crate::app::shortcuts;
use crate::app::state::App;

// ======================================================================
// ツールバー
// ======================================================================
impl App {
    /// ツールバーの行
    ///
    /// 右端の設定パネル切替・メニューを先に配置し、残りの幅に取り込みのボタンを並べる
    /// (幅が足りない場合はボタンを折り返し、右端と重ならないようにする)
    /// 保存・コピーは設定パネル下部に置き、パネルを閉じている間だけ右端に出す
    pub(super) fn render_toolbar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // 1 行分の高さの領域に右端のボタンを置き、その左側を操作ボタンの最大幅とする
        let mut row = ui.available_rect_before_wrap();
        row.max.y = row.min.y + ui.spacing().interact_size.y;
        let mut right = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(row)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        self.render_view_controls(&mut right, ctx);
        let right_rect = right.min_rect();

        let left_max_x = (right_rect.min.x - ui.spacing().item_spacing.x * 4.0).max(row.min.x);
        let mut left = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(row.with_max_x(left_max_x))
                .layout(egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true)),
        );
        // ボタン内の文字ではなくボタン単位で折り返す
        left.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        self.render_input_buttons(&mut left, ctx);

        ui.advance_cursor_after_rect(left.min_rect() | right_rect);
    }

    /// 画像を取り込む操作 (開く・貼り付け・一括トリミング)
    fn render_input_buttons(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if ui
            .add(icons::button(Icon::Folder, "開く"))
            .on_hover_text(shortcuts::with_shortcut(
                ctx,
                "画像ファイルを開く",
                shortcuts::OPEN,
            ))
            .clicked()
        {
            self.open_image(ctx);
        }

        if ui
            .add(icons::button(Icon::Paste, "貼り付け"))
            .on_hover_text(shortcuts::with_shortcut(
                ctx,
                "クリップボードの画像を使用",
                shortcuts::PASTE,
            ))
            .clicked()
        {
            self.paste_from_clipboard(ctx);
        }

        // 処理中は同じ位置をキャンセルボタンに差し替える
        if self.batch.running {
            if ui
                .add(icons::button(
                    Icon::Stop,
                    egui::RichText::new("キャンセル").color(ui.visuals().error_fg_color),
                ))
                .on_hover_text(shortcuts::with_shortcut(
                    ctx,
                    "一括トリミングを中止",
                    shortcuts::CANCEL,
                ))
                .clicked()
            {
                self.cancel_batch();
            }
        } else if ui
            .add(icons::button(Icon::Batch, "一括トリミング…"))
            .on_hover_text(shortcuts::with_shortcut(
                ctx,
                "複数の画像をまとめてトリミングして保存 (出力形式の設定に従う)",
                shortcuts::BATCH_TRIM,
            ))
            .clicked()
        {
            self.batch_trim(ctx);
        }
    }

    /// 右端の表示メニューと設定パネルの開閉ボタン
    ///
    /// 右から左へ並ぶレイアウトの `ui` に右端から順に配置する
    fn render_view_controls(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let mut check_update = false;
        let mut open_bundled = None;
        ui.menu_button(Icon::Menu.atom(None), |ui| {
            let theme_label = if self.prefs.dark_mode {
                "☀ ライトテーマ"
            } else {
                "🌙 ダークテーマ"
            };
            if ui.button(theme_label).clicked() {
                self.prefs.dark_mode = !self.prefs.dark_mode;
                ui.close();
            }
            ui.separator();
            for item in Bundled::ALL {
                // 配布物の外 (cargo run など) では文書が無いため無効にする
                if ui
                    .add_enabled(item.locate().is_some(), egui::Button::new(item.label()))
                    .on_hover_text(item.description())
                    .on_disabled_hover_text("配布物に同梱されたファイルが見つからない")
                    .clicked()
                {
                    open_bundled = Some(item);
                    ui.close();
                }
            }
            ui.separator();
            ui.label(egui::RichText::new(format!("EdgeCut v{}", env!("CARGO_PKG_VERSION"))).weak());
            if ui.button("更新を確認…").clicked() {
                check_update = true;
                ui.close();
            }
        })
        .response
        .on_hover_text("表示切替・ドキュメント・ライセンス・バージョン情報");

        if let Some(item) = open_bundled {
            self.status = match item.open() {
                Ok(()) => format!("{}を開きました", item.name()),
                Err(message) => format!("{}を開けませんでした: {message}", item.name()),
            };
        }

        let panel_open = settings::is_panel_open(ctx);
        if ui
            .add(egui::Button::selectable(
                panel_open,
                (Icon::Settings.atom(None), "設定"),
            ))
            .on_hover_text("トリミング・背景色・出力の設定パネルを開閉する")
            .clicked()
        {
            settings::set_panel_open(ctx, !panel_open);
        }

        // 保存・コピーは設定パネル下部にあるため、閉じている間だけここに出す
        if !panel_open {
            ui.separator();
            self.render_output_buttons(ui, ctx, false);
        }

        if check_update {
            self.open_update_dialog(ctx);
        }
    }
}
