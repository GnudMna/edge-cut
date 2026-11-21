//! 画像読み込み用の専用バックグラウンドワーカー
//!
//! 読み込みのたびに `thread::spawn` する代わりに、1 本のワーカースレッドで順に処理する。
//! 連続して開いても同時にデコードされる画像は常に 1 枚までで、メモリ使用量が跳ねない。
//! キューに溜まった未処理ジョブは処理開始前に破棄し、常に最新の要求だけを実行する
//!
//! 実行中のデコードは中断できないため、古い結果は呼び出し側が `request_id` で捨てる
//!
//! ジョブ内の panic は捕捉して失敗結果として返し、ワーカースレッド自体は動かし続ける

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use super::state::IoMsg;
use crate::image_load::{LoadedImage, open_image_with_icc};
use crate::panic_message::catch_panic;

use eframe::egui;

// ======================================================================
// ジョブ定義
// ======================================================================
/// ワーカーへ送る画像読み込み要求
pub(super) struct LoadJob {
    /// 読み込み要求 ID。古い結果を無視するために使用する
    pub request_id: u64,
    /// 読み込む画像ファイルのパス
    pub path: PathBuf,
    /// ステータス表示用のファイル名
    pub file_name: String,
    pub ctx: egui::Context,
}

// ======================================================================
// ワーカースレッド
// ======================================================================
/// 画像読み込み専用の永続ワーカースレッド
pub(super) struct LoadWorkerHandle {
    job_tx: Sender<LoadJob>,
    _thread: JoinHandle<()>,
}

impl LoadWorkerHandle {
    /// バックグラウンドワーカーを起動する
    ///
    /// # Arguments
    /// * `result_tx` - 読み込み結果を送る I/O 通知チャネル
    pub(super) fn spawn(result_tx: Sender<IoMsg>) -> Self {
        let (job_tx, job_rx) = mpsc::channel::<LoadJob>();
        let thread = thread::spawn(move || {
            while let Ok(job) = job_rx.recv() {
                let job = take_latest(job, &job_rx);
                let result = load_guarded(&job.path, |path| {
                    open_image_with_icc(path).map_err(|e| e.to_string())
                });
                let _ = result_tx.send(IoMsg::Loaded {
                    request_id: job.request_id,
                    file_name: job.file_name,
                    result,
                });
                job.ctx.request_repaint();
            }
        });
        Self {
            job_tx,
            _thread: thread,
        }
    }

    /// 読み込み要求をワーカーへ送る
    pub(super) fn submit(&self, job: LoadJob) {
        let _ = self.job_tx.send(job);
    }
}

/// 画像を読み込み、panic を失敗結果に変換する
///
/// # Arguments
/// * `path` - 読み込む画像ファイルのパス
/// * `load` - 読み込みの処理本体
///
/// # Returns
/// * `Result<LoadedImage, String>` - 読み込んだ画像、または失敗時のステータスメッセージ
fn load_guarded(
    path: &Path,
    load: impl FnOnce(&Path) -> Result<LoadedImage, String>,
) -> Result<LoadedImage, String> {
    // ジョブはワーカー外と可変状態を共有しないため、panic 後も不整合にならない
    catch_panic(|| load(path))
        .unwrap_or_else(|msg| Err(format!("画像の読み込み中にエラーが発生しました: {msg}")))
}

/// キューに溜まったジョブを破棄し、最新のジョブだけを返す
///
/// # Arguments
/// * `job` - 受信済みのジョブ
/// * `job_rx` - 未処理ジョブの受信チャネル
///
/// # Returns
/// * `LoadJob` - 最新のジョブ
fn take_latest(mut job: LoadJob, job_rx: &Receiver<LoadJob>) -> LoadJob {
    while let Ok(newer) = job_rx.try_recv() {
        job = newer;
    }
    job
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    fn job(request_id: u64, ctx: &egui::Context) -> LoadJob {
        LoadJob {
            request_id,
            path: PathBuf::from(format!("missing_{request_id}.png")),
            file_name: format!("missing_{request_id}.png"),
            ctx: ctx.clone(),
        }
    }

    /// キューに溜まったジョブのうち最新のものだけが残ることを確認する
    #[test]
    fn take_latest_keeps_only_newest_job() {
        let ctx = egui::Context::default();
        let (tx, rx) = mpsc::channel();
        tx.send(job(2, &ctx)).unwrap();
        tx.send(job(3, &ctx)).unwrap();

        // 受信済みの 1 件より後続が優先される
        let latest = take_latest(job(1, &ctx), &rx);
        assert_eq!(latest.request_id, 3);
        assert!(rx.try_recv().is_err());

        // 後続がなければ受信済みのジョブをそのまま返す
        let latest = take_latest(job(4, &ctx), &rx);
        assert_eq!(latest.request_id, 4);
    }

    /// 読み込み本体が panic しても、失敗結果として返ることを確認する
    #[test]
    fn load_guarded_converts_panic_to_error() {
        let result = load_guarded(Path::new("dummy.png"), |_| panic!("テスト用の panic"));
        let msg = result.err().unwrap();
        assert!(msg.contains("テスト用の panic"), "{msg}");
    }

    /// 連続送信したジョブのうち、最新の `request_id` の結果が必ず返り、それより後に古い結果が来ないことを確認する
    #[test]
    fn worker_always_returns_latest_result() {
        let (result_tx, result_rx) = mpsc::channel();
        let handle = LoadWorkerHandle::spawn(result_tx);
        let ctx = egui::Context::default();

        for id in 1..=5 {
            handle.submit(job(id, &ctx));
        }

        let mut ids = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        // 処理時間に依存しないよう、最新の結果を受け取るか期限切れまで待つ
        while ids.last() != Some(&5) && Instant::now() < deadline {
            match result_rx.recv_timeout(Duration::from_millis(50)) {
                Ok(IoMsg::Loaded {
                    request_id, result, ..
                }) => {
                    assert!(result.is_err(), "存在しないファイルは失敗する");
                    ids.push(request_id);
                }
                Ok(_) => unreachable!("読み込みワーカーは Loaded 以外を送らない"),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        assert_eq!(ids.last(), Some(&5));
        // 結果は request_id の昇順で届く
        assert!(ids.windows(2).all(|w| w[0] < w[1]), "{ids:?}");
    }
}
