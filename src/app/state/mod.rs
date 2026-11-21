//! アプリケーションの状態管理
//!
//! `App` 本体と初期化・永続化をここに置き、状態の部分構造体は責務ごとのサブモジュール
//! (`settings` / `image_state` / `trim_state` / `io_state` / `batch_state` / `update_state`) に分ける。
//! `app` 配下からは従来どおり `state::` 直下の名前で参照できるよう再エクスポートする。

mod batch_state;
mod image_state;
mod io_state;
mod settings;
mod trim_state;
mod update_state;

use super::clipboard_worker::ClipboardWorkerHandle;
use super::load_worker::LoadWorkerHandle;
use super::shortcuts::ShortcutTracker;

pub(super) use self::batch_state::BatchState;
pub(super) use self::image_state::{BackgroundState, ImageState, PaneView};
pub(super) use self::io_state::{
    AutoSaveKey, DialogResult, IoMsg, IoState, SaveKind, SaveRequest, ScannedDrop,
};
pub(super) use self::settings::{SETTINGS_KEY, Settings};
pub(super) use self::trim_state::{TrimPreview, TrimWorkerResult, TrimWorkerState};
pub(super) use self::update_state::{UpdateState, UpdateUiPhase, UpdateWorkerMsg};

// ======================================================================
// アプリケーション状態
// ======================================================================
/// アプリケーションの状態
pub struct App {
    pub(super) image: ImageState,
    pub(super) background: BackgroundState,
    /// 永続化対象の設定
    pub(super) prefs: Settings,
    /// ステータスバーに表示するメッセージ
    pub(super) status: String,
    /// クリップボード操作用ワーカー。初回使用時に遅延起動する
    pub(super) clipboard: Option<ClipboardWorkerHandle>,
    /// 画像読み込み用ワーカー。初回使用時に遅延起動する
    pub(super) loader: Option<LoadWorkerHandle>,
    pub(super) batch: BatchState,
    pub(super) trim_worker: TrimWorkerState,
    pub(super) io: IoState,
    /// キーボードショートカットの判定状態
    pub(super) shortcuts: ShortcutTracker,
    /// アプリ更新
    pub(super) update: UpdateState,
}

// ======================================================================
// 初期化・永続化
// ======================================================================
impl App {
    /// アプリケーションの初期状態を生成する
    ///
    /// # Arguments
    /// * `storage` - eframe の永続化ストレージ。保存済みの設定があれば復元する
    ///
    /// # Returns
    /// * `Self` - 初期化済みの `App`
    pub fn new(storage: Option<&dyn eframe::Storage>) -> Self {
        let s = storage
            .and_then(|st| eframe::get_value::<Settings>(st, SETTINGS_KEY))
            .unwrap_or_default()
            .normalize();
        Self {
            image: ImageState::default(),
            background: BackgroundState::restore_from_settings(s.bg_override),
            prefs: s,
            status: "「画像を開く」で画像を選択してください".into(),
            clipboard: None,
            loader: None,
            batch: BatchState::default(),
            trim_worker: TrimWorkerState::new(),
            io: IoState::default(),
            shortcuts: ShortcutTracker::default(),
            update: UpdateState::default(),
        }
    }

    /// 現在の設定をストレージに保存する
    pub(super) fn save_settings(&self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, SETTINGS_KEY, &self.prefs.clone().normalize());
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new(None)
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests;
