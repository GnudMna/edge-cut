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

use eframe::egui;
use image::DynamicImage;

use super::image_convert::to_preview_color_image;
use super::state::{TrimPreview, TrimWorkerResult};
use crate::panic_message::catch_panic;
use crate::trim::detect_trim;

/// ワーカーへ送るトリミング要求
pub(super) struct TrimJob {
    pub request_id: u64,
    pub original: Arc<DynamicImage>,
    pub tolerance: u32,
    pub padding: u32,
    pub bg_override: Option<[u8; 4]>,
    pub refresh_orig_tex: bool,
    pub ctx: egui::Context,
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
                let _ = result_tx.send(run_guarded(&job, run_job));
                job.ctx.request_repaint();
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
/// # Arguments
/// * `job` - 実行するジョブ
/// * `run` - ジョブの処理本体
///
/// # Returns
/// * `TrimWorkerResult` - 処理結果。panic した場合は `outcome` が `Err(panic メッセージ)`
fn run_guarded(job: &TrimJob, run: impl FnOnce(&TrimJob) -> TrimPreview) -> TrimWorkerResult {
    // ジョブはワーカー外と可変状態を共有しないため、panic 後も不整合にならない
    TrimWorkerResult {
        request_id: job.request_id,
        outcome: catch_panic(|| run(job)),
    }
}

/// トリミング矩形を検出し、要求があれば元画像のプレビュー画像も生成する
fn run_job(job: &TrimJob) -> TrimPreview {
    let orig_color = job
        .refresh_orig_tex
        .then(|| to_preview_color_image(job.original.as_ref()));
    let bounds = detect_trim(
        job.original.as_ref(),
        job.tolerance,
        job.padding,
        job.bg_override,
    );
    TrimPreview {
        params: (job.tolerance, job.padding, job.bg_override),
        info: bounds.info,
        orig_color,
        background: bounds.background,
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
        let ctx = egui::Context::default();
        let original = solid_image();

        for id in 1..=5 {
            handle.submit(TrimJob {
                request_id: id,
                original: Arc::clone(&original),
                tolerance: 0,
                padding: 0,
                bg_override: None,
                refresh_orig_tex: false,
                ctx: ctx.clone(),
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
            tolerance: 0,
            padding: 0,
            bg_override: None,
            refresh_orig_tex: false,
            ctx: egui::Context::default(),
        };

        // panic は Err に変換される
        let result = run_guarded(&job, |_| panic!("テスト用の panic"));
        assert_eq!(result.request_id, 7);
        assert_eq!(result.outcome.err().as_deref(), Some("テスト用の panic"));

        // 正常時は検出結果を返す
        let result = run_guarded(&job, run_job);
        assert_eq!(result.request_id, 7);
        assert!(result.outcome.is_ok());
    }

    /// 破棄したジョブのテクスチャ再生成要求が最新のジョブへ引き継がれること
    #[test]
    fn take_latest_carries_over_refresh_orig_tex() {
        let ctx = egui::Context::default();
        let original = solid_image();
        let job = |request_id, refresh_orig_tex| TrimJob {
            request_id,
            original: Arc::clone(&original),
            tolerance: 0,
            padding: 0,
            bg_override: None,
            refresh_orig_tex,
            ctx: ctx.clone(),
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
