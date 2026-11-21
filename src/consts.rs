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

/// 許容度のスケール係数の分母 (許容度 0–100 を 0–MAX_RGB_SQ_DIST へ変換する際に使用)
pub const TOL_SCALE_DENOM: u64 = 10_000;

/// 許容度の最小値
pub const TOL_MIN: u32 = 0;

/// 許容度の最大値
pub const TOL_MAX: u32 = 100;

/// 余白ピクセル数の最小値
pub const PADDING_MIN: u32 = 0;

/// 余白ピクセル数の最大値
pub const PADDING_MAX: u32 = 100;

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
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

    /// MAX_RGB_SQ_DIST が 255^2 * 3 であることを確認する
    #[test]
    fn max_rgb_sq_dist_is_correct() {
        assert_eq!(MAX_RGB_SQ_DIST, 255u64 * 255 * 3);
    }
}
