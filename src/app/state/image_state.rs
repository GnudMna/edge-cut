//! 読み込み画像・プレビュー表示・背景色の状態

use std::path::PathBuf;
use std::sync::Arc;

use image::{DynamicImage, RgbaImage};

use super::trim_state::TrimParams;
use crate::dpi::Dpi;
use crate::image_output::OutputFormat;
use crate::trim::TrimInfo;

// ======================================================================
// 状態の部分構造体
// ======================================================================
/// 読み込み画像とプレビュー表示
#[derive(Default)]
pub(in crate::app) struct ImageState {
    /// 読み込んだ元画像
    pub original: Option<Arc<DynamicImage>>,
    /// 元画像のファイル名 (クリップボード由来は拡張子なしの `clipboard`)
    ///
    /// 出力ファイル名と、出力形式が `Original` のときの形式の決定に使う
    pub source_name: Option<PathBuf>,
    /// 元画像のあるフォルダ。手動保存ダイアログの初期位置に使う
    ///
    /// クリップボード由来や親フォルダを特定できない場合は `None`
    pub source_dir: Option<PathBuf>,
    /// 読み込み時にファイルの中身から判定した元画像の形式
    ///
    /// 出力形式が `Original` のときの形式の決定に使う。クリップボード由来・判定不能は `None`
    pub source_format: Option<OutputFormat>,
    /// 元画像の ICC プロファイル。保存時に埋め込む。保存スレッドへ複製なしで渡すため `Arc` で保持する
    pub icc_profile: Option<Arc<[u8]>>,
    /// 元画像の解像度。保存時に書き込む
    pub dpi: Option<Dpi>,
    /// 元画像のプレビュー画像 (sRGB の RGBA)。トリミング後のプレビューも UI 側で切り出して共用する
    ///
    /// UI はカスタムプロトコル経由で `preview_id` ごとに取得する
    pub preview: Option<Arc<RgbaImage>>,
    /// プレビュー画像の識別子。差し替えるたびに増やし、UI に再取得を促す
    pub preview_id: u64,
    /// 余白の塗り色の ICC 変換結果 (変換前の色, 変換後の色)。描画のたびにプロファイルを解析しないための保持
    pub preview_fill: Option<([u8; 4], [u8; 4])>,
    /// 直近のトリミング情報。切り抜き画像は保持せず、保存・コピー時にこの矩形で切り出す
    pub info: Option<TrimInfo>,
    /// 直近のトリミング実行時のトリミング設定。同じ値なら再実行をスキップする
    pub cache_key: Option<TrimParams>,
}

impl ImageState {
    /// 新しい画像読み込み前にプレビュー関連の状態をリセットする
    pub(in crate::app) fn prepare_for_load(&mut self) {
        self.preview = None;
        self.preview_fill = None;
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

    /// 元画像のプレビュー画像を差し替える
    ///
    /// # Arguments
    /// * `preview` - 新しいプレビュー画像
    pub(in crate::app) fn set_preview(&mut self, preview: RgbaImage) {
        self.preview = Some(Arc::new(preview));
        self.preview_id = self.preview_id.wrapping_add(1);
    }

    /// 画像とプレビューをすべてクリアする
    ///
    /// プレビュー画像の識別子は引き継ぎ、クリア前の画像を UI が取り違えないようにする
    pub(in crate::app) fn clear(&mut self) {
        let preview_id = self.preview_id.wrapping_add(1);
        *self = Self {
            preview_id,
            ..Self::default()
        };
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
