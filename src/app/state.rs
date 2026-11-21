//! アプリケーションの状態管理

use std::env;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::Instant;

use super::clipboard_worker::ClipboardWorkerHandle;
use super::trim_worker::TrimWorkerHandle;
use crate::consts::{PADDING_MAX, PADDING_MIN, TOL_MAX, TOL_MIN};
use crate::image_load::LoadedImage;
use crate::image_output::OutputFormat;
use crate::trim::TrimInfo;

use eframe::egui;
use image::DynamicImage;

/// バックグラウンドのプレビュートリミング結果
pub(super) struct TrimWorkerResult {
    pub request_id: u64,
    pub info: TrimInfo,
    pub orig_color: Option<egui::ColorImage>,
    pub background: [u8; 4],
}

// ======================================================================
// 設定
// ======================================================================
/// 永続化する設定キー
pub(super) const SETTINGS_KEY: &str = "settings";

/// 永続化対象の UI・トリミング設定
///
/// アプリ状態 (`App::prefs`) としてそのまま保持し、終了時に正規化して保存する
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct Settings {
    /// 余白検出の許容度 (0–100)
    pub tolerance: u32,
    /// トリミング後に残す余白ピクセル数
    pub padding: u32,
    /// ダークモードの有効/無効
    pub dark_mode: bool,
    /// 自動保存モードの有効/無効
    pub auto_save: bool,
    /// 自動保存先ディレクトリ
    pub auto_save_dir: PathBuf,
    /// コンパクト UI モードの有効/無効
    pub compact_mode: bool,
    /// 手動指定の背景色。`None` のとき四辺から自動検出する
    #[serde(default)]
    pub bg_override: Option<[u8; 4]>,
    /// 保存・自動保存・一括トリミングの出力形式
    #[serde(default)]
    pub output_format: OutputFormat,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            tolerance: 0,
            padding: 0,
            dark_mode: true,
            auto_save: false,
            auto_save_dir: default_auto_save_dir(),
            compact_mode: false,
            bg_override: None,
            output_format: OutputFormat::default(),
        }
    }
}

impl Settings {
    /// 永続化データを有効範囲へ正規化する
    ///
    /// 手動編集や将来のバージョン差分で範囲外の値が保存されていても、
    /// 読み込み時にクランプして安全に復元する
    fn normalize(mut self) -> Self {
        self.tolerance = self.tolerance.clamp(TOL_MIN, TOL_MAX);
        self.padding = self.padding.clamp(PADDING_MIN, PADDING_MAX);
        if self.auto_save_dir.as_os_str().is_empty() {
            self.auto_save_dir = default_auto_save_dir();
        }
        self
    }
}

/// 自動保存先デフォルトパスを決定する
fn default_auto_save_dir() -> PathBuf {
    env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(|h| PathBuf::from(h).join("Pictures").join("EdgeCut"))
        .or_else(|| {
            env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.join("output")))
        })
        .unwrap_or_else(|| PathBuf::from("output"))
}

// ======================================================================
// 状態の部分構造体
// ======================================================================
/// 読み込み画像とプレビュー表示
#[derive(Default)]
pub(super) struct ImageState {
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
    /// 直近のトリミング情報。切り抜き画像は保持せず、保存・コピー時にこの矩形で切り出す
    pub info: Option<TrimInfo>,
    /// 直近のトリミング実行時の `(tolerance, padding, bg_override)`。同じ値なら再実行をスキップする
    pub cache_key: Option<(u32, u32, Option<[u8; 4]>)>,
}

impl ImageState {
    /// 新しい画像読み込み前にプレビュー関連の状態をリセットする
    pub(super) fn prepare_for_load(&mut self) {
        self.orig_tex = None;
        self.info = None;
        self.cache_key = None;
    }

    /// 保存・コピー対象の元画像とトリミング矩形を返す
    ///
    /// # Returns
    /// * `Option<(Arc<DynamicImage>, TrimInfo)>` - トリミング済みなら元画像と矩形、
    ///   画像未読み込みまたはトリミング未完了なら `None`
    pub(super) fn trim_target(&self) -> Option<(Arc<DynamicImage>, TrimInfo)> {
        Some((Arc::clone(self.original.as_ref()?), self.info?))
    }

    /// トリミングパラメータ変更時にキャッシュを無効化する
    pub(super) fn invalidate_cache(&mut self) {
        self.cache_key = None;
    }

    /// 画像とプレビューをすべてクリアする
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }
}

