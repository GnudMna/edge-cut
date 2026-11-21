//! アプリケーションの状態管理
use std::env;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, mpsc};

use crate::trim::TrimInfo;

use eframe::egui;
use image::DynamicImage;

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
        }
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
// アプリケーション状態
// ======================================================================
/// アプリケーションの状態
pub struct App {
    /// 読み込んだ元画像
    pub(super) original: Option<DynamicImage>,
    /// トリミング後の画像
    pub(super) trimmed: Option<DynamicImage>,
    /// 元画像の GPU テクスチャ
    pub(super) orig_tex: Option<egui::TextureHandle>,
    /// トリミング後画像の GPU テクスチャ
    pub(super) trim_tex: Option<egui::TextureHandle>,
    /// 余白検出の許容度 (0–100)
    pub(super) tolerance: u32,
    /// トリミング後に残す余白ピクセル数
    pub(super) padding: u32,
    /// ステータスバーに表示するメッセージ
    pub(super) status: String,
    /// 直近のトリミング情報
    pub(super) info: Option<TrimInfo>,
    /// ダークモードの有効/無効
    pub(super) dark_mode: bool,
    /// 自動保存モードの有効/無効
    pub(super) auto_save: bool,
    /// 自動保存先ディレクトリ
    pub(super) auto_save_dir: PathBuf,
    /// コンパクト UI モードの有効/無効
    pub(super) compact_mode: bool,
    /// 直近のトリミング実行時の (tolerance, padding)。同じ値なら再実行をスキップする
    pub(super) trim_cache_key: Option<(u32, u32)>,
    /// クリップボードアクセス用インスタンス。初回使用時に遅延初期化する
    pub(super) clipboard: Option<arboard::Clipboard>,
    /// 一括トリミングのバックグラウンドスレッドからの結果受信チャネル
    /// `(is_final, message)` — `is_final` が `true` のとき処理完了
    pub(super) batch_rx: Option<mpsc::Receiver<(bool, String)>>,
    /// 一括トリミング処理が実行中かどうか
    pub(super) batch_running: bool,
    /// 一括トリミングキャンセルフラグ。`true` にセットするとスレッドが次のファイル処理前に停止する
    pub(super) batch_cancel: Arc<AtomicBool>,
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
            .unwrap_or_default();
        Self {
            original: None,
            trimmed: None,
            orig_tex: None,
            trim_tex: None,
            tolerance: s.tolerance,
            padding: s.padding,
            status: "「画像を開く」で画像を選択してください".into(),
            info: None,
            dark_mode: s.dark_mode,
            auto_save: s.auto_save,
            auto_save_dir: s.auto_save_dir,
            compact_mode: s.compact_mode,
            trim_cache_key: None,
            clipboard: None,
            batch_rx: None,
            batch_running: false,
            batch_cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 現在の設定をストレージに保存する
    pub(super) fn save_settings(&self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(
            storage,
            SETTINGS_KEY,
            &Settings {
                tolerance: self.tolerance,
                padding: self.padding,
                dark_mode: self.dark_mode,
                auto_save: self.auto_save,
                auto_save_dir: self.auto_save_dir.clone(),
                compact_mode: self.compact_mode,
            },
        );
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
        assert!(app.original.is_none());
        assert!(app.trimmed.is_none());
        assert!(app.orig_tex.is_none());
        assert!(app.trim_tex.is_none());
        assert!(app.info.is_none());
    }

    /// `App::default()` が許容度と余白をゼロで初期化することを確認する
    #[test]
    fn new_default_tolerance_and_padding_are_zero() {
        let app = App::default();
        assert_eq!(app.tolerance, 0);
        assert_eq!(app.padding, 0);
    }

    /// `App::default()` がステータスメッセージを適切に初期化することを確認する
    #[test]
    fn new_dark_mode_enabled_by_default() {
        let app = App::default();
        assert!(app.dark_mode);
    }

    /// `App::default()` が自動保存モードをデフォルトで無効にすることを確認する
    #[test]
    fn new_auto_save_disabled_by_default() {
        let app = App::default();
        assert!(!app.auto_save);
    }

    /// `App::default()` がコンパクトモードをデフォルトで無効にすることを確認する
    #[test]
    fn new_compact_mode_disabled_by_default() {
        let app = App::default();
        assert!(!app.compact_mode);
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
        assert!(!app.auto_save_dir.as_os_str().is_empty());
    }
}
