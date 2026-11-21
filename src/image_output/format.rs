//! 出力形式 (`OutputFormat`) の定義

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::consts::{DEFAULT_IMAGE_EXTENSION, is_supported_image_extension};

// ======================================================================
// 出力形式
// ======================================================================
/// トリミング結果の出力形式
///
/// 自動保存・一括トリミングの保存形式と、手動保存ダイアログの既定値に使う
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum OutputFormat {
    /// PNG
    Png,
    /// JPEG (透過は `JPEG_FLATTEN_BACKGROUND` に合成する)
    Jpeg,
    /// WebP (品質が最大のときだけロスレス)
    Webp,
    /// BMP
    Bmp,
    /// TIFF
    Tiff,
    /// GIF
    Gif,
    /// 元画像の形式を維持する。拡張子がない・未対応の場合は PNG
    ///
    /// 未知の値を読み込んだ場合もこの値とする
    #[default]
    #[serde(other)]
    Original,
}

impl OutputFormat {
    /// 選択肢として表示する順序の全形式
    pub const ALL: [Self; 7] = [
        Self::Original,
        Self::Png,
        Self::Jpeg,
        Self::Webp,
        Self::Bmp,
        Self::Tiff,
        Self::Gif,
    ];

    /// UI に表示する形式名を返す
    ///
    /// # Arguments
    /// * `self` - 出力形式
    ///
    /// # Returns
    /// * `&'static str` - 表示名 (例: `PNG`)
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Original => "元の形式",
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Webp => "WebP",
            Self::Bmp => "BMP",
            Self::Tiff => "TIFF",
            Self::Gif => "GIF",
        }
    }

    /// 形式に対応する拡張子を返す
    ///
    /// `Original` は固有の拡張子を持たないため空を返す
    ///
    /// # Arguments
    /// * `self` - 出力形式
    ///
    /// # Returns
    /// * `&'static [&'static str]` - 拡張子 (小文字、ドットなし) の一覧。先頭が出力時の拡張子
    #[must_use]
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Original => &[],
            Self::Png => &["png"],
            Self::Jpeg => &["jpg", "jpeg"],
            Self::Webp => &["webp"],
            Self::Bmp => &["bmp"],
            Self::Tiff => &["tiff", "tif"],
            Self::Gif => &["gif"],
        }
    }

    /// 拡張子から形式を判定する (大文字小文字は区別しない)
    ///
    /// # Arguments
    /// * `ext` - ドットを含まない拡張子
    ///
    /// # Returns
    /// * `Option<Self>` - 対応形式なら `Some`。`Original` を返すことはない
    #[must_use]
    pub fn from_extension(ext: &str) -> Option<Self> {
        let ext = ext.to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|f| f.extensions().contains(&ext.as_str()))
    }

    /// 元画像をこの形式で出力するときの拡張子を返す
    ///
    /// `Original` は元の拡張子を小文字にして維持し、拡張子がない・未対応の場合は
    /// `DEFAULT_IMAGE_EXTENSION` とする
    ///
    /// # Arguments
    /// * `self` - 出力形式
    /// * `source` - 元画像ファイルのパス
    ///
    /// # Returns
    /// * `String` - 出力ファイルの拡張子 (小文字、ドットなし)
    #[must_use]
    pub fn output_extension(self, source: &Path) -> String {
        if let Some(ext) = self.extensions().first() {
            return (*ext).into();
        }
        source
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .filter(|ext| is_supported_image_extension(ext))
            .unwrap_or_else(|| DEFAULT_IMAGE_EXTENSION.into())
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// 拡張子から形式を大文字小文字を区別せず判定し、未対応は `None` とすること
    #[test]
    fn output_format_from_extension() {
        assert_eq!(OutputFormat::from_extension("PNG"), Some(OutputFormat::Png));
        assert_eq!(
            OutputFormat::from_extension("jpeg"),
            Some(OutputFormat::Jpeg)
        );
        assert_eq!(
            OutputFormat::from_extension("tif"),
            Some(OutputFormat::Tiff)
        );
        assert_eq!(OutputFormat::from_extension("pdf"), None);
        assert_eq!(OutputFormat::from_extension(""), None);
    }

    /// 全形式の出力拡張子が対応拡張子であり、`from_extension` で同じ形式に戻ること
    #[test]
    fn output_format_extensions_round_trip() {
        for format in OutputFormat::ALL {
            if format == OutputFormat::Original {
                continue;
            }
            let ext = format.output_extension(Path::new("a.png"));
            assert!(is_supported_image_extension(&ext), "{format:?}: {ext}");
            assert_eq!(OutputFormat::from_extension(&ext), Some(format));
        }
    }

    /// 未知の保存値は `Original` として読み込むこと
    #[test]
    fn output_format_unknown_value_deserializes_to_original() {
        let value: OutputFormat =
            serde::de::Deserialize::deserialize(serde::de::value::StrDeserializer::<
                serde::de::value::Error,
            >::new("Avif"))
            .expect("deserialize");
        assert_eq!(value, OutputFormat::Original);
    }
}
