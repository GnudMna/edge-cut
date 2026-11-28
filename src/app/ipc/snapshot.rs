//! UI へ送る画面状態
//!
//! イベントを処理するたびに [`App::snapshot`] で画面状態を作り、前回と変わっていれば
//! JSON にして UI へ送る。UI はこれだけを元に描画する (UI 側に正の状態を持たない)

use serde::Serialize;

use crate::app::bundled::Bundled;
use crate::app::image_convert::to_preview_fill;
use crate::app::state::{App, CloseConfirm, PreviewMode, ThemeMode, UpdateUiPhase};
use crate::consts::{
    ASPECT_PART_MAX, ASPECT_PART_MIN, JPEG_QUALITY_MAX, JPEG_QUALITY_MIN, PADDING_MAX, PADDING_MIN,
    SPECK_MAX, SPECK_MIN, TOL_MAX, TOL_MIN, WEBP_QUALITY_MAX, WEBP_QUALITY_MIN,
};
use crate::image_output::OutputFormat;
use crate::trim::{AspectFit, Margins, TrimInfo, TrimSides};

// ======================================================================
// 画面状態
// ======================================================================
/// UI へ送る画面状態
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::app) struct Snapshot {
    /// ステータスバーのメッセージ
    pub status: String,
    /// アプリのバージョン
    pub version: &'static str,
    /// 設定
    pub prefs: PrefsView,
    /// 読み込んだ画像。未読み込みなら `None`
    pub image: Option<ImageView>,
    /// 背景色
    pub background: BackgroundView,
    /// 処理中の状態
    pub busy: BusyView,
    /// 一括トリミング
    pub batch: BatchView,
    /// アプリ更新
    pub update: UpdateView,
    /// 一括トリミング中に閉じようとして、終了を確認しているか
    pub close_confirm: bool,
    /// 同梱の文書が見つかったか
    pub bundled: BundledView,
    /// ウィンドウの状態
    pub window: WindowView,
    /// 設定値の範囲
    pub limits: Limits,
}

/// 設定
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_excessive_bools,
    reason = "UI に表示する独立した ON/OFF の設定項目"
)]
pub(in crate::app) struct PrefsView {
    pub tolerance: u32,
    pub ignore_pixels: u32,
    pub padding: u32,
    pub link_margins: bool,
    pub margins: Margins,
    pub sides: TrimSides,
    pub aspect_enabled: bool,
    pub aspect_w: u32,
    pub aspect_h: u32,
    pub aspect_fit: AspectFit,
    pub theme: ThemeMode,
    pub preview_mode: PreviewMode,
    pub output_format: OutputFormat,
    pub jpeg_quality: u8,
    pub webp_quality: u8,
    pub overwrite_output: bool,
    pub auto_save: bool,
    /// 自動保存先 (表示用の文字列)
    pub auto_save_dir: String,
    pub panel_open: bool,
    pub panel_width: u32,
}

/// 読み込んだ画像
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::app) struct ImageView {
    /// 元画像のファイル名 (クリップボード由来は `clipboard`)
    pub name: String,
    /// 元画像の幅 (px)
    pub width: u32,
    /// 元画像の高さ (px)
    pub height: u32,
    /// プレビュー画像。生成中は `None`
    pub preview: Option<PreviewView>,
    /// 直近のトリミング結果。未計算・失敗時は `None`
    pub trim: Option<TrimView>,
    /// 保存時の拡張子 (出力形式と元画像の形式から決まる)
    pub output_ext: String,
}

/// プレビュー画像の取得先
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::app) struct PreviewView {
    /// 識別子。カスタムプロトコルの `/preview/{id}` で RGBA を取得する
    pub id: u64,
    /// プレビュー画像の幅 (px)。大きな画像は縮小している
    pub width: u32,
    /// プレビュー画像の高さ (px)
    pub height: u32,
}

/// トリミング結果
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::app) struct TrimView {
    /// トリミング後の範囲 (元画像の座標。範囲外へ広げた余白を含むため負の値もとる)
    pub rect: RectView,
    /// 元画像から切り出す範囲 (元画像の座標)
    pub crop: RectView,
    /// 範囲外へ広げた余白の塗り色 (プレビュー用に sRGB へ変換済み)
    pub preview_fill: [u8; 4],
    /// 範囲外へ広げた余白の塗り色 (変換前)
    pub fill: [u8; 4],
}

