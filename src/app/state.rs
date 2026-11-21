//! アプリケーションの状態管理

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::Instant;

use eframe::egui;
use image::DynamicImage;

use super::clipboard_worker::ClipboardWorkerHandle;
use super::load_worker::LoadWorkerHandle;
use super::save_dir::default_auto_save_dir;
use super::shortcuts::ShortcutTracker;
use super::trim_worker::TrimWorkerHandle;
use crate::consts::{
    JPEG_QUALITY_DEFAULT, JPEG_QUALITY_MAX, JPEG_QUALITY_MIN, PADDING_MAX, PADDING_MIN, TOL_MAX,
    TOL_MIN,
};
use crate::image_load::LoadedImage;
use crate::image_output::OutputFormat;
use crate::trim::TrimInfo;

/// トリミング結果を決めるパラメータ `(tolerance, padding, bg_override)`
pub(super) type TrimParams = (u32, u32, Option<[u8; 4]>);

/// バックグラウンドのプレビュートリミング結果
pub(super) struct TrimWorkerResult {
    pub request_id: u64,
    /// 検出結果。処理中に panic した場合は `Err(panic メッセージ)`
    pub outcome: Result<TrimPreview, String>,
}

/// プレビュートリミングの検出結果
pub(super) struct TrimPreview {
    /// このジョブで使ったパラメータ。受信時点の UI の値とは異なりうる
    pub params: TrimParams,
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
///
/// 保存データに無いフィールドは `Default` の値で補う。フィールドの追加・削除・改名があっても
/// 残りの設定を読み込める
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
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
    pub bg_override: Option<[u8; 4]>,
    /// 保存・自動保存・一括トリミングの出力形式
    pub output_format: OutputFormat,
    /// JPEG 保存時のエンコード品質 (1–100)
    pub jpeg_quality: u8,
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
            jpeg_quality: JPEG_QUALITY_DEFAULT,
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
        self.jpeg_quality = self.jpeg_quality.clamp(JPEG_QUALITY_MIN, JPEG_QUALITY_MAX);
        if self.auto_save_dir.as_os_str().is_empty() {
            self.auto_save_dir = default_auto_save_dir();
        }
        self
    }
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
    pub cache_key: Option<TrimParams>,
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
    /// 元画像テクスチャも再生成するか。再生成したテクスチャを反映するまで `true` のまま
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

    /// ワーカースレッドを起動し直し、処理中のジョブを打ち切る
    ///
    /// ワーカーが終了して結果チャネルが切断された場合に呼ぶ
    pub(super) fn restart(&mut self) {
        let (result_tx, result_rx) = mpsc::channel();
        self.handle = TrimWorkerHandle::spawn(result_tx);
        self.result_rx = result_rx;
        self.in_flight = false;
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
    /// クリップボードからファイルのパスを取得した
    PastedFiles {
        /// 読み込み要求 ID。古い結果を無視するために使用する
        request_id: u64,
        /// コピーされていたファイル・フォルダのパス
        paths: Vec<PathBuf>,
    },
    /// ファイルダイアログが閉じられた
    DialogClosed(DialogResult),
}

/// 手動保存の対象。ダイアログを開いた時点の内容を保持する
pub(super) struct SaveRequest {
    /// 元画像
    pub original: Arc<DynamicImage>,
    /// 保存するトリミング矩形
    pub info: TrimInfo,
    /// 元画像の ICC プロファイル
    pub icc_profile: Option<Arc<[u8]>>,
    /// 拡張子なしで入力された場合に補う拡張子
    pub ext: String,
}

