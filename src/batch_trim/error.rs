//! 一括トリミングのエラー型

use std::error::Error;
use std::fmt;

use crate::image_load::LoadError;

// ======================================================================
// エラー型
// ======================================================================
/// 一括トリミングで 1 ファイルを処理する際のエラー
#[derive(Debug)]
pub enum BatchFileError {
    /// 入力画像の読み込みエラー
    Load(LoadError),
    /// 出力画像の保存エラー
    Save(crate::image_output::SaveError),
    /// 処理中に panic した (メッセージは panic の内容)
    Panic(String),
}

impl From<LoadError> for BatchFileError {
    fn from(value: LoadError) -> Self {
        Self::Load(value)
    }
}

impl From<crate::image_output::SaveError> for BatchFileError {
    fn from(value: crate::image_output::SaveError) -> Self {
        Self::Save(value)
    }
}

impl fmt::Display for BatchFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Load(e) => e.fmt(f),
            Self::Save(e) => e.fmt(f),
            Self::Panic(msg) => write!(f, "処理中に内部エラーが発生: {msg}"),
        }
    }
}

impl Error for BatchFileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Load(e) => Some(e),
            Self::Save(e) => Some(e),
            Self::Panic(_) => None,
        }
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// `BatchFileError` が内包する読み込みエラーを `source()` で返すこと
    #[test]
    fn batch_file_error_source_chains_load_error() {
        use std::error::Error;
        use std::io;

        use crate::image_load::LoadError;

        let inner = io::Error::new(io::ErrorKind::NotFound, "missing");
        let err = BatchFileError::Load(LoadError::Io(inner));
        assert!(Error::source(&err).is_some());
    }
}
