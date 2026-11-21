//! アプリケーションの状態管理
use crate::trim::TrimInfo;

use eframe::egui;
use image::DynamicImage;

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
    pub(super) auto_save_dir: std::path::PathBuf,
    /// コンパクト UI モードの有効/無効
    pub(super) compact_mode: bool,
}

impl App {
    /// アプリケーションの初期状態を生成する
    ///
    /// # Returns
    /// * `Self` - 初期化済みの `App`
    pub fn new() -> Self {
        Self {
            original: None,
            trimmed: None,
            orig_tex: None,
            trim_tex: None,
            tolerance: 0,
            padding: 0,
            status: "「画像を開く」で画像を選択してください".into(),
            info: None,
            dark_mode: true,
            auto_save: false,
            auto_save_dir: std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.join("output")))
                .unwrap_or_else(|| std::path::PathBuf::from("output")),
            compact_mode: false,
        }
    }
}
