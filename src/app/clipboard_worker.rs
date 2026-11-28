//! クリップボード操作用の専用バックグラウンドワーカー
//!
//! 切り出し・RGBA 変換・arboard 内部の DIB/PNG 変換・バッファ複製は大きな画像で重いため、
//! UI スレッドではなく 1 本のワーカースレッドで順に処理する。`arboard::Clipboard` は
//! ワーカーが所有し続け、コピーした内容がアプリ終了まで保持されるようにする
//!
//! ジョブ内の panic は捕捉して失敗結果として返し、ワーカースレッド自体は動かし続ける

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

use image::DynamicImage;

use super::image_convert::convert_to_srgb;
use super::state::IoMsg;
use super::waker::Waker;
use crate::image_load::{check_dimensions, validate_clipboard_rgba};
use crate::panic_message::catch_panic;
use crate::trim::{TrimInfo, apply_trim};

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
        /// 元画像の ICC プロファイル。あれば sRGB へ変換してからコピーする
        icc_profile: Option<Arc<[u8]>>,
        waker: Waker,
    },
    /// クリップボードの画像を取得し、完了を `IoMsg::Pasted` で通知する
    ///
    /// 画像がなくファイル (エクスプローラーでコピーしたもの) があれば、
    /// そのパスを `IoMsg::PastedFiles` で通知する
    Paste {
        /// 読み込み要求 ID。古い結果を無視するために使用する
        request_id: u64,
        waker: Waker,
    },
    /// 実クリップボードに触れず、`IoMsg::Copied` の成功を返すだけのジョブ
    #[cfg(test)]
    Probe { waker: Waker },
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
                    ClipboardJob::Copy {
                        image,
                        info,
                        icc_profile,
                        waker,
                    } => {
                        let result = run_guarded("コピー", &mut clipboard, |clipboard| {
                            let trimmed = apply_trim(&image, &info);
                            get_clipboard(clipboard).and_then(|cb| {
                                cb.set_image(to_clipboard_data(&trimmed, icc_profile.as_deref()))
                                    .map_err(|e| format!("コピーエラー: {e}"))
                            })
                        });
                        let _ = result_tx.send(IoMsg::Copied { result });
                        waker.wake();
                    }
                    ClipboardJob::Paste { request_id, waker } => {
                        let result = run_guarded("貼り付け", &mut clipboard, |clipboard| {
                            get_clipboard(clipboard).and_then(read_paste_content)
                        });
                        let msg = match result {
                            Ok(PasteContent::Image(image)) => IoMsg::Pasted {
                                request_id,
                                result: Ok(image),
                            },
                            Ok(PasteContent::Files(paths)) => {
                                IoMsg::PastedFiles { request_id, paths }
                            }
                            Err(msg) => IoMsg::Pasted {
                                request_id,
                                result: Err(msg),
                            },
                        };
                        let _ = result_tx.send(msg);
                        waker.wake();
                    }
                    #[cfg(test)]
                    ClipboardJob::Probe { waker } => {
                        let _ = result_tx.send(IoMsg::Copied { result: Ok(()) });
                        waker.wake();
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
    if let Some(clipboard) = clipboard {
        return Ok(clipboard);
    }
    let created = arboard::Clipboard::new()
        .map_err(|e| format!("クリップボードの初期化に失敗しました: {e}"))?;
    Ok(clipboard.insert(created))
}

// ======================================================================
// 貼り付け内容の取得
// ======================================================================
/// クリップボードから取得した貼り付け内容
enum PasteContent {
    /// 画像データ
    Image(DynamicImage),
    /// ファイルのパス (エクスプローラーなどでコピーしたファイル)
    Files(Vec<PathBuf>),
}

/// クリップボードの画像、なければファイルのパスを取得する
///
/// # Arguments
/// * `cb` - クリップボードアクセス用インスタンス
///
/// # Returns
/// * `Result<PasteContent, String>` - 取得した内容、または失敗時のステータスメッセージ
fn read_paste_content(cb: &mut arboard::Clipboard) -> Result<PasteContent, String> {
    // `get_image` は画像全体を RGBA へ展開するため、展開前にヘッダの寸法を検証する
    check_clipboard_image_dimensions()?;
    if let Ok(data) = cb.get_image() {
        return from_clipboard_data(data).map(PasteContent::Image);
    }
    match cb.get().file_list() {
        Ok(paths) if !paths.is_empty() => Ok(PasteContent::Files(paths)),
        _ => Err("クリップボードに画像またはファイルがありません".to_string()),
    }
}