/// 背景色の手動指定と表示
#[derive(Default)]
pub(super) struct BackgroundState {
    /// 直近のトリミングで使用した背景色 (表示用)
    pub bg_color: Option<[u8; 4]>,
    /// スポイトモード: 元画像クリックで背景色を設定する
    pub pick_mode: bool,
}

impl BackgroundState {
    /// 保存済みの手動背景色から状態を復元する
    fn restore_from_settings(bg_override: Option<[u8; 4]>) -> Self {
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
    pub(super) fn reset_to_override(&mut self, bg_override: Option<[u8; 4]>) {
        self.bg_color = bg_override;
        self.pick_mode = false;
    }
}

/// プレビュートリミングのバックグラウンド処理
pub(super) struct TrimWorkerState {
    /// 専用ワーカースレッドへのジョブ送信
    pub(super) handle: TrimWorkerHandle,
    /// ワーカーからの結果受信チャネル
    pub(super) result_rx: mpsc::Receiver<TrimWorkerResult>,
    /// 要求 ID。古い結果を無視するために使用する
    pub request_id: u64,
    /// バックグラウンドで実行中かどうか
    pub in_flight: bool,
    /// デバウンス後にトリミングを開始する時刻
    pub debounce_until: Option<Instant>,
    /// 次回完了後に元画像テクスチャも再生成するか
    pub refresh_orig_tex: bool,
    /// 次回完了後に自動保存するか
    pub auto_save_after: bool,
}

impl TrimWorkerState {
    /// 専用ワーカースレッド付きの初期状態を生成する
    pub(super) fn new() -> Self {
        let (result_tx, result_rx) = mpsc::channel();
        Self {
            handle: TrimWorkerHandle::spawn(result_tx),
            result_rx,
            request_id: 0,
            in_flight: false,
            debounce_until: None,
            refresh_orig_tex: false,
            auto_save_after: false,
        }
    }

    /// プレビュートリミングが進行中かどうか
    pub(super) fn is_busy(&self) -> bool {
        self.in_flight || self.debounce_until.is_some()
    }

    /// 進行中の処理を打ち切り、古い結果を無視する
    pub(super) fn abort(&mut self) {
        self.request_id = self.request_id.wrapping_add(1);
        self.in_flight = false;
        self.debounce_until = None;
        self.refresh_orig_tex = false;
        self.auto_save_after = false;
    }
}

/// 保存処理の種別
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SaveKind {
    /// ファイルダイアログからの手動保存
    Manual,
    /// 保存先フォルダへの自動保存
    Auto,
}

/// 画像ファイル I/O ワーカーからの通知
pub(super) enum IoMsg {
    /// 画像読み込みの完了
    Loaded {
        /// 読み込み要求 ID。古い結果を無視するために使用する
        request_id: u64,
        /// ステータス表示用のファイル名
        file_name: String,
        /// 読み込んだ画像、または失敗時のステータスメッセージ
        result: Result<LoadedImage, String>,
    },
    /// 画像保存の完了
    Saved {
        /// 保存処理の種別
        kind: SaveKind,
        /// 保存先パス、または失敗時のステータスメッセージ
        result: Result<PathBuf, String>,
    },
    /// クリップボードへのコピーの完了
    Copied {
        /// 成功、または失敗時のステータスメッセージ
        result: Result<(), String>,
    },
    /// クリップボードからの貼り付けの完了
    Pasted {
        /// 読み込み要求 ID。古い結果を無視するために使用する
        request_id: u64,
        /// 取得した画像、または失敗時のステータスメッセージ
        result: Result<DynamicImage, String>,
    },
}

/// 画像の読み込み・保存のバックグラウンド処理
///
/// 破棄時は実行中の保存スレッドの完了を待ち、書きかけのファイルが残らないようにする
pub(super) struct IoState {
    /// ワーカースレッドへ渡す送信側
    pub tx: mpsc::Sender<IoMsg>,
    /// ワーカースレッドからの結果受信チャネル
    pub rx: mpsc::Receiver<IoMsg>,
    /// 読み込み要求 ID。新しい読み込み・貼り付け・クリアで更新する
    pub load_request_id: u64,
    /// 実行中の保存スレッド
    pub save_handles: Vec<JoinHandle<()>>,
    /// 読み込み完了時のステータスに追記する補足 (ドロップ時の除外件数など)
    pub load_note: Option<String>,
}

impl Default for IoState {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            tx,
            rx,
            load_request_id: 0,
            save_handles: Vec::new(),
            load_note: None,
        }
    }
}

