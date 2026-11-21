//! ファイルへの画像保存
//!
//! 上書き保存と、既存ファイルを上書きしない連番保存を提供する。

use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use image::{DynamicImage, ImageFormat};

use super::encode::encode_image;
use super::error::SaveError;
use super::format::OutputFormat;
use super::path::trimmed_file_stem;
use super::temp_file::TempFile;
use crate::consts::{
    JPEG_QUALITY_DEFAULT, JPEG_QUALITY_MAX, JPEG_QUALITY_MIN, OUTPUT_SUFFIX_MAX,
    WEBP_QUALITY_DEFAULT, WEBP_QUALITY_MAX, WEBP_QUALITY_MIN,
};
use crate::dpi::Dpi;

// ======================================================================
// 保存オプション
// ======================================================================
/// 保存時のエンコード設定
///
/// `Default` は既定値 (JPEG 品質 `JPEG_QUALITY_DEFAULT`、WebP 品質 `WEBP_QUALITY_DEFAULT`) を表す
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveOptions {
    /// JPEG のエンコード品質。`JPEG_QUALITY_MIN` 〜 `JPEG_QUALITY_MAX` に丸めて使う
    pub jpeg_quality: u8,
    /// WebP のエンコード品質。`WEBP_QUALITY_MIN` 〜 `WEBP_QUALITY_MAX` に丸めて使う
    ///
    /// `WEBP_QUALITY_MAX` のときだけロスレスで保存する
    pub webp_quality: u8,
}

impl Default for SaveOptions {
    fn default() -> Self {
        Self {
            jpeg_quality: JPEG_QUALITY_DEFAULT,
            webp_quality: WEBP_QUALITY_DEFAULT,
        }
    }
}

impl SaveOptions {
    /// JPEG 品質を有効範囲に丸めた値
    pub(super) fn clamped_jpeg_quality(self) -> u8 {
        self.jpeg_quality.clamp(JPEG_QUALITY_MIN, JPEG_QUALITY_MAX)
    }

    /// WebP 品質を有効範囲に丸めた値
    pub(super) fn clamped_webp_quality(self) -> u8 {
        self.webp_quality.clamp(WEBP_QUALITY_MIN, WEBP_QUALITY_MAX)
    }
}

// ======================================================================
// 画像保存
// ======================================================================
/// 画像をファイルへ保存する (既存ファイルは上書き)
///
/// 既定のエンコード設定 (`SaveOptions::default`) で `save_image_with` を呼ぶ
///
/// # Arguments
/// * `img`         - 保存する画像
/// * `icc_profile` - 埋め込む ICC プロファイル (元画像から引き継いだもの)
/// * `dpi`         - 書き込む解像度 (元画像から引き継いだもの。扱いは `save_image_with` と同じ)
/// * `path`        - 保存先パス
///
/// # Errors
/// 拡張子から形式を判定できない、エンコードまたは書き込みに失敗した場合
pub fn save_image(
    img: &DynamicImage,
    icc_profile: Option<&[u8]>,
    dpi: Option<Dpi>,
    path: &Path,
) -> Result<(), SaveError> {
    save_image_with(img, icc_profile, dpi, path, SaveOptions::default())
}