// ======================================================================
// 寸法の事前検証
// ======================================================================
/// `BITMAPV5HEADER` のバイト数 (`CF_DIBV5` の先頭に置かれるヘッダ)
const DIBV5_HEADER_LEN: usize = 124;

/// ヘッダのバイト列から幅と高さを取り出す関数
type DimensionsParser = fn(&[u8]) -> Option<(u32, u32)>;

/// クリップボード画像の寸法を、展開前にヘッダだけ読んで検証する
///
/// `arboard` と同じく PNG を優先し、なければ `CF_DIBV5` を対象にする。
/// ヘッダを読めない場合は検証せず、後続の `get_image` の判定に任せる
///
/// # Returns
/// * `Result<(), String>` - 寸法が上限内、または判定できない場合は `Ok(())`。上限超過は失敗時のステータスメッセージ
fn check_clipboard_image_dimensions() -> Result<(), String> {
    // arboard の読み取りと重ならないよう、ヘッダの取得後すぐに閉じる
    let Ok(_clipboard) = clipboard_win::Clipboard::new_attempts(10) else {
        return Ok(());
    };
    let png_format = clipboard_win::register_format("PNG")
        .map(u32::from)
        .filter(|&id| clipboard_win::is_format_avail(id));
    let (format, parse): (u32, DimensionsParser) = match png_format {
        Some(id) => (id, png_dimensions),
        None => (clipboard_win::formats::CF_DIBV5, dib_dimensions),
    };
    if !clipboard_win::is_format_avail(format) {
        return Ok(());
    }

    let mut header = [0u8; DIBV5_HEADER_LEN];
    let Ok(len) = clipboard_win::raw::get(format, &mut header) else {
        return Ok(());
    };
    match parse(&header[..len]) {
        Some((width, height)) => check_dimensions(width, height).map_err(|e| e.to_string()),
        None => Ok(()),
    }
}

