//! アプリケーションの状態管理

use std::env;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, mpsc};
use std::time::Instant;

use super::trim_worker::TrimWorkerHandle;
use crate::consts::{PADDING_MAX, PADDING_MIN, TOL_MAX, TOL_MIN};
use crate::trim::TrimInfo;

use eframe::egui;
use image::DynamicImage;

/// バックグラウンドのプレビュートリミング結果
pub(super) struct TrimWorkerResult {
    pub request_id: u64,
    pub trimmed: DynamicImage,
    pub trimmed_color: egui::ColorImage,
    pub info: TrimInfo,
    pub orig_color: Option<egui::ColorImage>,
    pub background: [u8; 4],
}

// ======================================================================
// 設定
// ======================================================================
/// 永続化する設定キー
pub(super) const SETTINGS_KEY: &str = "settings";

/// 永続化する設定項目
#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct Settings {
    pub tolerance: u32,
    pub padding: u32,
    pub dark_mode: bool,
    pub auto_save: bool,
    pub auto_save_dir: PathBuf,
    pub compact_mode: bool,
    /// 手動指定の背景色。`None` のとき四辺から自動検出する
    #[serde(default)]
    pub bg_override: Option<[u8; 4]>,
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

    /// 現在のアプリ状態から永続化用の設定を生成する
    fn from_app(app: &App) -> Self {
        Self {
            tolerance: app.prefs.tolerance,
            padding: app.prefs.padding,
            dark_mode: app.prefs.dark_mode,
            auto_save: app.prefs.auto_save,
            auto_save_dir: app.prefs.auto_save_dir.clone(),
            compact_mode: app.prefs.compact_mode,
            bg_override: app.background.bg_override,
        }
        .normalize()
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
/// 永続化対象の UI・トリミング設定
#[derive(Clone)]
pub(super) struct AppPreferences {
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
}

impl Default for AppPreferences {
    fn default() -> Self {
        let s = Settings::default();
        Self {
            tolerance: s.tolerance,
            padding: s.padding,
            dark_mode: s.dark_mode,
            auto_save: s.auto_save,
            auto_save_dir: s.auto_save_dir,
            compact_mode: s.compact_mode,
        }
    }
}

impl From<&Settings> for AppPreferences {
    fn from(s: &Settings) -> Self {
        Self {
            tolerance: s.tolerance,
            padding: s.padding,
            dark_mode: s.dark_mode,
            auto_save: s.auto_save,
            auto_save_dir: s.auto_save_dir.clone(),
            compact_mode: s.compact_mode,
        }
    }
}

/// 読み込み画像とプレビュー表示
#[derive(Default)]
pub(super) struct ImageState {
    /// 読み込んだ元画像
    pub original: Option<Arc<DynamicImage>>,
    /// トリミング後の画像
    pub trimmed: Option<DynamicImage>,
    /// 元画像の GPU テクスチャ
    pub orig_tex: Option<egui::TextureHandle>,
    /// トリミング後画像の GPU テクスチャ
    pub trim_tex: Option<egui::TextureHandle>,
    /// 直近のトリミング情報
    pub info: Option<TrimInfo>,
    /// 直近のトリミング実行時の `(tolerance, padding, bg_override)`。同じ値なら再実行をスキップする
    pub cache_key: Option<(u32, u32, Option<[u8; 4]>)>,
}

impl ImageState {
    /// 新しい画像読み込み前にプレビュー関連の状態をリセットする
    pub(super) fn prepare_for_load(&mut self) {
        self.orig_tex = None;
        self.trimmed = None;
        self.trim_tex = None;
        self.cache_key = None;
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
    /// 手動指定の背景色。`None` のとき四辺から自動検出する
    pub bg_override: Option<[u8; 4]>,
    /// 直近のトリミングで使用した背景色 (表示用)
    pub bg_color: Option<[u8; 4]>,
    /// スポイトモード: 元画像クリックで背景色を設定する
    pub pick_mode: bool,
}

impl BackgroundState {
    /// 保存済みの手動背景色から状態を復元する
    fn restore_from_settings(bg_override: Option<[u8; 4]>) -> Self {
        Self {
            bg_override,
            bg_color: bg_override,
            pick_mode: false,
        }
    }

    /// 画像読み込み後に表示色を手動指定へ同期し、スポイトを終了する
    pub(super) fn on_image_loaded(&mut self) {
        self.bg_color = self.bg_override;
        self.pick_mode = false;
    }

    /// 画像クリア後に表示色を手動指定へ戻し、スポイトを終了する
    pub(super) fn on_image_cleared(&mut self) {
        self.bg_color = self.bg_override;
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

/// 一括トリミングのバックグラウンド処理
pub(super) struct BatchState {
    /// バックグラウンドスレッドからの結果受信チャネル
    /// `(is_final, message)` — `is_final` が `true` のとき処理完了
    pub rx: Option<mpsc::Receiver<(bool, String)>>,
    /// 処理が実行中かどうか
    pub running: bool,
    /// キャンセルフラグ。`true` にセットするとスレッドが次のファイル処理前に停止する
    pub cancel: Arc<AtomicBool>,
}

impl Default for BatchState {
    fn default() -> Self {
        Self {
            rx: None,
            running: false,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
}

// ======================================================================
// アプリケーション状態
// ======================================================================
/// アプリケーションの状態
pub struct App {
    pub(super) image: ImageState,
    pub(super) background: BackgroundState,
    pub(super) prefs: AppPreferences,
    /// ステータスバーに表示するメッセージ
    pub(super) status: String,
    /// クリップボードアクセス用インスタンス。初回使用時に遅延初期化する
    pub(super) clipboard: Option<arboard::Clipboard>,
    pub(super) batch: BatchState,
    pub(super) trim_worker: TrimWorkerState,
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
            prefs: AppPreferences::from(&s),
            status: "「画像を開く」で画像を選択してください".into(),
            clipboard: None,
            batch: BatchState::default(),
            trim_worker: TrimWorkerState::new(),
        }
    }

    /// 現在の設定をストレージに保存する
    pub(super) fn save_settings(&self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, SETTINGS_KEY, &Settings::from_app(self));
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
    use super::*;

    /// `App::default()` が画像なしで初期化されることを確認する
    #[test]
    fn new_has_no_images() {
        let app = App::default();
        assert!(app.image.original.is_none());
        assert!(app.image.trimmed.is_none());
        assert!(app.image.orig_tex.is_none());
        assert!(app.image.trim_tex.is_none());
        assert!(app.image.info.is_none());
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

    /// `from_app` が範囲外の値を保存前に正規化することを確認する
    #[test]
    fn settings_from_app_clamps_out_of_range_values() {
        let app = App {
            prefs: AppPreferences {
                tolerance: 500,
                padding: 500,
                ..AppPreferences::default()
            },
            ..App::default()
        };
        let s = Settings::from_app(&app);
        assert_eq!(s.tolerance, TOL_MAX);
        assert_eq!(s.padding, PADDING_MAX);
    }

    /// デフォルト設定で手動背景色が未指定であることを確認する
    #[test]
    fn settings_default_bg_override_is_none() {
        let s = Settings::default();
        assert_eq!(s.bg_override, None);
    }

    /// `from_app` が手動背景色を永続化用設定へ含めることを確認する
    #[test]
    fn settings_from_app_includes_bg_override() {
        let manual = [10, 20, 30, 255];
        let app = App {
            background: BackgroundState {
                bg_override: Some(manual),
                ..BackgroundState::default()
            },
            ..App::default()
        };
        let s = Settings::from_app(&app);
        assert_eq!(s.bg_override, Some(manual));
    }

    /// 起動時に保存済みの手動背景色がアプリ状態へ復元されることを確認する
    #[test]
    fn app_new_restores_bg_override_from_settings() {
        use std::collections::HashMap;

        struct TestStorage(HashMap<String, String>);

        impl eframe::Storage for TestStorage {
            fn get_string(&self, key: &str) -> Option<String> {
                self.0.get(key).cloned()
            }

            fn set_string(&mut self, key: &str, value: String) {
                self.0.insert(key.to_owned(), value);
            }

            fn flush(&mut self) {}
        }

        let manual = [40, 50, 60, 200];
        let mut storage = TestStorage(HashMap::new());
        eframe::set_value(
            &mut storage,
            SETTINGS_KEY,
            &Settings {
                bg_override: Some(manual),
                ..Settings::default()
            },
        );

        let app = App::new(Some(&storage));
        assert_eq!(app.background.bg_override, Some(manual));
        assert_eq!(app.background.bg_color, Some(manual));
    }
}
