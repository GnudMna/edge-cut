//! クリップボード操作用の専用バックグラウンドワーカー
//!
//! 切り出し・RGBA 変換・arboard 内部の DIB/PNG 変換・バッファ複製は大きな画像で重いため、
//! UI スレッドではなく 1 本のワーカースレッドで順に処理する。`arboard::Clipboard` は
//! ワーカーが所有し続け、Linux でもコピーした内容がアプリ終了まで保持されるようにする
//!
//! ジョブ内の panic は捕捉して失敗結果として返し、ワーカースレッド自体は動かし続ける

use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

use super::state::IoMsg;
use crate::image_load::validate_clipboard_rgba;
use crate::panic_message::catch_panic;
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
                        let result = run_guarded("コピー", &mut clipboard, |clipboard| {
                            let trimmed = apply_trim(&image, &info);
                            get_clipboard(clipboard).and_then(|cb| {
                                cb.set_image(to_clipboard_data(&trimmed))
                                    .map_err(|e| format!("コピーエラー: {e}"))
                            })
                        });
                        let _ = result_tx.send(IoMsg::Copied { result });
                        ctx.request_repaint();
                    }
                    ClipboardJob::Paste { request_id, ctx } => {
                        let result = run_guarded("貼り付け", &mut clipboard, |clipboard| {
                            get_clipboard(clipboard).and_then(|cb| {
                                let data = cb
                                    .get_image()
                                    .map_err(|_| "クリップボードに画像がありません".to_string())?;
                                from_clipboard_data(data)
                            })
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
    ///
    /// # Arguments
    /// * `job` - ワーカーへ送るクリップボード操作
    ///
    /// # Returns
    /// * `Result<(), ClipboardJob>` - ワーカーが終了していて送れなかった場合は、そのジョブ
    pub(super) fn submit(&self, job: ClipboardJob) -> Result<(), ClipboardJob> {
        self.job_tx.send(job).map_err(|e| e.0)
    }

    /// 終了済みのワーカーを模したハンドルを生成する
    #[cfg(test)]
    pub(super) fn terminated() -> Self {
        let (job_tx, job_rx) = mpsc::channel::<ClipboardJob>();
        drop(job_rx);
        Self {
            job_tx,
            _thread: thread::spawn(|| {}),
        }
    }
}

/// ジョブを実行し、panic を失敗結果に変換する
///
/// panic 後のクリップボードは状態を信頼できないため破棄し、次のジョブで作り直す
///
/// # Arguments
/// * `action` - 失敗メッセージに表示する操作名
/// * `clipboard` - ワーカーが所有するクリップボードアクセス用インスタンス
/// * `run` - ジョブの処理本体
///
/// # Returns
/// * `Result<T, String>` - `run` の結果。panic した場合は失敗時のステータスメッセージ
fn run_guarded<T>(
    action: &str,
    clipboard: &mut Option<arboard::Clipboard>,
    run: impl FnOnce(&mut Option<arboard::Clipboard>) -> Result<T, String>,
) -> Result<T, String> {
    catch_panic(|| run(&mut *clipboard)).unwrap_or_else(|msg| {
        *clipboard = None;
        Err(format!(
            "クリップボードの{action}中にエラーが発生しました: {msg}"
        ))
    })
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

    /// ジョブが panic しても失敗結果として返り、クリップボードが破棄されることを確認する
    #[test]
    fn run_guarded_converts_panic_to_error() {
        let mut clipboard = None;

        // panic はメッセージ付きの Err に変換される
        let result: Result<(), String> =
            run_guarded("コピー", &mut clipboard, |_| panic!("テスト用の panic"));
        let msg = result.unwrap_err();
        assert!(msg.contains("コピー"), "{msg}");
        assert!(msg.contains("テスト用の panic"), "{msg}");
        assert!(clipboard.is_none());

        // 正常時は結果をそのまま返す
        let result = run_guarded("コピー", &mut clipboard, |_| Ok(1));
        assert_eq!(result, Ok(1));
    }

    /// 終了済みのワーカーへ送ると、ジョブが返ることを確認する
    #[test]
    fn submit_returns_job_when_worker_terminated() {
        let handle = ClipboardWorkerHandle::terminated();
        let job = ClipboardJob::Paste {
            request_id: 9,
            ctx: egui::Context::default(),
        };
        match handle.submit(job) {
            Err(ClipboardJob::Paste { request_id, .. }) => assert_eq!(request_id, 9),
            _ => panic!("送れなかったジョブが返るはず"),
        }
    }

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