/// PNG の先頭バイト列から `IHDR` の幅と高さを取り出す
///
/// # Arguments
/// * `header` - PNG データの先頭バイト列
///
/// # Returns
/// * `Option<(u32, u32)>` - 幅と高さ。PNG として読めない場合は `None`
fn png_dimensions(header: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if header.len() < 24 || header[..8] != SIGNATURE || &header[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(header[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(header[20..24].try_into().ok()?);
    Some((width, height))
}

/// DIB ヘッダの先頭バイト列から幅と高さを取り出す
///
/// 上下反転 (トップダウン) の DIB は高さが負になるため、絶対値を返す
///
/// # Arguments
/// * `header` - `BITMAPV5HEADER` を含む先頭バイト列
///
/// # Returns
/// * `Option<(u32, u32)>` - 幅と高さ。ヘッダとして読めない場合は `None`
fn dib_dimensions(header: &[u8]) -> Option<(u32, u32)> {
    if header.len() < DIBV5_HEADER_LEN {
        return None;
    }
    let width = i32::from_le_bytes(header[4..8].try_into().ok()?);
    let height = i32::from_le_bytes(header[8..12].try_into().ok()?);
    Some((width.unsigned_abs(), height.unsigned_abs()))
}

// ======================================================================
// 画像変換
// ======================================================================
/// 画像をクリップボード転送用の RGBA データへ変換する
///
/// クリップボードには ICC プロファイルを載せられないため、RGB / GRAY のプロファイルがあれば
/// sRGB へ変換して、貼り付け先でもプレビューと同じ色に見えるようにする
///
/// # Arguments
/// * `image` - コピーする画像
/// * `icc_profile` - 元画像の ICC プロファイル。`None` や変換できないプロファイルは sRGB として扱う
///
/// # Returns
/// * `arboard::ImageData<'static>` - RGBA8 のバッファを所有する画像データ
fn to_clipboard_data(
    image: &DynamicImage,
    icc_profile: Option<&[u8]>,
) -> arboard::ImageData<'static> {
    let mut rgba = image.to_rgba8();
    if let Some(icc) = icc_profile {
        convert_to_srgb(&mut rgba, icc);
    }
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
    use image::RgbaImage;

    use super::*;

    /// ジョブが panic しても失敗結果として返り、クリップボードが破棄されること
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

    /// 終了済みのワーカーへ送ると、ジョブが返ること
    #[test]
    fn submit_returns_job_when_worker_terminated() {
        let handle = ClipboardWorkerHandle::terminated();
        let job = ClipboardJob::Paste {
            request_id: 9,
            waker: Waker::noop(),
        };
        match handle.submit(job) {
            Err(ClipboardJob::Paste { request_id, .. }) => assert_eq!(request_id, 9),
            _ => panic!("送れなかったジョブが返るはず"),
        }
    }

    /// RGBA へ変換したデータを戻すと元の画素と一致すること
    #[test]
    fn clipboard_data_round_trip_preserves_pixels() {
        let buf: Vec<u8> = (0u8..2 * 3 * 4).collect();
        let original = DynamicImage::ImageRgba8(RgbaImage::from_raw(2, 3, buf).unwrap());

        let data = to_clipboard_data(&original, None);
        assert_eq!((data.width, data.height), (2, 3));

        let restored = from_clipboard_data(data).unwrap();
        assert_eq!(restored.to_rgba8(), original.to_rgba8());
    }

    /// RGBA 以外の画像も RGBA8 に変換してコピーすること
    #[test]
    fn to_clipboard_data_converts_rgb_to_rgba() {
        let rgb = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(2, 2, image::Rgb([1, 2, 3])));
        let data = to_clipboard_data(&rgb, None);
        assert_eq!(data.bytes.len(), 2 * 2 * 4);
        assert_eq!(&data.bytes[..4], &[1, 2, 3, 255]);
    }

    /// ICC プロファイルがあるとコピーするデータが sRGB へ変換されること
    ///
    /// プロファイルなしや壊れたプロファイルでは画素を変更しない
    #[test]
    fn to_clipboard_data_converts_icc_profile_to_srgb() {
        let icc = moxcms::ColorProfile::new_display_p3().encode().unwrap();
        let img = DynamicImage::ImageRgba8(RgbaImage::from_pixel(
            2,
            2,
            image::Rgba([200, 100, 50, 128]),
        ));

        let plain = to_clipboard_data(&img, None);
        let converted = to_clipboard_data(&img, Some(&icc));
        // 色は変わり、アルファは保たれる
        assert_ne!(&converted.bytes[..3], &plain.bytes[..3]);
        assert_eq!(converted.bytes[3], 128);

        // 壊れたプロファイルは無視する
        let broken = to_clipboard_data(&img, Some(&[1, 2, 3]));
        assert_eq!(broken.bytes, plain.bytes);
    }

    /// PNG の `IHDR` から幅と高さを取り出し、PNG でないデータは `None` になること
    #[test]
    fn png_dimensions_reads_ihdr() {
        let mut header = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        header.extend_from_slice(&13u32.to_be_bytes());
        header.extend_from_slice(b"IHDR");
        header.extend_from_slice(&30_000u32.to_be_bytes());
        header.extend_from_slice(&20_000u32.to_be_bytes());
        assert_eq!(png_dimensions(&header), Some((30_000, 20_000)));

        // 短すぎる・シグネチャ不一致は読めない
        assert_eq!(png_dimensions(&header[..23]), None);
        header[1] = b'X';
        assert_eq!(png_dimensions(&header), None);
    }

    /// DIB ヘッダから幅と高さを取り出し、負の高さは絶対値にすること
    #[test]
    fn dib_dimensions_reads_header_and_handles_top_down() {
        let mut header = [0u8; DIBV5_HEADER_LEN];
        header[4..8].copy_from_slice(&1_000i32.to_le_bytes());
        header[8..12].copy_from_slice(&(-2_000i32).to_le_bytes());
        assert_eq!(dib_dimensions(&header), Some((1_000, 2_000)));

        // ヘッダ長に満たないデータは読めない
        assert_eq!(dib_dimensions(&header[..DIBV5_HEADER_LEN - 1]), None);
    }

    /// バッファ長が寸法と一致しないデータを拒否すること
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
