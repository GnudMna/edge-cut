//! 操作ボタン・表示切替・ヘルプメニューの描画

use super::ERROR_COLOR;
use crate::app::shortcuts;
use crate::app::state::App;

use eframe::egui;

// ======================================================================
// ツールバー
// ======================================================================
impl App {
    /// 操作ボタンの行
    ///
    /// 右端の表示切替を先に配置し、残りの幅に操作ボタンを並べる
    /// (幅が足りない場合は操作ボタンを折り返し、右端と重ならないようにする)
    pub(super) fn render_action_buttons(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
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
            .button("📂 一括トリミング")
            .on_hover_text(shortcuts::with_shortcut(
                ctx,
                "複数の画像をまとめてトリミングして保存 (出力形式の設定に従う)",
                shortcuts::BATCH_TRIM,
            ))
            .clicked()
            && !self.batch.running
        {
            self.batch_trim(ctx);
        }

        if self.batch.running
            && ui
                .button(egui::RichText::new("⏹ キャンセル").color(ERROR_COLOR))
                .on_hover_text(shortcuts::with_shortcut(
                    ctx,
                    "一括トリミングを中止",
                    shortcuts::CANCEL,
                ))
                .clicked()
        {
            self.cancel_batch();
        }

        if ui
            .button("📋 貼り付け")
            .on_hover_text(shortcuts::with_shortcut(
                ctx,
                "クリップボードの画像を使用",
                shortcuts::PASTE,
            ))
            .clicked()
        {
            self.paste_from_clipboard(ctx);
        }

        // 子 UI で囲むと折り返しが効かないため、`add_enabled` で直接配置する
        // 再計算の待ち中・処理中は前回の矩形が残っているため無効にする
        let has_trimmed = self.has_current_trim();
        if ui
            .add_enabled(has_trimmed, egui::Button::new("💾 保存"))
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
            .add_enabled(has_trimmed, egui::Button::new("📋 コピー"))
            .on_hover_text(shortcuts::with_shortcut(
                ctx,
                "トリミング後の画像をクリップボードへコピー",
                shortcuts::COPY,
            ))
            .clicked()
        {
            self.copy_trimmed_to_clipboard(ctx);
        }

        let has_original = self.image.original.is_some();
        let clear_color = if has_original {
            ERROR_COLOR
        } else {
            ui.visuals().weak_text_color()
        };
        if ui
            .add_enabled(
                has_original,
                egui::Button::new(egui::RichText::new("🗑 クリア").color(clear_color)),
            )
            .on_hover_text(shortcuts::with_shortcut(
                ctx,
                "画像をクリア",
                shortcuts::CLEAR,
            ))
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
            // コンパクトモードでは元画像を表示しないため、スポイトの対象がなくなる
            if self.prefs.compact_mode {
                self.background.pick_mode = false;
            }
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
}
