//! 全体で共通の定数を提供

// ======================================================================
// アプリケーション情報
// ======================================================================
/// アプリケーションの表示名
pub const APP_NAME: &str = "EdgeCut";

// ======================================================================
// ウインドウサイズ
// ======================================================================
/// ウインドウのデフォルトの横幅
pub const DEFAULT_WINDOW_WIDTH: f32 = 1200.0;

/// ウインドウのデフォルトの縦幅
pub const DEFAULT_WINDOW_HEIGHT: f32 = 720.0;

/// ウインドウのデフォルトサイズ
pub const DEFAULT_WINDOW_SIZE: [f32; 2] = [DEFAULT_WINDOW_WIDTH, DEFAULT_WINDOW_HEIGHT];

/// ウインドウの最小の横幅
pub const MIN_WINDOW_WIDTH: f32 = 600.0;

/// ウインドウの最小の縦幅
pub const MIN_WINDOW_HEIGHT: f32 = 300.0;

/// ウインドウの最小サイズ
pub const MIN_WINDOW_SIZE: [f32; 2] = [MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT];

// ======================================================================
// トリミング
// ======================================================================
/// アルファ値がこの値未満のピクセルはほぼ完全に透明とみなし背景として扱う
pub const ALPHA_TRANSPARENT_THRESHOLD: u8 = 16;

/// RGB 各チャネルの差の二乗和の最大値 (255^2 * 3 = 195,075)
/// 許容度 100 のときにすべての色を背景とみなす基準値
pub const MAX_RGB_SQ_DIST: u64 = 195_075;

/// 許容度のスケール係数の分母 (許容度 0–100 を `0–MAX_RGB_SQ_DIST` へ変換する際に使用)
pub const TOL_SCALE_DENOM: u64 = 10_000;

/// 背景色検出で各チャネルを量子化する右シフト量 (3 のときビン幅 8 階調)
///
/// 隣接ビンも合算するため、おおむね ±8〜15 階調の揺らぎを同一クラスタとして扱う
pub const BG_DETECT_QUANT_SHIFT: u32 = 3;

/// 許容度の最小値
pub const TOL_MIN: u32 = 0;

/// 許容度の最大値
pub const TOL_MAX: u32 = 100;

/// 余白ピクセル数の最小値
pub const PADDING_MIN: u32 = 0;

/// 余白ピクセル数の最大値
pub const PADDING_MAX: u32 = 100;

/// スライダー操作時にプレビュートリミングを再実行するまでの待機時間 (ms)
pub const TRIM_DEBOUNCE_MS: u64 = 150;

/// このピクセル数未満の画像ではトリミング走査を逐次実行する (rayon オーバーヘッド回避)
pub const TRIM_PARALLEL_MIN_PIXELS: u64 = 512 * 512;

// ======================================================================
// プレビュー表示
// ======================================================================
/// プレビュー用テクスチャの長辺上限 (px)。超える画像は縮小して GPU 転送する
pub const PREVIEW_MAX_DIMENSION: u32 = 4096;

// ======================================================================
// 画像フォーマット
// ======================================================================
/// 対応する画像ファイル拡張子 (小文字、ドットなし)
pub const SUPPORTED_IMAGE_EXTENSIONS: &[&str] =
    &["png", "jpg", "jpeg", "bmp", "gif", "tiff", "tif", "webp"];

/// 拡張子が未対応・欠落時の出力フォールバック
pub const DEFAULT_IMAGE_EXTENSION: &str = "png";

/// JPEG 保存時のエンコード品質 (1–100)
pub const JPEG_QUALITY: u8 = 95;

/// JPEG 保存時に透過ピクセルを合成する背景色 (RGB)
pub const JPEG_FLATTEN_BACKGROUND: [u8; 3] = [255, 255, 255];

/// 出力ファイル名の衝突時に付ける連番サフィックスの上限 (`_2` 〜 `_{N}`)
pub const OUTPUT_SUFFIX_MAX: u32 = 10_000;

/// 拡張子文字列が対応形式かどうか (`ext` は小文字想定)
#[must_use]
pub fn is_supported_image_extension(ext: &str) -> bool {
    SUPPORTED_IMAGE_EXTENSIONS.contains(&ext)
}

