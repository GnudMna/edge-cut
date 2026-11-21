//! 読み込み画像・プレビュー表示・背景色の状態

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui;
use image::DynamicImage;

use super::trim_state::TrimParams;
use crate::trim::TrimInfo;

// ======================================================================
// 状態の部分構造体
// ======================================================================
/// プレビュー 1 ペイン分のズームとパンの状態
#[derive(Clone, Copy, Default)]
pub(in crate::app) struct PaneView {
    /// 画像 1 px あたりの画面 px 数。`None` のときはペインに収まる倍率 (最大 1.0) に追従する
    pub zoom: Option<f32>,
    /// 画像の中心がペインの中心からずれている量 (画面 px)
    pub offset: egui::Vec2,
}

/// 読み込み画像とプレビュー表示
#[derive(Default)]
pub(in crate::app) struct ImageState {
    /// 読み込んだ元画像
    pub original: Option<Arc<DynamicImage>>,
    /// 元画像のファイル名 (クリップボード由来は拡張子なしの `clipboard`)
    ///
    /// 出力ファイル名と、出力形式が `Original` のときの形式の決定に使う
    pub source_name: Option<PathBuf>,
    /// 元画像の ICC プロファイル。保存時に埋め込む。保存スレッドへ複製なしで渡すため `Arc` で保持する
    pub icc_profile: Option<Arc<[u8]>>,
    /// 元画像の GPU テクスチャ。トリミング後のプレビューも UV で切り出して共用する
    pub orig_tex: Option<egui::TextureHandle>,
    /// プレビューのズームとパン。`[0]` が元画像、`[1]` がトリミング後のペイン
    pub views: [PaneView; 2],
    /// 直近のトリミング情報。切り抜き画像は保持せず、保存・コピー時にこの矩形で切り出す
    pub info: Option<TrimInfo>,
    /// 直近のトリミング実行時の `(tolerance, padding, bg_override)`。同じ値なら再実行をスキップする
    pub cache_key: Option<TrimParams>,
}

impl ImageState {
    /// 新しい画像読み込み前にプレビュー関連の状態をリセットする
    pub(in crate::app) fn prepare_for_load(&mut self) {
        self.orig_tex = None;
        self.views = [PaneView::default(); 2];
        self.info = None;
        self.cache_key = None;
    }

    /// 保存・コピー対象の元画像とトリミング矩形を返す
    ///
    /// # Returns
    /// * `Option<(Arc<DynamicImage>, TrimInfo)>` - トリミング済みなら元画像と矩形、
    ///   画像未読み込みまたはトリミング未完了なら `None`
    pub(in crate::app) fn trim_target(&self) -> Option<(Arc<DynamicImage>, TrimInfo)> {
        Some((Arc::clone(self.original.as_ref()?), self.info?))
    }

    /// トリミングパラメータ変更時にキャッシュを無効化する
    pub(in crate::app) fn invalidate_cache(&mut self) {
        self.cache_key = None;
    }

    /// 画像とプレビューをすべてクリアする
    pub(in crate::app) fn clear(&mut self) {
        *self = Self::default();
    }
}

/// 背景色の手動指定と表示
#[derive(Default)]
pub(in crate::app) struct BackgroundState {
    /// 直近のトリミングで使用した背景色 (表示用)
    pub bg_color: Option<[u8; 4]>,
    /// スポイトモード: 元画像クリックで背景色を設定する
    pub pick_mode: bool,
}

impl BackgroundState {
    /// 保存済みの手動背景色から状態を復元する
    pub(super) fn restore_from_settings(bg_override: Option<[u8; 4]>) -> Self {
        Self {
            bg_color: bg_override,
            pick_mode: false,
        }
    }

    /// 表示色を手動指定へ同期し、スポイトを終了する
    ///
    /// 画像の読み込み後・クリア後に呼ぶ
    ///
    /// # Arguments
    /// * `bg_override` - 現在の手動指定の背景色 (`Settings::bg_override`)
    pub(in crate::app) fn reset_to_override(&mut self, bg_override: Option<[u8; 4]>) {
        self.bg_color = bg_override;
        self.pick_mode = false;
    }
}
