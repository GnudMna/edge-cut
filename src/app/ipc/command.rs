//! UI から送られるコマンドの定義と処理
//!
//! UI は `window.ipc.postMessage` で JSON のコマンドを送る。ウィンドウ操作以外のコマンドは
//! [`App::handle_command`] で状態へ反映し、ウィンドウ操作はイベントループへ返して処理させる

use serde::Deserialize;

use crate::app::bundled::Bundled;
use crate::app::state::{App, CloseConfirm, PreviewMode, ThemeMode};
use crate::image_output::OutputFormat;
use crate::trim::{AspectFit, Margins, TrimSides};

// ======================================================================
// コマンド
// ======================================================================
/// UI から送られるコマンド
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(in crate::app) enum Command {
    /// UI の初期化が終わった (ウィンドウを表示し、現在の状態を送る)
    Ready,
    /// 画像を開くダイアログを表示する
    OpenImage,
    /// クリップボードから貼り付ける
    Paste,
    /// 一括トリミングの対象を選ぶダイアログを表示する
    BatchTrim,
    /// 一括トリミングを中止する
    CancelBatch,
    /// 画像を閉じる
    Clear,
    /// トリミング後の画像を保存する
    Save,
    /// トリミング後の画像をクリップボードへコピーする
    Copy,
    /// トリミングの設定を変える
    SetTrim {
        /// 変える項目
        patch: TrimPatch,
        /// スライダーのドラッグ中など、続けて値が変わる途中か (再計算を待ってまとめる)
        #[serde(default)]
        live: bool,
    },
    /// 出力の設定を変える
    SetOutput {
        /// 変える項目
        patch: OutputPatch,
    },
    /// 自動保存の有無を変える (有効にしたときは現在の結果を保存する)
    SetAutoSave {
        /// 自動保存するか
        enabled: bool,
    },
    /// 自動保存先を選ぶダイアログを表示する
    PickAutoSaveDir,
    /// プレビューの表示を変える
    SetPreviewMode {
        /// 表示モード
        mode: PreviewMode,
    },
    /// 配色テーマを変える
    SetTheme {
        /// テーマ
        mode: ThemeMode,
    },
    /// 設定パネルの開閉・幅を変える
    SetPanel {
        /// 開くか
        open: Option<bool>,
        /// 幅 (論理 px)
        width: Option<u32>,
    },
    /// スポイトの開始・終了
    SetPickMode {
        /// スポイト中にするか
        enabled: bool,
    },
    /// 元画像の画素から背景色を取得する (スポイト中のみ)
    PickPixel {
        /// 元画像の x 座標 (px)
        x: u32,
        /// 元画像の y 座標 (px)
        y: u32,
    },
    /// 背景色を手動で指定する
    SetBackground {
        /// RGBA
        rgba: [u8; 4],
    },
    /// 背景色を自動検出に戻す
    ResetBackground,
    /// 同梱の文書を開く
    OpenBundled {
        /// 開く文書
        item: BundledItem,
    },
    /// 更新ダイアログを開いて更新を確認する
    OpenUpdate,
    /// 更新を確認し直す
    CheckUpdate,
    /// 更新プログラムを起動する
    ApplyUpdate,
    /// 更新ダイアログを閉じる (更新のための終了中は閉じない)
    CloseUpdate,
    /// 一括トリミングのエラー一覧を開く
    ShowBatchErrors,
    /// 一括トリミングのエラー一覧を閉じる
    CloseBatchErrors,
    /// 一括トリミングを中止して終了する
    ConfirmClose,
    /// 終了をやめて一括トリミングを続ける
    CancelClose,
    /// ウィンドウを操作する
    Window {
        /// 操作
        action: WindowAction,
    },
}