impl IoState {
    /// 進行中の読み込みを打ち切り、古い結果を無視する
    pub(super) fn abort_load(&mut self) {
        self.load_request_id = self.load_request_id.wrapping_add(1);
        self.load_note = None;
    }

    /// 保存が進行中かどうか
    pub(super) fn is_saving(&self) -> bool {
        !self.save_handles.is_empty()
    }
}

impl Drop for IoState {
    fn drop(&mut self) {
        for handle in self.save_handles.drain(..) {
            let _ = handle.join();
        }
    }
}

/// 一括トリミングのバックグラウンド処理
pub(super) struct BatchState {
    /// バックグラウンドスレッドからの進捗受信チャネル
    pub rx: Option<std::sync::mpsc::Receiver<crate::batch_trim::BatchProgress>>,
    /// 処理が実行中かどうか
    pub running: bool,
    /// キャンセルフラグ。`true` にセットすると次のファイル処理前に停止する
    pub cancel: Arc<AtomicBool>,
    /// 直近バッチのエラー詳細 (ステータスバーは要約のみ、ホバーで全文表示)
    pub last_error_details: Vec<String>,
    /// 対象外として除外した入力ファイル数 (完了時の要約に追記する)
    pub skipped_inputs: usize,
}

impl Default for BatchState {
    fn default() -> Self {
        Self {
            rx: None,
            running: false,
            cancel: Arc::new(AtomicBool::new(false)),
            last_error_details: Vec::new(),
            skipped_inputs: 0,
        }
    }
}

// ======================================================================
// 更新確認 (Windows)
// ======================================================================
/// 更新 UI の状態
#[cfg(windows)]
#[derive(Debug, Clone, Default)]
pub(super) enum UpdateUiPhase {
    /// 未確認
    #[default]
    Idle,
    /// version API 照会中
    Checking,
    /// 最新版
    UpToDate {
        /// 現在バージョン
        current_version: String,
    },
    /// 更新あり
    Available(crate::update::AvailableUpdate),
    /// MSI ダウンロード・検証中
    Downloading {
        /// 確認済みの更新情報
        update: crate::update::AvailableUpdate,
    },
    /// 失敗
    Error {
        /// ユーザー向けメッセージ
        message: String,
    },
    /// MSI を起動済み (アプリ終了待ち)
    MsiStarted,
}

/// バックグラウンド更新処理の結果
#[cfg(windows)]
pub(super) enum UpdateWorkerMsg {
    /// 更新確認の結果
    Check(crate::update::UpdateCheckOutcome),
    /// 更新適用の結果
    Apply(crate::update::UpdateApplyOutcome),
}

/// アプリ更新の UI / ワーカー状態
#[cfg(windows)]
#[derive(Default)]
pub(super) struct UpdateState {
    /// 現在の UI フェーズ
    pub phase: UpdateUiPhase,
    /// バックグラウンドからの結果受信
    pub rx: Option<mpsc::Receiver<UpdateWorkerMsg>>,
    /// 更新ダイアログを表示中か
    pub dialog_open: bool,
}

#[cfg(windows)]
impl UpdateState {
    /// 確認または適用が進行中か
    pub(super) fn is_busy(&self) -> bool {
        matches!(
            self.phase,
            UpdateUiPhase::Checking | UpdateUiPhase::Downloading { .. }
        )
    }
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
    pub(super) batch: BatchState,
    pub(super) trim_worker: TrimWorkerState,
    pub(super) io: IoState,
    /// アプリ更新 (Windows のみ)
    #[cfg(windows)]
    pub(super) update: UpdateState,
}

// ======================================================================
// 初期化・永続化
// ======================================================================
impl App {
    /// アプリケーションの初期状態を生成する
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
            batch: BatchState::default(),
            trim_worker: TrimWorkerState::new(),
            io: IoState::default(),
            #[cfg(windows)]
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
mod tests {
    use std::collections::HashMap;

    use super::*;

    /// `App::default()` が画像なしで初期化されることを確認する
    #[test]
    fn new_has_no_images() {
        let app = App::default();
        assert!(app.image.original.is_none());
        assert!(app.image.orig_tex.is_none());
        assert!(app.image.info.is_none());
        assert!(app.image.trim_target().is_none());
    }

    /// `App::default()` が許容度と余白をゼロで初期化することを確認する
    #[test]
    fn new_default_tolerance_and_padding_are_zero() {
        let app = App::default();
        assert_eq!(app.prefs.tolerance, 0);
        assert_eq!(app.prefs.padding, 0);
    }

