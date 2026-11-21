//! キーボードショートカットとファイルドロップの入力処理

use std::path::PathBuf;

use eframe::egui;

use crate::app::shortcuts::Shortcut;
use crate::app::state::App;

// ======================================================================
// キーボードショートカット
// ======================================================================
impl App {
    /// キーボードショートカットを判定して対応する操作を実行する
    ///
    /// テキスト入力中・更新ダイアログ表示中は受け付けない
    /// ラベルの文字列を選択中の Ctrl+C は文字列のコピーを優先する
    pub(super) fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        #[cfg(windows)]
        let dialog_open = self.update.dialog_open;
        #[cfg(not(windows))]
        let dialog_open = false;
        let enabled = !dialog_open && !ctx.text_edit_focused();
        let Some(shortcut) = ctx.input_mut(|i| self.shortcuts.detect(i, enabled)) else {
            return;
        };

        match shortcut {
            Shortcut::Open => self.open_image(ctx),
            Shortcut::BatchTrim => self.batch_trim(ctx),
            Shortcut::Paste => self.paste_from_clipboard(ctx),
            Shortcut::Save => self.save_image(ctx),
            Shortcut::Copy => {
                let label_selected = ctx
                    .with_plugin(|s: &mut egui::text_selection::LabelSelectionState| {
                        s.has_selection()
                    })
                    .unwrap_or(false);
                if !label_selected {
                    self.copy_trimmed_to_clipboard(ctx);
                }
            }
            Shortcut::Clear => {
                if self.image.original.is_some() {
                    self.clear_image();
                }
            }
            Shortcut::Cancel => {
                // 開いているメニュー・ポップアップを閉じる操作を優先する
                if egui::Popup::is_any_open(ctx) {
                    return;
                }
                if self.background.pick_mode {
                    self.background.pick_mode = false;
                } else if self.batch.running {
                    self.cancel_batch();
                }
            }
        }
    }
}

pub(super) fn handle_file_drop(app: &mut App, ctx: &egui::Context) {
    let dropped: Vec<PathBuf> = ctx.input(|i| {
        i.raw
            .dropped_files
            .iter()
            .map(|f| f.path().to_path_buf())
            .collect()
    });
    app.handle_dropped_paths(ctx, dropped);
}