/// 画像をエンコード設定を指定してファイルへ保存する (既存ファイルは上書き)
///
/// 形式は拡張子から判定する。JPEG は透過を `JPEG_FLATTEN_BACKGROUND` に合成して
/// RGB 化し、品質 `options.jpeg_quality` でエンコードする
/// WebP は品質 `options.webp_quality` でエンコードし、最大値のときだけロスレスにする
/// ICC プロファイルは PNG / JPEG / TIFF / WebP に埋め込む。BMP / GIF など非対応の形式や、
/// プロファイルの色空間が出力画像と合わない場合は埋め込まない
/// 解像度は PNG / JPEG / TIFF に書き込む。それ以外の形式では書き込まない
/// エンコードをメモリ上で済ませ、同じディレクトリの一時ファイルへ書き込んでから置き換えるため、
/// エンコードや書き込みに失敗しても既存ファイルを壊さない
///
/// # Arguments
/// * `img`         - 保存する画像
/// * `icc_profile` - 埋め込む ICC プロファイル (元画像から引き継いだもの)
/// * `dpi`         - 書き込む解像度 (元画像から引き継いだもの。扱いは `save_image_with` と同じ)
/// * `path`        - 保存先パス
/// * `options`     - エンコード設定
///
/// # Errors
/// 拡張子から形式を判定できない、エンコードまたは書き込みに失敗した場合
pub fn save_image_with(
    img: &DynamicImage,
    icc_profile: Option<&[u8]>,
    dpi: Option<Dpi>,
    path: &Path,
    options: SaveOptions,
) -> Result<(), SaveError> {
    let bytes = encode_image(
        img,
        icc_profile,
        dpi,
        ImageFormat::from_path(path)?,
        options,
    )?;
    TempFile::write(path, &bytes)?.replace(path)?;
    Ok(())
}

/// 画像を `out_dir` に新規ファイルとして保存する (既存ファイルは上書きしない)
///
/// 既定のエンコード設定 (`SaveOptions::default`) で `save_image_unique_with` を呼ぶ
///
/// # Arguments
/// * `img`         - 保存する画像
/// * `icc_profile` - 埋め込む ICC プロファイル (扱いは `save_image_with` と同じ)
/// * `dpi`         - 書き込む解像度 (元画像から引き継いだもの。扱いは `save_image_with` と同じ)
/// * `out_dir`     - 出力先ディレクトリ (作成済みであること)
/// * `stem`        - 拡張子を除くファイル名
/// * `ext`         - 拡張子 (ドットなし。形式の判定にも使う)
///
/// # Returns
/// * `(PathBuf, bool)` - 保存したパスと、連番サフィックスで回避したかどうか
///
/// # Errors
/// 形式を判定できない、エンコードまたは書き込みに失敗した場合
pub fn save_image_unique(
    img: &DynamicImage,
    icc_profile: Option<&[u8]>,
    dpi: Option<Dpi>,
    out_dir: &Path,
    stem: &str,
    ext: &str,
) -> Result<(PathBuf, bool), SaveError> {
    save_image_unique_with(
        img,
        icc_profile,
        dpi,
        out_dir,
        stem,
        ext,
        SaveOptions::default(),
    )
}

/// 画像をエンコード設定を指定して `out_dir` に新規ファイルとして保存する (既存ファイルは上書きしない)
///
/// `{stem}.{ext}` が既に存在する場合は `{stem}_2.{ext}`, `_3`, ... と連番を付ける
/// 一時ファイルへ書き込んでから、存在確認と作成を同時に行う方法で公開するため、
/// 並行する保存とも衝突せず、中断しても書きかけのファイルが保存先に残らない
///
/// # Arguments
/// * `img`         - 保存する画像
/// * `icc_profile` - 埋め込む ICC プロファイル (扱いは `save_image_with` と同じ)
/// * `dpi`         - 書き込む解像度 (元画像から引き継いだもの。扱いは `save_image_with` と同じ)
/// * `out_dir`     - 出力先ディレクトリ (作成済みであること)
/// * `stem`        - 拡張子を除くファイル名
/// * `ext`         - 拡張子 (ドットなし。形式の判定にも使う)
/// * `options`     - エンコード設定
///
/// # Returns
/// * `(PathBuf, bool)` - 保存したパスと、連番サフィックスで回避したかどうか
///
/// # Errors
/// 形式を判定できない、エンコードまたは書き込みに失敗した場合
pub fn save_image_unique_with(
    img: &DynamicImage,
    icc_profile: Option<&[u8]>,
    dpi: Option<Dpi>,
    out_dir: &Path,
    stem: &str,
    ext: &str,
    options: SaveOptions,
) -> Result<(PathBuf, bool), SaveError> {
    let base = out_dir.join(format!("{stem}.{ext}"));
    let bytes = encode_image(
        img,
        icc_profile,
        dpi,
        ImageFormat::from_path(&base)?,
        options,
    )?;
    // 書き込みは 1 回だけ行い、空いている名前が見つかるまで公開を試す
    let mut temp = TempFile::write(&base, &bytes)?;

    if temp.persist_new(&base)? {
        return Ok((base, false));
    }
    for n in 2..=OUTPUT_SUFFIX_MAX {
        let candidate = out_dir.join(format!("{stem}_{n}.{ext}"));
        if temp.persist_new(&candidate)? {
            return Ok((candidate, true));
        }
    }

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let fallback = out_dir.join(format!("{stem}_{ts}.{ext}"));
    if temp.persist_new(&fallback)? {
        return Ok((fallback, true));
    }
    Err(io::Error::from(io::ErrorKind::AlreadyExists).into())
}

