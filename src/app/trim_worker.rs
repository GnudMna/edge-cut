//! プレビュートリミング用の専用バックグラウンドワーカー
//!
//! スライダー操作のたびに `thread::spawn` する代わりに、アプリ起動時に 1 本の
//! ワーカースレッドを立ててジョブを処理する。キューに溜まった未処理ジョブは
//! 処理開始前に破棄し、常に最新の要求だけを実行する
//!
//! ワーカーはトリミング矩形の検出だけを行い、切り抜き画像やそのテクスチャは作らない。
//! プレビューは元画像テクスチャへ矩形を重ねる・UV で切り出すことで表示する
//!
//! ジョブ内の panic は捕捉して失敗結果として返し、ワーカースレッド自体は動かし続ける

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use image::{DynamicImage, RgbaImage};

use super::image_convert::to_preview_image;
use super::state::{TrimPreview, TrimWorkerResult};
use super::waker::Waker;
use crate::panic_message::catch_panic;
use crate::trim::{TrimOptions, detect_trim_with_options};

/// ワーカーへ送るトリミング要求
pub(super) struct TrimJob {
    pub request_id: u64,
    pub original: Arc<DynamicImage>,
    /// 元画像の ICC プロファイル。プレビューの色を sRGB へ変換するために使う
    pub icc_profile: Option<Arc<[u8]>>,
    pub options: TrimOptions,
    pub refresh_orig_tex: bool,
    pub waker: Waker,
}

/// プレビュートリミング専用の永続ワーカースレッド
pub(super) struct TrimWorkerHandle {
    job_tx: Sender<TrimJob>,
    _thread: JoinHandle<()>,
}

impl TrimWorkerHandle {
    /// バックグラウンドワーカーを起動する
    pub(super) fn spawn(result_tx: Sender<TrimWorkerResult>) -> Self {
        let (job_tx, job_rx) = mpsc::channel::<TrimJob>();
        let thread = thread::spawn(move || {
            while let Ok(job) = job_rx.recv() {
                let job = take_latest(job, &job_rx);
                let _ = result_tx.send(run_guarded(&job, make_orig_color, detect_job));
                job.waker.wake();
            }
        });
        Self {
            job_tx,
            _thread: thread,
        }
    }

    /// トリミング要求をワーカーへ送る
    pub(super) fn submit(&self, job: TrimJob) {
        let _ = self.job_tx.send(job);
    }
}

/// ジョブを実行し、panic を失敗結果に変換する
///
/// 元画像のプレビュー画像の生成と検出は別々に捕捉する
/// (どちらかが panic しても、もう一方の結果は失わない)
///
/// # Arguments
/// * `job` - 実行するジョブ
/// * `make_orig` - 元画像のプレビュー画像の生成
/// * `detect` - トリミング矩形の検出
///
/// # Returns
/// * `TrimWorkerResult` - 処理結果。検出が panic した場合は `outcome` が `Err(panic メッセージ)`、
///   プレビュー画像の生成が panic した場合は `orig_color` が `None`
fn run_guarded(
    job: &TrimJob,
    make_orig: impl FnOnce(&TrimJob) -> RgbaImage,
    detect: impl FnOnce(&TrimJob) -> TrimPreview,
) -> TrimWorkerResult {
    // ジョブはワーカー外と可変状態を共有しないため、panic 後も不整合にならない
    let orig_color = if job.refresh_orig_tex {
        catch_panic(|| make_orig(job)).ok()
    } else {
        None
    };
    TrimWorkerResult {
        request_id: job.request_id,
        outcome: catch_panic(|| detect(job)),
        orig_color,
    }
}

/// 元画像のプレビュー画像を生成する
fn make_orig_color(job: &TrimJob) -> RgbaImage {
    to_preview_image(job.original.as_ref(), job.icc_profile.as_deref())
}

/// トリミング矩形を検出する
fn detect_job(job: &TrimJob) -> TrimPreview {
    let bounds = detect_trim_with_options(job.original.as_ref(), &job.options);
    TrimPreview {
        params: job.options,
        info: bounds.info,
        background: bounds.background,
        aspect_skip: bounds.aspect_skip,
    }
}