/// 矩形
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(in crate::app) struct RectView {
    pub x: i64,
    pub y: i64,
    pub w: u32,
    pub h: u32,
}

/// 背景色
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::app) struct BackgroundView {
    /// 直近のトリミングで使った背景色 (未計算なら `None`)
    pub color: Option<[u8; 4]>,
    /// 手動指定中か
    pub manual: bool,
    /// スポイト中か
    pub picking: bool,
}

/// 処理中の状態
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_excessive_bools,
    reason = "UI に表示する独立した処理中フラグ"
)]
pub(in crate::app) struct BusyView {
    /// プレビューを再計算中か
    pub trimming: bool,
    /// 保存中か
    pub saving: bool,
    /// ファイルを読み込み中か
    pub loading: bool,
    /// ファイルダイアログを表示中か
    pub dialog: bool,
    /// 現在の設定でのトリミング結果があり、保存・コピーできるか
    pub can_output: bool,
}

/// 一括トリミング
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::app) struct BatchView {
    /// 実行中か
    pub running: bool,
    /// 処理済みの件数
    pub processed: usize,
    /// 全件数
    pub total: usize,
    /// 直近の一括トリミングで失敗したファイルとその理由
    pub errors: Vec<String>,
    /// エラー一覧を表示中か
    pub errors_open: bool,
}

/// アプリ更新
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::app) struct UpdateView {
    /// 更新ダイアログを表示中か
    pub open: bool,
    /// 一括トリミング・保存の実行中で適用できないか
    pub apply_blocked: bool,
    /// 進行状況
    pub phase: UpdatePhaseView,
}

/// 更新の進行状況
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(in crate::app) enum UpdatePhaseView {
    /// 未確認
    Idle,
    /// 確認中
    Checking,
    /// 最新版
    UpToDate { current: String },
    /// 更新あり
    Available {
        current: String,
        latest: String,
        size: u64,
    },
    /// 失敗
    Error { message: String },
    /// 更新プログラムで更新を進めている
    UpdaterRunning { latest: String },
    /// 更新のため終了する (保存の完了待ちを含む)
    Exiting,
}

/// ウィンドウの状態 (イベントループが持つ)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::app) struct WindowView {
    /// 最大化しているか
    pub maximized: bool,
    /// ファイルをドラッグしてウィンドウに重ねているか
    pub drag_hover: bool,
}

/// 同梱の文書が見つかったか
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::app) struct BundledView {
    pub docs: bool,
    pub license: bool,
    pub third_party_notices: bool,
}

impl BundledView {
    /// 同梱の文書を探す (ファイルシステムを見るため起動時に 1 回だけ呼ぶ)
    ///
    /// # Returns
    /// * `Self` - 文書ごとの有無
    pub(in crate::app) fn locate() -> Self {
        Self {
            docs: Bundled::Docs.locate().is_some(),
            license: Bundled::License.locate().is_some(),
            third_party_notices: Bundled::ThirdPartyNotices.locate().is_some(),
        }
    }
}

/// 設定値の範囲 `[最小, 最大]`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::app) struct Limits {
    pub tolerance: [u32; 2],
    pub padding: [u32; 2],
    pub ignore_pixels: [u32; 2],
    pub aspect_part: [u32; 2],
    pub jpeg_quality: [u8; 2],
    pub webp_quality: [u8; 2],
    pub panel_width: [u32; 2],
}

/// 設定値の範囲
const LIMITS: Limits = Limits {
    tolerance: [TOL_MIN, TOL_MAX],
    padding: [PADDING_MIN, PADDING_MAX],
    ignore_pixels: [SPECK_MIN, SPECK_MAX],
    aspect_part: [ASPECT_PART_MIN, ASPECT_PART_MAX],
    jpeg_quality: [JPEG_QUALITY_MIN, JPEG_QUALITY_MAX],
    webp_quality: [WEBP_QUALITY_MIN, WEBP_QUALITY_MAX],
    panel_width: [
        crate::app::state::PANEL_WIDTH_MIN,
        crate::app::state::PANEL_WIDTH_MAX,
    ],
};

