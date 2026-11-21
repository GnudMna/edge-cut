//! UI 描画 (`eframe::App` 実装)
use std::path::PathBuf;

use super::state::App;
use crate::consts::{PADDING_MAX, PADDING_MIN, TOL_MAX, TOL_MIN};

use eframe::egui;

// ======================================================================
// UI 描画
// ======================================================================
impl eframe::App for App {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.save_settings(storage);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // 一括トリミングの結果をポーリング
        self.poll_batch_result();

        // テーマの適用
        if self.dark_mode {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }

        // ファイルのドラッグアンドドロップ
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if let Some(path) = dropped.into_iter().next() {
            self.load_image_from_path(&ctx, &path);
        }

        let sp = if self.compact_mode { 1.0 } else { 3.0 };

        // ステータスバー (下部)
        egui::Panel::bottom("bottom").show_inside(ui, |ui| {
            ui.add_space(sp);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&self.status).color(ui.visuals().text_color()));
                if let Some(ref info) = self.info {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let detail = if self.compact_mode {
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

        // トップパネル
        egui::Panel::top("top").show_inside(ui, |ui| {
            ui.add_space(if self.compact_mode { 2.0 } else { 6.0 });

            // --- 行1: アクションボタン ---
            ui.horizontal(|ui| {
                if ui
                    .button(egui::RichText::new("📂 開く").strong())
                    .on_hover_text("画像ファイルを開く")
                    .clicked()
                {
                    self.open_image(&ctx);
                }

                if ui
                    .button("📂 一括トリミング")
                    .on_hover_text("複数の画像ファイルをまとめてトリミングして保存")
                    .clicked()
                    && !self.batch_running
                {
                    self.batch_trim(&ctx);
                }

                if self.batch_running
                    && ui
                        .button(
                            egui::RichText::new("⏹ キャンセル")
                                .color(egui::Color32::from_rgb(200, 80, 80)),
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
                    self.paste_from_clipboard(&ctx);
                }

                ui.add_enabled_ui(self.trimmed.is_some(), |ui| {
                    if ui
                        .button("💾 保存")
                        .on_hover_text("トリミング後の画像を保存")
                        .clicked()
                    {
                        self.save_image();
                    }
                });

                ui.add_enabled_ui(self.trimmed.is_some(), |ui| {
                    if ui
                        .button("📋 コピー")
                        .on_hover_text("トリミング後の画像をクリップボードへコピー")
                        .clicked()
                    {
                        self.copy_trimmed_to_clipboard();
                    }
                });

                ui.add_enabled_ui(self.original.is_some(), |ui| {
                    if ui
                        .button(egui::RichText::new("🗑 クリア").color(if ui.is_enabled() {
                            egui::Color32::from_rgb(200, 80, 80)
                        } else {
                            ui.visuals().weak_text_color()
                        }))
                        .on_hover_text("画像をクリア")
                        .clicked()
                    {
                        self.clear_image();
                    }
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let icon = if self.dark_mode {
                        "☀ ライト"
                    } else {
                        "🌙 ダーク"
                    };
                    if ui.button(icon).clicked() {
                        self.dark_mode = !self.dark_mode;
                    }
                    let compact_icon = if self.compact_mode {
                        "⊞ 標準"
                    } else {
                        "⊟ コンパクト"
                    };
                    if ui
                        .button(compact_icon)
                        .on_hover_text("コンパクト UI モードの切替 (トリミング後のみ表示)")
                        .clicked()
                    {
                        self.compact_mode = !self.compact_mode;
                    }
                });
            });

            if !self.compact_mode {
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(2.0);
            }

            // --- 行2: トリミング設定 ---
            ui.horizontal(|ui| {
                ui.label("許容度:");
                let tol_resp = ui.add(
                    egui::Slider::new(&mut self.tolerance, TOL_MIN..=TOL_MAX)
                        .clamping(egui::SliderClamping::Always),
                );
                // ドラッグ解放・キーボード操作時にトリミングを実行する
                if slider_needs_trim(&tol_resp, &ctx, &mut self.tolerance, TOL_MIN, TOL_MAX) {
                    self.do_trim(&ctx);
                }
                tol_resp.on_hover_text("背景色との類似度。値が大きいほど広い範囲を余白とみなす");

                ui.add_space(12.0);
                ui.label("余白:");
                let pad_resp = ui.add(
                    egui::Slider::new(&mut self.padding, PADDING_MIN..=PADDING_MAX)
                        .suffix(" px")
                        .clamping(egui::SliderClamping::Always),
                );
                // ドラッグ解放・キーボード操作時にトリミングを実行する
                if slider_needs_trim(&pad_resp, &ctx, &mut self.padding, PADDING_MIN, PADDING_MAX) {
                    self.do_trim(&ctx);
                }
                pad_resp.on_hover_text("トリミング後に四辺へ追加する余白ピクセル数");

                ui.add_space(if self.compact_mode { 8.0 } else { 16.0 });
                ui.separator();
                ui.add_space(if self.compact_mode { 2.0 } else { 4.0 });

                // 自動保存
                let prev_auto_save = self.auto_save;
                ui.checkbox(&mut self.auto_save, "自動保存");
                if self.auto_save && !prev_auto_save {
                    self.auto_save_if_enabled();
                }

                ui.add_space(4.0);
                let full_path = self.auto_save_dir.to_string_lossy().into_owned();
                let dir_btn = ui.button("📁 変更").on_hover_text(if self.compact_mode {
                    full_path.as_str()
                } else {
                    "保存先フォルダを変更"
                });
                if dir_btn.clicked()
                    && let Some(dir) = rfd::FileDialog::new()
                        .set_directory(&self.auto_save_dir)
                        .pick_folder()
                {
                    self.auto_save_dir = dir;
                }
                if !self.compact_mode {
                    ui.label(egui::RichText::new(&full_path).weak().monospace());
                }
            });

            ui.add_space(if self.compact_mode { 2.0 } else { 6.0 });
        });

        // 中央パネル (分割ビュー)
        egui::CentralPanel::default().show_inside(ui, |ui| {
            if self.original.is_none() {
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

            let orig_size = self.orig_tex.as_ref().map(|t| t.size_vec2());
            let trim_size = self.trim_tex.as_ref().map(|t| t.size_vec2());

            if self.compact_mode {
                let col_w = avail.x - 8.0;
                let trim_label = trim_size
                    .map(|s| format!("トリミング後  {}×{}", s.x as u32, s.y as u32))
                    .unwrap_or_else(|| "トリミング後".into());
                show_panel(ui, &trim_label, &self.trim_tex, col_w);
            } else {
                let col_w = (avail.x - 16.0) / 2.0;
                ui.columns(2, |cols| {
                    let orig_label = orig_size
                        .map(|s| format!("元画像  {}×{}", s.x as u32, s.y as u32))
                        .unwrap_or_else(|| "元画像".into());
                    let trim_label = trim_size
                        .map(|s| format!("トリミング後  {}×{}", s.x as u32, s.y as u32))
                        .unwrap_or_else(|| "トリミング後".into());
                    show_panel(&mut cols[0], &orig_label, &self.orig_tex, col_w);
                    show_panel(&mut cols[1], &trim_label, &self.trim_tex, col_w);
                });
            }
        });
    }
}

// ======================================================================
// UI ヘルパー
// ======================================================================
/// スライダーのレスポンスを受け取り、トリミングを再実行すべきかどうかを返す。
///
/// 以下のいずれかの条件を満たす場合に `true` を返し、`value` を更新する:
/// - ドラッグ操作が完了した
/// - ドラッグ以外 (キーボード等) で値が変化した
/// - ホバー中かつ未フォーカス状態で左右矢印キーが押された
///
/// # Arguments
/// * `resp`  - スライダーの `Response`
/// * `ctx`   - キー入力の取得に使う egui コンテキスト
/// * `value` - 書き換え対象の値
/// * `min`   - クランプ下限
/// * `max`   - クランプ上限
fn slider_needs_trim(
    resp: &egui::Response,
    ctx: &egui::Context,
    value: &mut u32,
    min: u32,
    max: u32,
) -> bool {
    // ドラッグ中はトリミングしない。ドラッグ解放またはキーボード操作時のみ実行
    if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
        return true;
    }
    // ホバー中はカーソルキーで操作可能にする(フォーカス済みの場合は egui 内部で処理)
    if resp.hovered() && !resp.has_focus() {
        let delta: i32 = ctx.input(|i| {
            i.key_pressed(egui::Key::ArrowRight) as i32 - i.key_pressed(egui::Key::ArrowLeft) as i32
        });
        if delta != 0 {
            *value = (*value as i32 + delta).clamp(min as i32, max as i32) as u32;
            return true;
        }
    }
    false
}

/// タイトルとスクロール可能な画像を 1 ペイン分描画する
///
/// # Arguments
/// * `ui`    - 描画対象の egui UI
/// * `title` - ペインのタイトル文字列
/// * `tex`   - 表示するテクスチャ (`None` のときは何も表示しない)
/// * `max_w` - ペインの最大幅 (px)
fn show_panel(ui: &mut egui::Ui, title: &str, tex: &Option<egui::TextureHandle>, max_w: f32) {
    ui.strong(title);
    ui.separator();
    let avail_h = ui.available_height();
    egui::ScrollArea::both()
        .id_salt(title)
        .max_width(max_w)
        .max_height(avail_h)
        .show(ui, |ui| {
            if let Some(t) = tex {
                let ts = t.size_vec2();
                let scale = ((max_w - 8.0) / ts.x).min((avail_h - 8.0) / ts.y).min(1.0);
                let disp = ts * scale;
                ui.add(egui::Image::new((t.id(), disp)));
            }
        });
}