    /// `App::default()` がステータスメッセージを適切に初期化することを確認する
    #[test]
    fn new_dark_mode_enabled_by_default() {
        let app = App::default();
        assert!(app.prefs.dark_mode);
    }

    /// `App::default()` が自動保存モードをデフォルトで無効にすることを確認する
    #[test]
    fn new_auto_save_disabled_by_default() {
        let app = App::default();
        assert!(!app.prefs.auto_save);
    }

    /// `App::default()` がコンパクトモードをデフォルトで無効にすることを確認する
    #[test]
    fn new_compact_mode_disabled_by_default() {
        let app = App::default();
        assert!(!app.prefs.compact_mode);
    }

    /// `App::default()` がステータスメッセージを適切に初期化することを確認する
    #[test]
    fn new_status_message_is_not_empty() {
        let app = App::default();
        assert!(!app.status.is_empty());
    }

    /// `App::default()` が自動保存ディレクトリを適切に初期化することを確認する
    #[test]
    fn new_auto_save_dir_is_non_empty_path() {
        let app = App::default();
        assert!(!app.prefs.auto_save_dir.as_os_str().is_empty());
    }

    /// 範囲外の許容度が読み込み時にクランプされることを確認する
    #[test]
    fn settings_normalize_clamps_tolerance() {
        let s = Settings {
            tolerance: 999,
            ..Settings::default()
        }
        .normalize();
        assert_eq!(s.tolerance, TOL_MAX);

        let s = Settings {
            tolerance: TOL_MAX + 1,
            ..Settings::default()
        }
        .normalize();
        assert_eq!(s.tolerance, TOL_MAX);
    }

    /// 範囲外の余白が読み込み時にクランプされることを確認する
    #[test]
    fn settings_normalize_clamps_padding() {
        let s = Settings {
            padding: 200,
            ..Settings::default()
        }
        .normalize();
        assert_eq!(s.padding, PADDING_MAX);
    }

    /// 空の自動保存先がデフォルトパスへ置き換わることを確認する
    #[test]
    fn settings_normalize_empty_auto_save_dir_uses_default() {
        let s = Settings {
            auto_save_dir: PathBuf::new(),
            ..Settings::default()
        }
        .normalize();
        assert!(!s.auto_save_dir.as_os_str().is_empty());
    }

    /// `save_settings` が範囲外の値を保存前に正規化することを確認する
    #[test]
    fn save_settings_clamps_out_of_range_values() {
        let app = App {
            prefs: Settings {
                tolerance: 500,
                padding: 500,
                ..Settings::default()
            },
            ..App::default()
        };
        let s = save_and_load(&app);
        assert_eq!(s.tolerance, TOL_MAX);
        assert_eq!(s.padding, PADDING_MAX);
    }

    /// デフォルト設定で手動背景色が未指定であることを確認する
    #[test]
    fn settings_default_bg_override_is_none() {
        let s = Settings::default();
        assert_eq!(s.bg_override, None);
    }

    /// `save_settings` が手動背景色を保存することを確認する
    #[test]
    fn save_settings_includes_bg_override() {
        let manual = [10, 20, 30, 255];
        let app = App {
            prefs: Settings {
                bg_override: Some(manual),
                ..Settings::default()
            },
            ..App::default()
        };
        let s = save_and_load(&app);
        assert_eq!(s.bg_override, Some(manual));
    }

    /// 起動時に保存済みの手動背景色がアプリ状態へ復元されることを確認する
    #[test]
    fn app_new_restores_bg_override_from_settings() {
        let manual = [40, 50, 60, 200];
        let mut storage = TestStorage::default();
        eframe::set_value(
            &mut storage,
            SETTINGS_KEY,
            &Settings {
                bg_override: Some(manual),
                ..Settings::default()
            },
        );

        let app = App::new(Some(&storage));
        assert_eq!(app.prefs.bg_override, Some(manual));
        assert_eq!(app.background.bg_color, Some(manual));
    }

    // ======================================================================
    // ヘルパー
    // ======================================================================
    /// メモリ上のテスト用ストレージ
    #[derive(Default)]
    struct TestStorage(HashMap<String, String>);

    impl eframe::Storage for TestStorage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }

        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.to_owned(), value);
        }

        fn remove_string(&mut self, key: &str) {
            self.0.remove(key);
        }

        fn flush(&mut self) {}
    }

    /// `save_settings` で保存した設定をそのまま読み戻す
    fn save_and_load(app: &App) -> Settings {
        let mut storage = TestStorage::default();
        app.save_settings(&mut storage);
        eframe::get_value(&storage, SETTINGS_KEY).expect("保存済みの設定")
    }
}
