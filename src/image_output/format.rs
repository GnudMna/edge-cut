//! 出力形式 (`OutputFormat`) の定義

use std::io::BufReader;
use std::path::Path;

use image::{ImageFormat, ImageReader};
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
    /// `Original` は元ファイルの先頭バイトから判定した形式を維持する (読み込みと同じ判定)
    /// 拡張子が判定結果と同じ形式ならその拡張子 (小文字) を使い、食い違う場合は判定結果の
    /// 拡張子とする。ファイルを読めない・判定できない場合は元の拡張子を小文字にして使い、
    /// 拡張子がない・未対応の場合は `DEFAULT_IMAGE_EXTENSION` とする
    ///
    /// # Arguments
    /// * `self` - 出力形式
    /// * `source` - 元画像ファイルのパス
    ///
    /// # Returns
    /// * `String` - 出力ファイルの拡張子 (小文字、ドットなし)
    #[must_use]
    pub fn output_extension(self, source: &Path) -> String {
        self.resolve_extension(source, sniff_format(source))
    }

    /// 判定済みの元画像の形式を使って出力拡張子を返す
    ///
    /// `output_extension` と同じ規則だが、ファイルを開かず `detected` を中身の判定結果として使う
    /// 読み込み時に判定した形式を保存まで持ち回るときに使う
    ///
    /// # Arguments
    /// * `self` - 出力形式
    /// * `source` - 元画像のファイル名またはパス (拡張子の参照にだけ使う)
    /// * `detected` - 読み込み時に中身から判定した形式。判定できなかった場合は `None`
    ///
    /// # Returns
    /// * `String` - 出力ファイルの拡張子 (小文字、ドットなし)
    #[must_use]
    pub fn resolve_extension(self, source: &Path, detected: Option<Self>) -> String {
        if let Some(ext) = self.extensions().first() {
            return (*ext).into();
        }
        let ext = source
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .filter(|ext| is_supported_image_extension(ext));
        match (detected, ext) {
            (Some(detected), Some(ext)) if Self::from_extension(&ext) == Some(detected) => ext,
            (Some(detected), _) => detected.extensions()[0].into(),
            (None, Some(ext)) => ext,
            (None, None) => DEFAULT_IMAGE_EXTENSION.into(),
        }
    }

    /// `image` クレートの形式から出力可能な形式へ変換する
    ///
    /// # Arguments
    /// * `format` - 読み込み時に判定した形式
    ///
    /// # Returns
    /// * `Option<Self>` - 対応する出力形式。出力非対応の形式は `None`
    pub(crate) fn from_image_format(format: ImageFormat) -> Option<Self> {
        match format {
            ImageFormat::Png => Some(Self::Png),
            ImageFormat::Jpeg => Some(Self::Jpeg),
            ImageFormat::WebP => Some(Self::Webp),
            ImageFormat::Bmp => Some(Self::Bmp),
            ImageFormat::Tiff => Some(Self::Tiff),
            ImageFormat::Gif => Some(Self::Gif),
            _ => None,
        }
    }
}

/// ファイルの先頭バイトから出力可能な形式を判定する
///
/// # Arguments
/// * `source` - 画像ファイルのパス
///
/// # Returns
/// * `Option<OutputFormat>` - 判定できた形式。読めない・出力非対応の形式は `None`
fn sniff_format(source: &Path) -> Option<OutputFormat> {
    let file = std::fs::File::open(source).ok()?;
    let format = ImageReader::new(BufReader::new(file))
        .with_guessed_format()
        .ok()?
        .format()?;
    OutputFormat::from_image_format(format)
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

    /// `Original` は中身が PNG の `.jpg` を PNG として出力し、一致する拡張子は維持すること
    #[test]
    fn original_output_extension_follows_content() {
        let dir = std::env::temp_dir().join(format!("edge_cut_sniff_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create dir");
        let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 0]));
        let mismatched = dir.join("a.JPG");
        img.save_with_format(&mismatched, ImageFormat::Png)
            .expect("save png");
        let matched = dir.join("b.png");
        img.save_with_format(&matched, ImageFormat::Png)
            .expect("save png");

        // 中身と拡張子が食い違う場合は中身に従う
        assert_eq!(OutputFormat::Original.output_extension(&mismatched), "png");
        // 一致する場合は拡張子を維持する
        assert_eq!(OutputFormat::Original.output_extension(&matched), "png");
        // 存在しないファイルは拡張子に従う
        assert_eq!(
            OutputFormat::Original.output_extension(&dir.join("none.jpeg")),
            "jpeg"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 判定済みの形式を渡した場合はファイルを開かず、その形式に従うこと
    #[test]
    fn resolve_extension_uses_detected_format() {
        let jpg = Path::new("missing.JPG");
        let original = OutputFormat::Original;
        // 食い違う場合は判定結果、一致する場合は拡張子 (小文字)
        assert_eq!(
            original.resolve_extension(jpg, Some(OutputFormat::Png)),
            "png"
        );
        assert_eq!(
            original.resolve_extension(jpg, Some(OutputFormat::Jpeg)),
            "jpg"
        );
        // 判定できない場合は拡張子、拡張子もなければ既定
        assert_eq!(original.resolve_extension(jpg, None), "jpg");
        assert_eq!(
            original.resolve_extension(Path::new("clipboard"), None),
            DEFAULT_IMAGE_EXTENSION
        );
        // 形式を指定した場合は判定結果に関わらず指定に従う
        assert_eq!(
            OutputFormat::Webp.resolve_extension(jpg, Some(OutputFormat::Png)),
            "webp"
        );
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
