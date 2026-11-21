//! アクションのテストで共有するヘルパー

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use eframe::egui;
use image::{DynamicImage, RgbaImage};

use crate::app::state::{App, ImageState};

// ======================================================================
// テスト用のヘルパー関数
// ======================================================================
/// テスト用の単色 4×4 RGBA 画像を生成する
pub(super) fn make_test_image() -> DynamicImage {
    let buf = vec![200u8; 4 * 4 * 4];
    DynamicImage::ImageRgba8(RgbaImage::from_raw(4, 4, buf).unwrap())
}

/// テスト画像と画像全体のトリミング矩形を持つ (トリミング済みの) 画像状態を生成する
pub(super) fn trimmed_image_state() -> ImageState {
    ImageState {
        original: Some(Arc::new(make_test_image())),
        info: Some(crate::trim::TrimInfo {
            orig_w: 4,
            orig_h: 4,
            trim_w: 4,
            trim_h: 4,
            offset_x: 0,
            offset_y: 0,
            extend_left: 0,
            extend_top: 0,
            extend_right: 0,
            extend_bottom: 0,
            fill: [255, 255, 255, 255],
        }),
        ..ImageState::default()
    }
}

/// 条件を満たすまで `poll_io()` を繰り返す。タイムアウト時は `false` を返す
pub(super) fn poll_io_until(
    app: &mut App,
    ctx: &egui::Context,
    cond: impl Fn(&App) -> bool,
) -> bool {
    for _ in 0..500 {
        app.poll_io(ctx);
        if cond(app) {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}

/// プレビュートリミングが完了するまで `poll_trim_work()` を繰り返す。タイムアウト時は `false` を返す
pub(super) fn poll_trim_until_idle(app: &mut App, ctx: &egui::Context) -> bool {
    for _ in 0..500 {
        app.poll_trim_work(ctx);
        if !app.trim_worker.is_busy() {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}