// ======================================================================
// 組み立て
// ======================================================================
impl App {
    /// 現在の画面状態を作る
    ///
    /// 余白の塗り色の ICC 変換結果は `image.preview_fill` に保持し、毎回変換し直さない
    ///
    /// # Arguments
    /// * `bundled` - 同梱の文書の有無
    /// * `window` - ウィンドウの状態
    ///
    /// # Returns
    /// * `Snapshot` - UI へ送る画面状態
    pub(in crate::app) fn snapshot(
        &mut self,
        bundled: BundledView,
        window: WindowView,
    ) -> Snapshot {
        let image = self.image_view();
        let p = &self.prefs;
        Snapshot {
            status: self.status.clone(),
            version: env!("CARGO_PKG_VERSION"),
            prefs: PrefsView {
                tolerance: p.tolerance,
                ignore_pixels: p.ignore_pixels,
                padding: p.padding,
                link_margins: p.link_margins,
                margins: p.margins,
                sides: p.sides,
                aspect_enabled: p.aspect_enabled,
                aspect_w: p.aspect_w,
                aspect_h: p.aspect_h,
                aspect_fit: p.aspect_fit,
                theme: p.theme.unwrap_or(ThemeMode::System),
                preview_mode: p.preview_mode,
                output_format: p.output_format,
                jpeg_quality: p.jpeg_quality,
                webp_quality: p.webp_quality,
                overwrite_output: p.overwrite_output,
                auto_save: p.auto_save,
                auto_save_dir: p.auto_save_dir.display().to_string(),
                panel_open: p.panel_open,
                panel_width: p.panel_width,
            },
            image,
            background: BackgroundView {
                color: self.background.bg_color,
                manual: self.prefs.bg_override.is_some(),
                picking: self.background.pick_mode,
            },
            busy: BusyView {
                trimming: self.trim_worker.is_busy(),
                saving: self.io.is_saving(),
                loading: self.io.loading,
                dialog: self.io.dialog_open,
                can_output: self.has_current_trim(),
            },
            batch: BatchView {
                running: self.batch.running,
                processed: self.batch.progress.0,
                total: self.batch.progress.1,
                errors: self.batch.last_error_details.clone(),
                errors_open: self.batch.errors_open,
            },
            update: UpdateView {
                open: self.update.dialog_open,
                apply_blocked: self.has_file_work(),
                phase: update_phase_view(&self.update.phase),
            },
            close_confirm: self.batch.close_confirm == CloseConfirm::Asking && self.batch.running,
            bundled,
            window,
            limits: LIMITS,
        }
    }

    /// 読み込んだ画像の表示内容を作る
    fn image_view(&mut self) -> Option<ImageView> {
        let original = self.image.original.as_ref()?;
        let (width, height) = (original.width(), original.height());
        let source = self.image.source_name.clone().unwrap_or_default();
        let output_ext = self
            .prefs
            .output_format
            .resolve_extension(&source, self.image.source_format);
        let preview = self.image.preview.as_ref().map(|p| PreviewView {
            id: self.image.preview_id,
            width: p.width(),
            height: p.height(),
        });
        let trim = self.image.info.map(|info| {
            let preview_fill = self.preview_fill(info.fill);
            trim_view(&info, preview_fill)
        });
        Some(ImageView {
            name: source.display().to_string(),
            width,
            height,
            preview,
            trim,
            output_ext,
        })
    }

    /// 余白の塗り色をプレビュー用の色へ変換する (同じ色なら前回の結果を使う)
    fn preview_fill(&mut self, fill: [u8; 4]) -> [u8; 4] {
        match self.image.preview_fill {
            Some((from, to)) if from == fill => to,
            _ => {
                let to = to_preview_fill(fill, self.image.icc_profile.as_deref());
                self.image.preview_fill = Some((fill, to));
                to
            }
        }
    }
}

