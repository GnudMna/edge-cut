//! タイトルバーの色設定 (Windows 11 専用)
//!
//! OS 標準のタイトルバーの背景色と文字色を、アプリのテーマ色へ合わせる
//! Win32 API の呼び出しを含むため、このモジュールだけ `unsafe_code` を許可する

#![expect(unsafe_code, reason = "Win32 API の呼び出しに unsafe が必要なため")]

use std::ffi::c_void;

use eframe::egui;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DwmSetWindowAttribute,
};

/// `COLORREF` のバイト数
const COLORREF_SIZE: u32 = u32::BITS / 8;

// ======================================================================
// タイトルバー色の適用
// ======================================================================
/// タイトルバーの色をテーマに合わせて設定する
///
/// 背景色はパネルの塗りつぶし色、文字色は通常のテキスト色に揃える
/// Windows 10 など非対応の環境では、API の失敗を無視して標準の色のままにする
///
/// # Arguments
/// * `window` - タイトルバーを持つウィンドウ
/// * `dark_mode` - ダークテーマか
pub fn apply(window: &impl HasWindowHandle, dark_mode: bool) {
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return;
    };
    let hwnd = win32.hwnd.get() as HWND;

    let visuals = super::theme::visuals(dark_mode);
    set_color(hwnd, DWMWA_CAPTION_COLOR, visuals.panel_fill);
    set_color(hwnd, DWMWA_TEXT_COLOR, visuals.text_color());
}

/// `DwmSetWindowAttribute` で色の属性を 1 つ設定する
///
/// # Arguments
/// * `hwnd` - 対象ウィンドウのハンドル
/// * `attribute` - `DWMWA_CAPTION_COLOR` など色を取る属性
/// * `color` - 設定する色
fn set_color(hwnd: HWND, attribute: i32, color: egui::Color32) {
    let colorref = colorref(color);
    // SAFETY: `hwnd` は eframe が保持する有効なウィンドウのハンドルで、
    // `colorref` は呼び出し中に有効な `u32` を指す (サイズも一致する)
    // 戻り値は非対応環境で失敗するだけなので無視する
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            attribute.cast_unsigned(),
            std::ptr::from_ref(&colorref).cast::<c_void>(),
            COLORREF_SIZE,
        );
    }
}

/// `Color32` を Win32 の `COLORREF` (`0x00BBGGRR`) へ変換する
///
/// # Arguments
/// * `color` - 変換する色
///
/// # Returns
/// * `u32` - `COLORREF` 形式の値
fn colorref(color: egui::Color32) -> u32 {
    u32::from(color.r()) | (u32::from(color.g()) << 8) | (u32::from(color.b()) << 16)
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// `COLORREF` が `0x00BBGGRR` の並びになること
    #[test]
    fn colorref_orders_channels_as_bgr() {
        let color = egui::Color32::from_rgb(0x11, 0x22, 0x33);
        assert_eq!(colorref(color), 0x0033_2211);
    }
}