/// トリミング設定の変更。`None` の項目は変えない
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(in crate::app) struct TrimPatch {
    pub tolerance: Option<u32>,
    pub ignore_pixels: Option<u32>,
    pub padding: Option<u32>,
    pub link_margins: Option<bool>,
    pub margins: Option<Margins>,
    pub sides: Option<TrimSides>,
    pub aspect_enabled: Option<bool>,
    pub aspect_w: Option<u32>,
    pub aspect_h: Option<u32>,
    pub aspect_fit: Option<AspectFit>,
}

/// 出力設定の変更。`None` の項目は変えない
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(in crate::app) struct OutputPatch {
    pub format: Option<OutputFormat>,
    pub jpeg_quality: Option<u8>,
    pub webp_quality: Option<u8>,
    pub overwrite_output: Option<bool>,
}

/// UI から開ける同梱の文書
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::app) enum BundledItem {
    /// 利用者向けドキュメント
    Docs,
    /// このアプリのライセンス
    License,
    /// 第三者ライセンス
    ThirdPartyNotices,
}

impl From<BundledItem> for Bundled {
    fn from(item: BundledItem) -> Self {
        match item {
            BundledItem::Docs => Self::Docs,
            BundledItem::License => Self::License,
            BundledItem::ThirdPartyNotices => Self::ThirdPartyNotices,
        }
    }
}

/// ウィンドウの操作 (イベントループで処理する)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(in crate::app) enum WindowAction {
    /// 最小化する
    Minimize,
    /// 最大化と元のサイズを切り替える
    ToggleMaximize,
    /// 閉じる (一括トリミング中は確認する)
    Close,
    /// タイトルバーのドラッグでウィンドウを移動する
    Drag,
    /// 縁のドラッグでウィンドウの大きさを変える
    Resize {
        /// ドラッグした縁
        edge: ResizeEdge,
    },
}

/// 大きさを変えるときにドラッグした縁
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::app) enum ResizeEdge {
    North,
    NorthEast,
    NorthWest,
}

/// コマンドを処理した結果、イベントループで行うこと
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum Effect {
    /// なし
    None,
    /// UI の準備ができた
    Ready,
    /// ウィンドウを操作する
    Window(WindowAction),
}

/// UI から届いた JSON をコマンドとして解析する
///
/// # Arguments
/// * `json` - `window.ipc.postMessage` で届いた文字列
///
/// # Returns
/// * `Result<Command, String>` - 解析したコマンド、または解析できなかった理由
pub(in crate::app) fn parse_command(json: &str) -> Result<Command, String> {
    serde_json::from_str(json).map_err(|e| format!("UI からのコマンドを解析できません: {e}"))
}

