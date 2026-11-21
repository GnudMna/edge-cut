//! プレビュートリミングのワーカー状態と結果

use std::sync::mpsc;
use std::time::Instant;

use eframe::egui;

use crate::app::trim_worker::TrimWorkerHandle;
use crate::trim::{TrimInfo, TrimOptions};

/// トリミング結果を決めるパラメータ (トリミングの設定そのもの)
pub(in crate::app) type TrimParams = TrimOptions;

/// バックグラウンドのプレビュートリミング結果
pub(in crate::app) struct TrimWorkerResult {
    pub request_id: u64,
    /// 検出結果。処理中に panic した場合は `Err(panic メッセージ)`
    pub outcome: Result<TrimPreview, String>,
    /// 元画像のプレビュー画像。要求があり生成できた場合のみ `Some`
    ///
    /// 検出が失敗した場合でも元画像を表示できるよう、`outcome` とは独立して返す
    pub orig_color: Option<egui::ColorImage>,
}

/// プレビュートリミングの検出結果
pub(in crate::app) struct TrimPreview {
    /// このジョブで使ったパラメータ。受信時点の UI の値とは異なりうる
    pub params: TrimParams,
    pub info: TrimInfo,
    pub background: [u8; 4],
}

/// プレビュートリミングのバックグラウンド処理
pub(in crate::app) struct TrimWorkerState {
    /// 専用ワーカースレッドへのジョブ送信
    pub(in crate::app) handle: TrimWorkerHandle,
    /// ワーカーからの結果受信チャネル
    pub(in crate::app) result_rx: mpsc::Receiver<TrimWorkerResult>,
    /// 要求 ID。古い結果を無視するために使用する
    pub request_id: u64,
    /// バックグラウンドで実行中かどうか
    pub in_flight: bool,
    /// デバウンス後にトリミングを開始する時刻
    pub debounce_until: Option<Instant>,
    /// 元画像テクスチャも再生成するか。再生成したテクスチャを反映するまで `true` のまま
    pub refresh_orig_tex: bool,
    /// 次回完了後に自動保存するか
    pub auto_save_after: bool,
}

impl TrimWorkerState {
    /// 専用ワーカースレッド付きの初期状態を生成する
    pub(in crate::app) fn new() -> Self {
        let (result_tx, result_rx) = mpsc::channel();
        Self {
            handle: TrimWorkerHandle::spawn(result_tx),
            result_rx,
            request_id: 0,
            in_flight: false,
            debounce_until: None,
            refresh_orig_tex: false,
            auto_save_after: false,
        }
    }

    /// プレビュートリミングが進行中かどうか
    pub(in crate::app) fn is_busy(&self) -> bool {
        self.in_flight || self.debounce_until.is_some()
    }

    /// ワーカースレッドを起動し直し、処理中のジョブを打ち切る
    ///
    /// ワーカーが終了して結果チャネルが切断された場合に呼ぶ
    pub(in crate::app) fn restart(&mut self) {
        let (result_tx, result_rx) = mpsc::channel();
        self.handle = TrimWorkerHandle::spawn(result_tx);
        self.result_rx = result_rx;
        self.in_flight = false;
    }

    /// 進行中の処理を打ち切り、古い結果を無視する
    pub(in crate::app) fn abort(&mut self) {
        self.request_id = self.request_id.wrapping_add(1);
        self.in_flight = false;
        self.debounce_until = None;
        self.refresh_orig_tex = false;
        self.auto_save_after = false;
    }
}