// ======================================================================
// 画像読み込み上限
// ======================================================================
/// 読み込みを許可する画像ファイルの最大サイズ (バイト)
pub const MAX_IMAGE_FILE_BYTES: u64 = 256 * 1024 * 1024;

/// 読み込みを許可する画像の最大幅 (px)
pub const MAX_IMAGE_WIDTH: u32 = 16_384;

/// 読み込みを許可する画像の最大高さ (px)
pub const MAX_IMAGE_HEIGHT: u32 = 16_384;

/// 読み込みを許可する最大ピクセル数 (約 64 MP、8000×8000 相当)
pub const MAX_IMAGE_PIXELS: u64 = 64_000_000;

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
#[allow(clippy::assertions_on_constants, clippy::float_cmp)]
mod tests {
    use super::*;

    /// アプリケーション名が空でないことを確認する
    #[test]
    fn app_name_is_not_empty() {
        assert!(!APP_NAME.is_empty());
    }

    /// デフォルトのウインドウサイズが最小サイズより大きいことを確認する
    #[test]
    fn default_window_size_larger_than_minimum() {
        assert!(DEFAULT_WINDOW_WIDTH > MIN_WINDOW_WIDTH);
        assert!(DEFAULT_WINDOW_HEIGHT > MIN_WINDOW_HEIGHT);
    }

    /// 最小ウインドウサイズが正の値であることを確認する
    #[test]
    fn minimum_window_size_is_positive() {
        assert!(MIN_WINDOW_WIDTH > 0.0);
        assert!(MIN_WINDOW_HEIGHT > 0.0);
    }

    /// デフォルトのウインドウサイズ配列が個別のコンポーネントと一致することを確認する
    #[test]
    fn default_window_size_array_matches_components() {
        assert_eq!(DEFAULT_WINDOW_SIZE[0], DEFAULT_WINDOW_WIDTH);
        assert_eq!(DEFAULT_WINDOW_SIZE[1], DEFAULT_WINDOW_HEIGHT);
    }

    /// 最小ウインドウサイズ配列が個別のコンポーネントと一致することを確認する
    #[test]
    fn min_window_size_array_matches_components() {
        assert_eq!(MIN_WINDOW_SIZE[0], MIN_WINDOW_WIDTH);
        assert_eq!(MIN_WINDOW_SIZE[1], MIN_WINDOW_HEIGHT);
    }

    /// 許容度の範囲が正しいことを確認する
    #[test]
    fn tolerance_range_is_valid() {
        assert!(TOL_MIN < TOL_MAX);
    }

    /// 余白ピクセル数の範囲が正しいことを確認する
    #[test]
    fn padding_range_is_valid() {
        assert!(PADDING_MIN < PADDING_MAX);
    }

    /// `MAX_RGB_SQ_DIST` が 255^2 * 3 であることを確認する
    #[test]
    fn max_rgb_sq_dist_is_correct() {
        assert_eq!(MAX_RGB_SQ_DIST, 255u64 * 255 * 3);
    }

    /// JPEG 品質が有効範囲内であることを確認する
    #[test]
    fn jpeg_quality_is_in_valid_range() {
        assert!((1..=100).contains(&JPEG_QUALITY));
    }

    /// 画像読み込み上限が正の値であることを確認する
    #[test]
    fn image_load_limits_are_positive() {
        assert!(MAX_IMAGE_FILE_BYTES > 0);
        assert!(MAX_IMAGE_WIDTH > 0);
        assert!(MAX_IMAGE_HEIGHT > 0);
        assert!(MAX_IMAGE_PIXELS > 0);
    }

    /// 最大ピクセル数が幅・高さの積上限内に収まることを確認する
    #[test]
    fn max_pixels_fits_within_dimension_box() {
        let box_pixels = u64::from(MAX_IMAGE_WIDTH) * u64::from(MAX_IMAGE_HEIGHT);
        assert!(MAX_IMAGE_PIXELS <= box_pixels);
    }

    /// 対応拡張子リストが空でなく、代表形式を含むことを確認する
    #[test]
    fn supported_image_extensions_include_common_formats() {
        assert!(!SUPPORTED_IMAGE_EXTENSIONS.is_empty());
        assert!(is_supported_image_extension("png"));
        assert!(is_supported_image_extension("jpeg"));
        assert!(is_supported_image_extension("tiff"));
        assert!(is_supported_image_extension("tif"));
        assert!(!is_supported_image_extension("pdf"));
    }
}
