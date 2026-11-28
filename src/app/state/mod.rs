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

use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::clipboard_worker::ClipboardWorkerHandle;
use super::load_worker::LoadWorkerHandle;
use super::waker::Waker;

pub(super) use self::batch_state::{BatchState, CloseConfirm};
pub(super) use self::image_state::{BackgroundState, ImageState};
pub(super) use self::io_state::{
    AutoSaveKey, DialogResult, IoMsg, IoState, SaveKind, SaveRequest, ScannedDrop,
};
pub(super) use self::settings::{
    PANEL_WIDTH_MAX, PANEL_WIDTH_MIN, PreviewMode, Settings, ThemeMode, WindowGeometry,
};
pub(super) use self::trim_state::{TrimPreview, TrimWorkerResult, TrimWorkerState};
pub(super) use self::update_state::{UpdateState, UpdateUiPhase, UpdateWorkerMsg};

// ======================================================================
// 終了時の待機
// ======================================================================
/// 終了時にワーカースレッドの完了を待つ上限
///
/// 保存先の応答がない場合でも、非表示のウィンドウの裏でプロセスが残り続けないようにする
const SHUTDOWN_JOIN_TIMEOUT: Duration = Duration::from_secs(10);

/// 終了時のポーリング間隔
const SHUTDOWN_POLL_INTERVAL: Duration = Duration::from_millis(20);

/// スレッドの完了を期限付きで待つ
///
/// 期限を過ぎた場合はスレッドを切り離して待つのをやめる (プロセス終了時に打ち切られる)。
/// 期限は呼び出し側で共有でき、複数のスレッドを待っても合計が期限を超えない。
///
/// # Arguments
/// * `handle` - 待つスレッド
/// * `deadline` - 待つのをやめる時刻
///
/// # Returns
/// * `bool` - 期限内に完了したか
fn join_with_timeout(handle: JoinHandle<()>, deadline: Instant) -> bool {
    while !handle.is_finished() {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(SHUTDOWN_POLL_INTERVAL);
    }
    let _ = handle.join();
    true
}

/// 終了時の待機期限を `BatchState` と `IoState` で共有するための入れ物
///
/// 最初に破棄された側が期限を 1 つだけ決め、後から破棄される側も同じ期限を使う。
/// `App` に `Drop` を実装すると構造体更新構文が使えなくなるため、フィールド側で共有する。
pub(super) type SharedDeadline = Arc<OnceLock<Instant>>;

/// 共有された期限を返す (未設定なら現在時刻から `SHUTDOWN_JOIN_TIMEOUT` 後に決める)
///
/// # Arguments
/// * `shared` - 共有された期限
///
/// # Returns
/// * `Instant` - 待つのをやめる時刻
fn shutdown_deadline(shared: &SharedDeadline) -> Instant {
    *shared.get_or_init(|| Instant::now() + SHUTDOWN_JOIN_TIMEOUT)
}

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
    /// アプリ更新
    pub(super) update: UpdateState,
    /// ワーカースレッドの完了時にイベントループを起こす通知
    pub(super) waker: Waker,
    /// ウィンドウを閉じる要求 (更新の適用後など)。イベントループが確認して閉じる
    pub(super) close_requested: bool,
}

// ======================================================================
// 初期化・永続化
// ======================================================================
impl App {
    /// アプリケーションの初期状態を生成する
    ///
    /// # Arguments
    /// * `prefs` - 保存済みの設定 (範囲外の値はここで正規化する)
    /// * `waker` - ワーカースレッドの完了時にイベントループを起こす通知
    ///
    /// # Returns
    /// * `Self` - 初期化済みの `App`
    pub(super) fn new(prefs: Settings, waker: Waker) -> Self {
        let s = prefs.normalize();
        // 破棄順 (batch → io) をまたいで 1 つの期限を共有する
        let batch = BatchState::default();
        let mut io = IoState::default();
        io.shutdown_deadline = Arc::clone(&batch.shutdown_deadline);
        Self {
            image: ImageState::default(),
            background: BackgroundState::default(),
            prefs: s,
            status: "「画像を開く」で画像を選択してください".into(),
            clipboard: None,
            loader: None,
            batch,
            trim_worker: TrimWorkerState::new(),
            io,
            update: UpdateState::default(),
            waker,
            close_requested: false,
        }
    }

    /// 保存する設定を返す (範囲外の値は正規化する)
    ///
    /// # Returns
    /// * `Settings` - ファイルへ書き出す設定
    pub(super) fn settings_to_save(&self) -> Settings {
        self.prefs.clone().normalize()
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new(Settings::default(), Waker::noop())
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests;
