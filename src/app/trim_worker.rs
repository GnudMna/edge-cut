//! プレビュートリミング用の専用バックグラウンドワーカー
//!
//! スライダー操作のたびに `thread::spawn` する代わりに、アプリ起動時に 1 本の
//! ワーカースレッドを立ててジョブを処理する。キューに溜まった未処理ジョブは
//! 処理開始前に破棄し、常に最新の要求だけを実行する

use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

use super::state::TrimWorkerResult;
use crate::trim::{to_color_image, trim};

use eframe::egui;
use image::DynamicImage;

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
            while let Ok(mut job) = job_rx.recv() {
                while let Ok(newer) = job_rx.try_recv() {
                    job = newer;
                }

                let orig_color = if job.refresh_orig_tex {
                    Some(to_color_image(job.original.as_ref()))
                } else {
                    None
                };
                let (trimmed, info, background) = trim(
                    job.original.as_ref(),
                    job.tolerance,
                    job.padding,
                    job.bg_override,
                );
                let trimmed_color = to_color_image(&trimmed);
                let _ = result_tx.send(TrimWorkerResult {
                    request_id: job.request_id,
                    trimmed,
                    trimmed_color,
                    info,
                    orig_color,
                    background,
                });
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    use crate::trim::TrimInfo;

    use image::RgbaImage;

    fn solid_image() -> Arc<DynamicImage> {
        let buf = vec![200u8; 4 * 4 * 4];
        Arc::new(DynamicImage::ImageRgba8(
            RgbaImage::from_raw(4, 4, buf).unwrap(),
        ))
    }

    /// 連続送信されたジョブのうち、最新の `request_id` の結果だけが返ることを確認する
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

        let result = result_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("worker should produce a result");
        assert_eq!(result.request_id, 5);
        assert_eq!(
            result.info,
            TrimInfo {
                orig_w: 4,
                orig_h: 4,
                trim_w: 4,
                trim_h: 4,
                offset_x: 0,
                offset_y: 0,
            }
        );
    }
}