// ======================================================================
// コマンドの処理
// ======================================================================
impl App {
    /// UI からのコマンドを状態へ反映する
    ///
    /// # Arguments
    /// * `command` - 処理するコマンド
    ///
    /// # Returns
    /// * `Effect` - イベントループで行うこと (ウィンドウ操作など)
    pub(in crate::app) fn handle_command(&mut self, command: Command) -> Effect {
        match command {
            Command::Ready => return Effect::Ready,
            Command::Window { action } => return Effect::Window(action),
            Command::OpenImage => self.open_image(),
            Command::Paste => self.paste_from_clipboard(),
            Command::BatchTrim => self.batch_trim(),
            Command::CancelBatch => self.cancel_batch(),
            Command::Clear => {
                if self.image.original.is_some() {
                    self.clear_image();
                }
            }
            Command::Save => self.save_image(),
            Command::Copy => self.copy_trimmed_to_clipboard(),
            Command::SetTrim { patch, live } => self.apply_trim_patch(&patch, live),
            Command::SetOutput { patch } => self.apply_output_patch(&patch),
            Command::SetAutoSave { enabled } => {
                let was_enabled = self.prefs.auto_save;
                self.prefs.auto_save = enabled;
                if enabled && !was_enabled {
                    self.request_auto_save();
                }
            }
            Command::PickAutoSaveDir => self.pick_auto_save_dir(),
            Command::SetPreviewMode { mode } => self.prefs.preview_mode = mode,
            Command::SetTheme { mode } => self.prefs.theme = Some(mode),
            Command::SetPanel { open, width } => {
                if let Some(open) = open {
                    self.prefs.panel_open = open;
                }
                if let Some(width) = width {
                    self.prefs.panel_width = width;
                    self.prefs = self.prefs.clone().normalize();
                }
            }
            Command::SetPickMode { enabled } => {
                self.background.pick_mode = enabled && self.image.original.is_some();
            }
            Command::PickPixel { x, y } => {
                if self.background.pick_mode {
                    self.pick_bg_from_pixel(x, y);
                }
            }
            Command::SetBackground { rgba } => self.set_manual_bg(rgba),
            Command::ResetBackground => self.reset_bg_to_auto(),
            Command::OpenBundled { item } => self.open_bundled(item.into()),
            Command::OpenUpdate => self.open_update_dialog(),
            Command::CheckUpdate => self.start_update_check(),
            Command::ApplyUpdate => self.start_update_apply(),
            Command::CloseUpdate => {
                // 終了を依頼された後はアプリ終了まで進むため閉じない
                if !self.update.is_applying() {
                    self.update.dialog_open = false;
                }
            }
            Command::ShowBatchErrors => {
                self.batch.errors_open = !self.batch.last_error_details.is_empty();
            }
            Command::CloseBatchErrors => self.batch.errors_open = false,
            Command::ConfirmClose => {
                self.batch.close_confirm = CloseConfirm::Confirmed;
                return Effect::Window(WindowAction::Close);
            }
            Command::CancelClose => self.batch.close_confirm = CloseConfirm::None,
        }
        Effect::None
    }

    /// トリミング設定の変更を反映し、値が変わればプレビューを再計算する
    ///
    /// # Arguments
    /// * `patch` - 変える項目
    /// * `live` - 続けて値が変わる途中か (再計算を短く待ってまとめる)
    fn apply_trim_patch(&mut self, patch: &TrimPatch, live: bool) {
        let before = self.prefs.trim_options();
        let p = &mut self.prefs;
        macro_rules! set {
            ($($field:ident),*) => {
                $(if let Some(v) = patch.$field { p.$field = v; })*
            };
        }
        set!(
            tolerance,
            ignore_pixels,
            padding,
            link_margins,
            margins,
            sides,
            aspect_enabled,
            aspect_w,
            aspect_h,
            aspect_fit
        );
        self.prefs = self.prefs.clone().normalize();
        // ドラッグの終了時は値が同じでも、待ち中の再計算をすぐ始める
        if self.prefs.trim_options() != before || !live {
            self.schedule_trim(live, false, false);
        }
    }

    /// 出力設定の変更を反映する
    ///
    /// # Arguments
    /// * `patch` - 変える項目
    fn apply_output_patch(&mut self, patch: &OutputPatch) {
        if let Some(format) = patch.format {
            self.prefs.output_format = format;
        }
        if let Some(q) = patch.jpeg_quality {
            self.prefs.jpeg_quality = q;
        }
        if let Some(q) = patch.webp_quality {
            self.prefs.webp_quality = q;
        }
        if let Some(overwrite) = patch.overwrite_output {
            self.prefs.overwrite_output = overwrite;
        }
        self.prefs = self.prefs.clone().normalize();
    }

