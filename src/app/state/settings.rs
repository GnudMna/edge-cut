//! 永続化する設定

use std::path::PathBuf;

use crate::app::save_dir::default_auto_save_dir;
use crate::consts::{
    JPEG_QUALITY_DEFAULT, JPEG_QUALITY_MAX, JPEG_QUALITY_MIN, PADDING_MAX, PADDING_MIN, TOL_MAX,
    TOL_MIN,
};
use crate::image_output::OutputFormat;

// ======================================================================
// 設定
// ======================================================================
/// 永続化する設定キー
pub(in crate::app) const SETTINGS_KEY: &str = "settings";

/// 永続化対象の UI・トリミング設定
///
/// アプリ状態 (`App::prefs`) としてそのまま保持し、終了時に正規化して保存する
///
/// 保存データに無いフィールドは `Default` の値で補う。フィールドの追加・削除・改名があっても
/// 残りの設定を読み込める
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(in crate::app) struct Settings {
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
    pub(super) fn normalize(mut self) -> Self {
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
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

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
    /// デフォルト設定で手動背景色が未指定であること
    #[test]
    fn settings_default_bg_override_is_none() {
        let s = Settings::default();
        assert_eq!(s.bg_override, None);
    }
}
