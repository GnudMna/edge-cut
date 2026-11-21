//! 画像保存時のエラー型

use std::error::Error;
use std::fmt;
use std::io;

use image::ImageError;

use crate::image_load::image_error_message;

// ======================================================================
// エラー型
// ======================================================================
/// 画像保存時のエラー
#[derive(Debug)]
pub enum SaveError {
    /// 形式の判定またはエンコードのエラー
    Encode(ImageError),
    /// ファイルの作成・書き込みエラー
    Io(io::Error),
}

impl From<ImageError> for SaveError {
    fn from(value: ImageError) -> Self {
        Self::Encode(value)
    }
}

impl From<io::Error> for SaveError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Encode(e) => write!(f, "保存エラー: {}", image_error_message(e)),
            Self::Io(e) => write!(f, "保存エラー: {e}"),
        }
    }
}

impl Error for SaveError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Encode(e) => Some(e),
            Self::Io(e) => Some(e),
        }
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    /// `SaveError` が元の `ImageError` を `source()` で返すこと
    #[test]
    fn save_error_source_returns_encode_inner() {
        use std::error::Error;

        use image::ImageError;
        use image::error::{ParameterError, ParameterErrorKind};

        let inner = ImageError::Parameter(ParameterError::from_kind(
            ParameterErrorKind::DimensionMismatch,
        ));
        let err = super::SaveError::Encode(inner);
        assert!(Error::source(&err).is_some());
    }
}