/// トリミング結果を表示用の矩形にする
///
/// # Arguments
/// * `info` - トリミング結果
/// * `preview_fill` - プレビュー用の塗り色
///
/// # Returns
/// * `TrimView` - 元画像の座標での範囲
fn trim_view(info: &TrimInfo, preview_fill: [u8; 4]) -> TrimView {
    let (ox, oy) = (i64::from(info.offset_x), i64::from(info.offset_y));
    TrimView {
        rect: RectView {
            x: ox - i64::from(info.extend_left),
            y: oy - i64::from(info.extend_top),
            w: info.trim_w,
            h: info.trim_h,
        },
        crop: RectView {
            x: ox,
            y: oy,
            w: info.crop_w(),
            h: info.crop_h(),
        },
        preview_fill,
        fill: info.fill,
    }
}

/// 更新の進行状況を表示用にする
fn update_phase_view(phase: &UpdateUiPhase) -> UpdatePhaseView {
    match phase {
        UpdateUiPhase::Idle => UpdatePhaseView::Idle,
        UpdateUiPhase::Checking => UpdatePhaseView::Checking,
        UpdateUiPhase::UpToDate { current_version } => UpdatePhaseView::UpToDate {
            current: current_version.clone(),
        },
        UpdateUiPhase::Available(u) => UpdatePhaseView::Available {
            current: u.current_version.clone(),
            latest: u.latest_version.clone(),
            size: u.size,
        },
        UpdateUiPhase::Error { message } => UpdatePhaseView::Error {
            message: message.clone(),
        },
        UpdateUiPhase::UpdaterRunning(u) => UpdatePhaseView::UpdaterRunning {
            latest: u.latest_version.clone(),
        },
        UpdateUiPhase::Exiting => UpdatePhaseView::Exiting,
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::actions::test_helpers::trimmed_image_state;

    const NO_BUNDLED: BundledView = BundledView {
        docs: false,
        license: false,
        third_party_notices: false,
    };

    /// 画像が無いときは画像の表示内容を持たないこと
    #[test]
    fn snapshot_without_image() {
        let mut app = App::default();
        let snap = app.snapshot(NO_BUNDLED, WindowView::default());
        assert!(snap.image.is_none());
        assert!(!snap.busy.can_output);
        assert_eq!(snap.limits.tolerance, [TOL_MIN, TOL_MAX]);
    }

    /// トリミング結果を元画像の座標の矩形として持つこと
    #[test]
    fn snapshot_with_trim_result() {
        let mut app = App {
            image: trimmed_image_state(),
            ..App::default()
        };
        let snap = app.snapshot(NO_BUNDLED, WindowView::default());
        let image = snap.image.expect("画像あり");
        assert_eq!((image.width, image.height), (4, 4));
        let trim = image.trim.expect("トリミング結果あり");
        assert_eq!(
            trim.rect,
            RectView {
                x: 0,
                y: 0,
                w: 4,
                h: 4
            }
        );
        assert!(snap.busy.can_output);
    }

    /// 範囲外へ広げた余白は負の座標で表すこと
    #[test]
    fn trim_view_includes_extension() {
        let info = TrimInfo {
            orig_w: 10,
            orig_h: 10,
            trim_w: 14,
            trim_h: 12,
            offset_x: 0,
            offset_y: 1,
            extend_left: 3,
            extend_top: 2,
            extend_right: 1,
            extend_bottom: 0,
            fill: [1, 2, 3, 255],
        };
        let view = trim_view(&info, [1, 2, 3, 255]);
        assert_eq!(
            view.rect,
            RectView {
                x: -3,
                y: -1,
                w: 14,
                h: 12
            }
        );
        assert_eq!(
            view.crop,
            RectView {
                x: 0,
                y: 1,
                w: 10,
                h: 10
            }
        );
    }

    /// 画面状態を UI が読む JSON の形で書き出すこと
    #[test]
    fn snapshot_serializes_camel_case() {
        let mut app = App::default();
        let json = serde_json::to_value(app.snapshot(NO_BUNDLED, WindowView::default())).unwrap();
        assert!(json["prefs"]["ignorePixels"].is_number());
        assert_eq!(json["update"]["phase"]["kind"], "idle");
        assert_eq!(json["prefs"]["previewMode"], "Range");
    }
}