/// キューに溜まったジョブを破棄し、最新のジョブだけを返す
///
/// 破棄したジョブが元画像テクスチャの再生成を要求していた場合は最新のジョブへ引き継ぐ
/// (引き継がないと読み込み直後のテクスチャ生成が失われ、プレビューが空のままになる)
///
/// # Arguments
/// * `job` - 受信済みのジョブ
/// * `job_rx` - 未処理ジョブの受信チャネル
///
/// # Returns
/// * `TrimJob` - 最新のジョブ
fn take_latest(mut job: TrimJob, job_rx: &Receiver<TrimJob>) -> TrimJob {
    while let Ok(mut newer) = job_rx.try_recv() {
        newer.refresh_orig_tex |= job.refresh_orig_tex;
        job = newer;
    }
    job
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use image::RgbaImage;

    use super::*;

    fn solid_image() -> Arc<DynamicImage> {
        // 逐次/並列切替の閾値付近のサイズにし、連続 submit 時に coalesce しやすくする
        let buf = vec![200u8; 600 * 600 * 4];
        Arc::new(DynamicImage::ImageRgba8(
            RgbaImage::from_raw(600, 600, buf).unwrap(),
        ))
    }

    /// 連続送信されたジョブのうち、最新の `request_id` の結果だけが返ること
    #[test]
    fn coalesces_pending_jobs_to_latest() {
        let (result_tx, result_rx) = mpsc::channel();
        let handle = TrimWorkerHandle::spawn(result_tx);
        let original = solid_image();

        for id in 1..=5 {
            handle.submit(TrimJob {
                request_id: id,
                original: Arc::clone(&original),
                icc_profile: None,
                options: TrimOptions::default(),
                refresh_orig_tex: false,
                waker: Waker::noop(),
            });
        }

        let mut last_id = 0u64;
        let deadline = Instant::now() + Duration::from_secs(5);
        // 処理時間に依存しないよう、最新の結果を受け取るか期限切れまで待つ
        while last_id < 5 && Instant::now() < deadline {
            match result_rx.recv_timeout(Duration::from_millis(50)) {
                Ok(result) => last_id = result.request_id,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        assert_eq!(last_id, 5);
        if let Ok(result) = result_rx.recv_timeout(Duration::from_millis(10)) {
            assert!(
                result.request_id >= last_id,
                "coalescing 後に古い request_id が返るべきではない"
            );
        }
    }

    /// ジョブ本体が panic しても、失敗結果として同じ `request_id` で返ること
    #[test]
    fn run_guarded_converts_panic_to_error() {
        let job = TrimJob {
            request_id: 7,
            original: solid_image(),
            icc_profile: None,
            options: TrimOptions::default(),
            refresh_orig_tex: false,
            waker: Waker::noop(),
        };

        // panic は Err に変換される
        let result = run_guarded(&job, make_orig_color, |_| panic!("テスト用の panic"));
        assert_eq!(result.request_id, 7);
        assert_eq!(result.outcome.err().as_deref(), Some("テスト用の panic"));

        // 正常時は検出結果を返す
        let result = run_guarded(&job, make_orig_color, detect_job);
        assert_eq!(result.request_id, 7);
        assert!(result.outcome.is_ok());
        assert!(result.orig_color.is_none());
    }

    /// 検出が panic しても、要求された元画像のプレビュー画像は返ること
    #[test]
    fn run_guarded_keeps_orig_color_when_detection_panics() {
        let job = TrimJob {
            request_id: 8,
            original: solid_image(),
            icc_profile: None,
            options: TrimOptions::default(),
            refresh_orig_tex: true,
            waker: Waker::noop(),
        };

        let result = run_guarded(&job, make_orig_color, |_| panic!("テスト用の panic"));
        assert!(result.outcome.is_err());
        assert!(result.orig_color.is_some());

        // 逆にプレビュー画像の生成が panic しても検出結果は返る
        let result = run_guarded(&job, |_| panic!("テスト用の panic"), detect_job);
        assert!(result.outcome.is_ok());
        assert!(result.orig_color.is_none());
    }

    /// 破棄したジョブのテクスチャ再生成要求が最新のジョブへ引き継がれること
    #[test]
    fn take_latest_carries_over_refresh_orig_tex() {
        let original = solid_image();
        let job = |request_id, refresh_orig_tex| TrimJob {
            request_id,
            original: Arc::clone(&original),
            icc_profile: None,
            options: TrimOptions::default(),
            refresh_orig_tex,
            waker: Waker::noop(),
        };

        // 先頭の再生成要求が、後続 2 件を経ても最新のジョブに残る
        let (tx, rx) = mpsc::channel();
        tx.send(job(2, false)).unwrap();
        tx.send(job(3, false)).unwrap();
        let latest = take_latest(job(1, true), &rx);
        assert_eq!(latest.request_id, 3);
        assert!(latest.refresh_orig_tex);

        // どのジョブも要求していなければ再生成しない
        tx.send(job(5, false)).unwrap();
        let latest = take_latest(job(4, false), &rx);
        assert_eq!(latest.request_id, 5);
        assert!(!latest.refresh_orig_tex);
    }
}