/// ファイルダイアログの結果
///
/// キャンセルされた場合は `None` を持つ
pub(super) enum DialogResult {
    /// 開く画像の選択
    OpenImage(Option<PathBuf>),
    /// 手動保存の保存先の選択
    SaveImage {
        /// 保存対象
        request: SaveRequest,
        /// 選択された保存先
        path: Option<PathBuf>,
    },
    /// 一括トリミングの対象ファイルの選択
    BatchInputs(Option<Vec<PathBuf>>),
    /// 一括トリミングの保存先フォルダの選択
    BatchOutput {
        /// トリミング対象の画像ファイル
        paths: Vec<PathBuf>,
        /// 呼び出し側で対象外として除外した件数
        skipped: usize,
        /// 選択された保存先フォルダ
        dir: Option<PathBuf>,
    },
    /// 自動保存先フォルダの選択
    AutoSaveDir(Option<PathBuf>),
    /// ダイアログ処理スレッドの panic (ステータスメッセージ)
    Failed(String),
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
    /// 直近に自動保存を開始した内容。同じ結果を連番ファイルとして重複保存しないために使う
    pub last_auto_save: Option<AutoSaveKey>,
    /// 自動保存の追記前のステータスと追記後のステータス
    ///
    /// 表示中のステータスが追記後のままなら追記前に戻してから追記し、追記が重ならないようにする
    pub auto_save_status: Option<(String, String)>,
    /// ファイルダイアログを表示中かどうか。同時に 1 つだけ開く
    pub dialog_open: bool,
}

/// 自動保存の内容を識別するキー
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AutoSaveKey {
    /// 保存元画像の読み込み要求 ID
    pub load_request_id: u64,
    /// 保存したトリミング矩形
    pub info: TrimInfo,
    /// 出力形式
    pub format: OutputFormat,
    /// JPEG のエンコード品質
    pub jpeg_quality: u8,
    /// 保存先ディレクトリ
    pub dir: PathBuf,
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
            last_auto_save: None,
            auto_save_status: None,
            dialog_open: false,
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
    /// 実行中の一括トリミングスレッド。終了時に完了を待つ
    pub handle: Option<JoinHandle<()>>,
    /// 直近バッチのエラー詳細 (ステータスバーは要約のみ、全文はエラー一覧ウィンドウで表示)
    pub last_error_details: Vec<String>,
    /// エラー一覧ウィンドウを表示中かどうか
    pub errors_open: bool,
    /// 対象外として除外した入力ファイル数 (完了時の要約に追記する)
    pub skipped_inputs: usize,
}

impl Default for BatchState {
    fn default() -> Self {
        Self {
            rx: None,
            running: false,
            cancel: Arc::new(AtomicBool::new(false)),
            handle: None,
            last_error_details: Vec::new(),
            errors_open: false,
            skipped_inputs: 0,
        }
    }
}

impl Drop for BatchState {
    /// 未着手のファイルを打ち切り、処理中のファイルの保存完了を待つ
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            self.cancel.store(true, Ordering::Relaxed);
            let _ = handle.join();
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
    /// 画像読み込み用ワーカー。初回使用時に遅延起動する
    pub(super) loader: Option<LoadWorkerHandle>,
    pub(super) batch: BatchState,
    pub(super) trim_worker: TrimWorkerState,
    pub(super) io: IoState,
    /// キーボードショートカットの判定状態
    pub(super) shortcuts: ShortcutTracker,
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

    use eframe::Storage;

    use super::*;

    /// `App::default()` が画像なしで初期化されること
    #[test]
    fn new_has_no_images() {
        let app = App::default();
        assert!(app.image.original.is_none());
        assert!(app.image.orig_tex.is_none());
        assert!(app.image.info.is_none());
        assert!(app.image.trim_target().is_none());
    }

    /// `App::default()` が許容度と余白をゼロで初期化すること
    #[test]
    fn new_default_tolerance_and_padding_are_zero() {
        let app = App::default();
        assert_eq!(app.prefs.tolerance, 0);
        assert_eq!(app.prefs.padding, 0);
    }

    /// `App::default()` がステータスメッセージを適切に初期化すること
    #[test]
    fn new_dark_mode_enabled_by_default() {
        let app = App::default();
        assert!(app.prefs.dark_mode);
    }

    /// `App::default()` が自動保存モードをデフォルトで無効にすること
    #[test]
    fn new_auto_save_disabled_by_default() {
        let app = App::default();
        assert!(!app.prefs.auto_save);
    }

    /// `App::default()` がコンパクトモードをデフォルトで無効にすること
    #[test]
    fn new_compact_mode_disabled_by_default() {
        let app = App::default();
        assert!(!app.prefs.compact_mode);
    }