    /// 同梱の文書を開き、結果をステータスに表示する
    ///
    /// # Arguments
    /// * `item` - 開く文書
    fn open_bundled(&mut self, item: Bundled) {
        self.status = match item.open() {
            Ok(()) => format!("{}を開きました", item.name()),
            Err(message) => format!("{}を開けませんでした: {message}", item.name()),
        };
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::PANEL_WIDTH_MIN;
    use crate::consts::TOL_MAX;

    /// UI が送る JSON をコマンドとして解析できること
    #[test]
    fn parse_command_accepts_ui_json() {
        assert_eq!(
            parse_command(r#"{"type":"openImage"}"#),
            Ok(Command::OpenImage)
        );
        assert_eq!(
            parse_command(
                r#"{"type":"setTrim","patch":{"tolerance":5,"aspectFit":"Crop"},"live":true}"#
            ),
            Ok(Command::SetTrim {
                patch: TrimPatch {
                    tolerance: Some(5),
                    aspect_fit: Some(AspectFit::Crop),
                    ..TrimPatch::default()
                },
                live: true,
            })
        );
        assert_eq!(
            parse_command(r#"{"type":"window","action":{"kind":"resize","edge":"northEast"}}"#),
            Ok(Command::Window {
                action: WindowAction::Resize {
                    edge: ResizeEdge::NorthEast
                }
            })
        );
        assert_eq!(
            parse_command(r#"{"type":"setPreviewMode","mode":"SideBySide"}"#),
            Ok(Command::SetPreviewMode {
                mode: PreviewMode::SideBySide
            })
        );
    }

    /// 未知のコマンドや壊れた JSON は理由付きの失敗になること
    #[test]
    fn parse_command_rejects_unknown() {
        assert!(parse_command(r#"{"type":"format-disk"}"#).is_err());
        assert!(parse_command("{").is_err());
    }

    /// トリミング設定の変更は範囲内に収めて反映すること
    #[test]
    fn set_trim_clamps_values() {
        let mut app = App::default();
        let effect = app.handle_command(Command::SetTrim {
            patch: TrimPatch {
                tolerance: Some(1000),
                link_margins: Some(false),
                ..TrimPatch::default()
            },
            live: false,
        });
        assert_eq!(effect, Effect::None);
        assert_eq!(app.prefs.tolerance, TOL_MAX);
        assert!(!app.prefs.link_margins);
    }

    /// 画像があるときにトリミング設定を変えると再計算を始めること
    #[test]
    fn set_trim_schedules_trim_with_image() {
        let mut app = App {
            image: crate::app::actions::test_helpers::trimmed_image_state(),
            ..App::default()
        };
        app.image.cache_key = Some(app.prefs.trim_options());
        app.handle_command(Command::SetTrim {
            patch: TrimPatch {
                tolerance: Some(10),
                ..TrimPatch::default()
            },
            live: true,
        });
        // ドラッグ中はデバウンスして待つ
        assert!(app.trim_worker.debounce_until.is_some());
        assert!(!app.has_current_trim());
    }

    /// 画像が無いときはスポイトを始めないこと
    #[test]
    fn set_pick_mode_requires_image() {
        let mut app = App::default();
        app.handle_command(Command::SetPickMode { enabled: true });
        assert!(!app.background.pick_mode);
    }

    /// 更新のための終了中は更新ダイアログを閉じないこと
    #[test]
    fn close_update_keeps_dialog_while_exiting() {
        let mut app = App::default();
        app.update.dialog_open = true;
        app.update.phase = crate::app::state::UpdateUiPhase::Exiting;
        app.handle_command(Command::CloseUpdate);
        assert!(app.update.dialog_open);

        app.update.phase = crate::app::state::UpdateUiPhase::Idle;
        app.handle_command(Command::CloseUpdate);
        assert!(!app.update.dialog_open);
    }

    /// 終了の確認で中止を選ぶと、承諾済みにしてウィンドウを閉じること
    #[test]
    fn confirm_close_marks_confirmed_and_closes() {
        let mut app = App::default();
        app.batch.close_confirm = CloseConfirm::Asking;
        let effect = app.handle_command(Command::ConfirmClose);
        assert_eq!(effect, Effect::Window(WindowAction::Close));
        assert_eq!(app.batch.close_confirm, CloseConfirm::Confirmed);
    }

    /// パネル幅は範囲内に収めること
    #[test]
    fn set_panel_clamps_width() {
        let mut app = App::default();
        app.handle_command(Command::SetPanel {
            open: Some(false),
            width: Some(5),
        });
        assert!(!app.prefs.panel_open);
        assert_eq!(app.prefs.panel_width, PANEL_WIDTH_MIN);
    }
}
