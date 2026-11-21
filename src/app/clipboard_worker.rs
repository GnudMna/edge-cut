//! クリップボード操作用の専用バックグラウンドワーカー
//!
//! 切り出し・RGBA 変換・arboard 内部の DIB/PNG 変換・バッファ複製は大きな画像で重いため、
//! UI スレッドではなく 1 本のワーカースレッドで順に処理する。`arboard::Clipboard` は
//! ワーカーが所有し続け、Linux でもコピーした内容がアプリ終了まで保持されるようにする

use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

use super::state::IoMsg;
use crate::image_load::validate_clipboard_rgba;
use crate::trim::{TrimInfo, apply_trim};

use eframe::egui;
use image::DynamicImage;

// ======================================================================
// ジョブ定義
// ======================================================================
/// ワーカーへ送るクリップボード操作
pub(super) enum ClipboardJob {
    /// 元画像を矩形で切り出してクリップボードへコピーし、完了を `IoMsg::Copied` で通知する
    Copy {
        /// 切り出し元の画像
        image: Arc<DynamicImage>,
        /// 切り出すトリミング矩形
        info: TrimInfo,
        ctx: egui::Context,
    },
    /// クリップボードの画像を取得し、完了を `IoMsg::Pasted` で通知する
    Paste {
        /// 読み込み要求 ID。古い結果を無視するために使用する
        request_id: u64,
        ctx: egui::Context,
    },
}

// ======================================================================
// ワーカースレッド
// ======================================================================
/// クリップボード操作専用の永続ワーカースレッド
pub(super) struct ClipboardWorkerHandle {
    job_tx: Sender<ClipboardJob>,
    _thread: JoinHandle<()>,
}

impl ClipboardWorkerHandle {
    /// バックグラウンドワーカーを起動する
    ///
    /// # Arguments
    /// * `result_tx` - 処理結果を送る I/O 通知チャネル
    pub(super) fn spawn(result_tx: Sender<IoMsg>) -> Self {
        let (job_tx, job_rx) = mpsc::channel::<ClipboardJob>();
        let thread = thread::spawn(move || {
            // 初期化に失敗しても次のジョブで再試行する
            let mut clipboard: Option<arboard::Clipboard> = None;
            while let Ok(job) = job_rx.recv() {
                match job {
                    ClipboardJob::Copy { image, info, ctx } => {
                        let trimmed = apply_trim(&image, &info);
                        let result = get_clipboard(&mut clipboard).and_then(|cb| {
                            cb.set_image(to_clipboard_data(&trimmed))
                                .map_err(|e| format!("コピーエラー: {e}"))
                        });
                        let _ = result_tx.send(IoMsg::Copied { result });
                        ctx.request_repaint();
                    }
                    ClipboardJob::Paste { request_id, ctx } => {
                        let result = get_clipboard(&mut clipboard).and_then(|cb| {
                            let data = cb
                                .get_image()
                                .map_err(|_| "クリップボードに画像がありません".to_string())?;
                            from_clipboard_data(data)
                        });
                        let _ = result_tx.send(IoMsg::Pasted { request_id, result });
                        ctx.request_repaint();
                    }
                }
            }
        });
        Self {
            job_tx,
            _thread: thread,
        }
    }

    /// クリップボード操作をワーカーへ送る
    pub(super) fn submit(&self, job: ClipboardJob) {
        let _ = self.job_tx.send(job);
    }
}

/// クリップボードアクセス用インスタンスを返す。未初期化ならここで作成する
fn get_clipboard(
    clipboard: &mut Option<arboard::Clipboard>,
) -> Result<&mut arboard::Clipboard, String> {
    if clipboard.is_none() {
        *clipboard = Some(
            arboard::Clipboard::new()
                .map_err(|e| format!("クリップボードの初期化に失敗しました: {e}"))?,
        );
    }
    Ok(clipboard.as_mut().unwrap())
}

// ======================================================================
// 画像変換
// ======================================================================
/// 画像をクリップボード転送用の RGBA データへ変換する
///
/// # Arguments
/// * `image` - コピーする画像
///
/// # Returns
/// * `arboard::ImageData<'static>` - RGBA8 のバッファを所有する画像データ
fn to_clipboard_data(image: &DynamicImage) -> arboard::ImageData<'static> {
    let rgba = image.to_rgba8();
    let (w, h) = rgba.dimensions();
    arboard::ImageData {
        width: w as usize,
        height: h as usize,
        bytes: rgba.into_raw().into(),
    }
}

/// クリップボードから取得した RGBA データを画像へ変換する
///
/// 寸法の上限とバッファ長を検証してから変換する
///
/// # Arguments
/// * `data` - クリップボードから取得した画像データ
///
/// # Returns
/// * `Result<DynamicImage, String>` - 変換した画像、または失敗時のステータスメッセージ
fn from_clipboard_data(data: arboard::ImageData<'_>) -> Result<DynamicImage, String> {
    const INVALID_SIZE: &str = "クリップボードの画像サイズが不正です";
    let w = u32::try_from(data.width).map_err(|_| INVALID_SIZE.to_string())?;
    let h = u32::try_from(data.height).map_err(|_| INVALID_SIZE.to_string())?;
    let bytes: Vec<u8> = data.bytes.into_owned();
    validate_clipboard_rgba(w, h, bytes.len()).map_err(|e| e.to_string())?;
    image::RgbaImage::from_raw(w, h, bytes)
        .map(DynamicImage::ImageRgba8)
        .ok_or_else(|| "クリップボード画像の変換に失敗しました".to_string())
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    use image::RgbaImage;

    /// RGBA へ変換したデータを戻すと元の画素と一致することを確認する
    #[test]
    fn clipboard_data_round_trip_preserves_pixels() {
        let buf: Vec<u8> = (0u8..2 * 3 * 4).collect();
        let original = DynamicImage::ImageRgba8(RgbaImage::from_raw(2, 3, buf).unwrap());

        let data = to_clipboard_data(&original);
        assert_eq!((data.width, data.height), (2, 3));

        let restored = from_clipboard_data(data).unwrap();
        assert_eq!(restored.to_rgba8(), original.to_rgba8());
    }

    /// RGBA 以外の画像も RGBA8 に変換してコピーすることを確認する
    #[test]
    fn to_clipboard_data_converts_rgb_to_rgba() {
        let rgb = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(2, 2, image::Rgb([1, 2, 3])));
        let data = to_clipboard_data(&rgb);
        assert_eq!(data.bytes.len(), 2 * 2 * 4);
        assert_eq!(&data.bytes[..4], &[1, 2, 3, 255]);
    }

    /// バッファ長が寸法と一致しないデータを拒否することを確認する
    #[test]
    fn from_clipboard_data_rejects_size_mismatch() {
        let data = arboard::ImageData {
            width: 4,
            height: 4,
            bytes: vec![0u8; 10].into(),
        };
        assert!(from_clipboard_data(data).is_err());
    }
}