    /// `App::default()` がステータスメッセージを適切に初期化すること
    #[test]
    fn new_status_message_is_not_empty() {
        let app = App::default();
        assert!(!app.status.is_empty());
    }

    /// `App::default()` が自動保存ディレクトリを適切に初期化すること
    #[test]
    fn new_auto_save_dir_is_non_empty_path() {
        let app = App::default();
        assert!(!app.prefs.auto_save_dir.as_os_str().is_empty());
    }

    /// 範囲外の許容度が読み込み時にクランプされること
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

    /// 範囲外の余白が読み込み時にクランプされること
    #[test]
    fn settings_normalize_clamps_padding() {
        let s = Settings {
            padding: 200,
            ..Settings::default()
        }
        .normalize();
        assert_eq!(s.padding, PADDING_MAX);
    }

    /// 範囲外の JPEG 品質が読み込み時にクランプされること
    #[test]
    fn settings_normalize_clamps_jpeg_quality() {
        for (input, expected) in [(0, JPEG_QUALITY_MIN), (200, JPEG_QUALITY_MAX), (80, 80)] {
            let s = Settings {
                jpeg_quality: input,
                ..Settings::default()
            }
            .normalize();
            assert_eq!(s.jpeg_quality, expected);
        }
    }

    /// 空の自動保存先がデフォルトパスへ置き換わること
    #[test]
    fn settings_normalize_empty_auto_save_dir_uses_default() {
        let s = Settings {
            auto_save_dir: PathBuf::new(),
            ..Settings::default()
        }
        .normalize();
        assert!(!s.auto_save_dir.as_os_str().is_empty());
    }

    /// `save_settings` が範囲外の値を保存前に正規化すること
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

    /// デフォルト設定で手動背景色が未指定であること
    #[test]
    fn settings_default_bg_override_is_none() {
        let s = Settings::default();
        assert_eq!(s.bg_override, None);
    }

    /// `save_settings` が手動背景色を保存すること
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

    /// 起動時に保存済みの手動背景色がアプリ状態へ復元されること
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

    /// 保存データに一部のフィールドしかなくても、残りを既定値で補って読み込めること
    #[test]
    fn app_new_fills_missing_settings_fields_with_defaults() {
        let mut storage = TestStorage::default();
        storage.set_string(
            SETTINGS_KEY,
            "(tolerance: 42, compact_mode: true)".to_owned(),
        );

        let app = App::new(Some(&storage));
        let defaults = Settings::default();
        // 保存データにあるフィールドは復元される
        assert_eq!(app.prefs.tolerance, 42);
        assert!(app.prefs.compact_mode);
        // 無いフィールドは既定値になる
        assert_eq!(app.prefs.padding, defaults.padding);
        assert_eq!(app.prefs.dark_mode, defaults.dark_mode);
        assert_eq!(app.prefs.jpeg_quality, JPEG_QUALITY_DEFAULT);
    }

    /// 保存データに未知のフィールドがあっても、他の設定を読み込めること
    #[test]
    fn app_new_ignores_unknown_settings_fields() {
        let mut storage = TestStorage::default();
        storage.set_string(SETTINGS_KEY, "(tolerance: 7, removed_field: 1)".to_owned());

        let app = App::new(Some(&storage));
        assert_eq!(app.prefs.tolerance, 7);
    }

    // ======================================================================
    // 一括トリミング
    // ======================================================================
    /// 破棄時にキャンセルフラグを立て、一括トリミングスレッドの終了を待つこと
    #[test]
    fn batch_state_drop_cancels_and_joins_thread() {
        let mut batch = BatchState::default();
        let cancel = Arc::clone(&batch.cancel);
        let finished = Arc::new(AtomicBool::new(false));
        let finished_in_thread = Arc::clone(&finished);
        // キャンセルされるまで処理を続けるスレッドを模す
        batch.handle = Some(std::thread::spawn(move || {
            while !cancel.load(Ordering::Relaxed) {
                std::thread::yield_now();
            }
            finished_in_thread.store(true, Ordering::Relaxed);
        }));

        drop(batch);
        assert!(finished.load(Ordering::Relaxed));
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