/// トリミング結果を `out_dir` に新規ファイルとして保存する
///
/// 既定のエンコード設定 (`SaveOptions::default`) で `save_trimmed_output_with` を呼ぶ
///
/// # Arguments
/// * `img`         - トリミング後の画像
/// * `icc_profile` - 元画像の ICC プロファイル (扱いは `save_image_with` と同じ)
/// * `dpi`         - 書き込む解像度 (元画像から引き継いだもの。扱いは `save_image_with` と同じ)
/// * `source`      - 元画像ファイルのパス (名前と `Original` 時の形式に使う)
/// * `out_dir`     - 出力先ディレクトリ (作成済みであること)
/// * `format`      - 出力形式
///
/// # Returns
/// * `(PathBuf, bool)` - 保存したパスと、連番サフィックスで回避したかどうか
///
/// # Errors
/// エンコードまたは書き込みに失敗した場合
pub fn save_trimmed_output(
    img: &DynamicImage,
    icc_profile: Option<&[u8]>,
    dpi: Option<Dpi>,
    source: &Path,
    out_dir: &Path,
    format: OutputFormat,
) -> Result<(PathBuf, bool), SaveError> {
    save_trimmed_output_with(
        img,
        icc_profile,
        dpi,
        source,
        out_dir,
        format,
        SaveOptions::default(),
    )
}

/// トリミング結果をエンコード設定を指定して `out_dir` に新規ファイルとして保存する
///
/// ファイル名は `trimmed_file_name` と同じ規則で、衝突時は連番サフィックスを付ける
/// 自動保存と一括トリミングで共通に使う
///
/// # Arguments
/// * `img`         - トリミング後の画像
/// * `icc_profile` - 元画像の ICC プロファイル (扱いは `save_image_with` と同じ)
/// * `dpi`         - 書き込む解像度 (元画像から引き継いだもの。扱いは `save_image_with` と同じ)
/// * `source`      - 元画像ファイルのパス (名前と `Original` 時の形式に使う)
/// * `out_dir`     - 出力先ディレクトリ (作成済みであること)
/// * `format`      - 出力形式
/// * `options`     - エンコード設定
///
/// # Returns
/// * `(PathBuf, bool)` - 保存したパスと、連番サフィックスで回避したかどうか
///
/// # Errors
/// エンコードまたは書き込みに失敗した場合
pub fn save_trimmed_output_with(
    img: &DynamicImage,
    icc_profile: Option<&[u8]>,
    dpi: Option<Dpi>,
    source: &Path,
    out_dir: &Path,
    format: OutputFormat,
    options: SaveOptions,
) -> Result<(PathBuf, bool), SaveError> {
    let stem = trimmed_file_stem(source);
    save_image_unique_with(
        img,
        icc_profile,
        dpi,
        out_dir,
        &stem,
        &format.output_extension(source),
        options,
    )
}
