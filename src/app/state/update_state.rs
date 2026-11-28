//! アプリ更新の UI・ワーカー状態

use std::sync::mpsc;

use crate::update::{AvailableUpdate, UpdateCheckOutcome, UpdaterSession};

// ======================================================================
// 更新確認
// ======================================================================
/// 更新 UI の状態
#[derive(Debug, Clone, Default)]
pub(in crate::app) enum UpdateUiPhase {
    /// 未確認
    #[default]
    Idle,
    /// version API 照会中
    Checking,
    /// 最新版
    UpToDate {
        /// 現在バージョン
        current_version: String,
    },
    /// 更新あり
    Available(AvailableUpdate),
    /// 失敗
    Error {
        /// ユーザー向けメッセージ
        message: String,
    },
    /// 更新プログラムを起動済み (ダウンロード中も本体は使える)
    UpdaterRunning(AvailableUpdate),
    /// 更新プログラムから終了を依頼された (保存の完了を待って終了する)
    Exiting,
}

/// バックグラウンド更新処理の結果
pub(in crate::app) enum UpdateWorkerMsg {
    /// 更新確認の結果
    Check(UpdateCheckOutcome),
}

/// アプリ更新の UI / ワーカー状態
#[derive(Default)]
pub(in crate::app) struct UpdateState {
    /// 現在の UI フェーズ
    pub phase: UpdateUiPhase,
    /// バックグラウンドからの結果受信
    pub rx: Option<mpsc::Receiver<UpdateWorkerMsg>>,
    /// 起動中の更新プログラムとの接続 (本体の終了まで保持する)
    pub session: Option<UpdaterSession>,
    /// 更新ダイアログを表示中か
    pub dialog_open: bool,
}

impl UpdateState {
    /// 確認中か、更新プログラムが動作中か
    pub(in crate::app) fn is_busy(&self) -> bool {
        matches!(
            self.phase,
            UpdateUiPhase::Checking | UpdateUiPhase::UpdaterRunning(_) | UpdateUiPhase::Exiting
        )
    }

    /// 更新の適用のため、アプリの終了へ向かっているか
    pub(in crate::app) fn is_applying(&self) -> bool {
        matches!(self.phase, UpdateUiPhase::Exiting)
    }
}
