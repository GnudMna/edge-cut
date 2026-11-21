//! アプリ更新の UI・ワーカー状態

use std::sync::mpsc;

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
    Available(crate::update::AvailableUpdate),
    /// MSI ダウンロード・検証中
    Downloading {
        /// 確認済みの更新情報
        update: crate::update::AvailableUpdate,
    },
    /// 失敗
    Error {
        /// ユーザー向けメッセージ
        message: String,
    },
    /// MSI を起動済み (アプリ終了待ち)
    MsiStarted,
}

/// バックグラウンド更新処理の結果
pub(in crate::app) enum UpdateWorkerMsg {
    /// 更新確認の結果
    Check(crate::update::UpdateCheckOutcome),
    /// 更新適用の結果
    Apply(crate::update::UpdateApplyOutcome),
}

/// アプリ更新の UI / ワーカー状態
#[derive(Default)]
pub(in crate::app) struct UpdateState {
    /// 現在の UI フェーズ
    pub phase: UpdateUiPhase,
    /// バックグラウンドからの結果受信
    pub rx: Option<mpsc::Receiver<UpdateWorkerMsg>>,
    /// 更新ダイアログを表示中か
    pub dialog_open: bool,
}

impl UpdateState {
    /// 確認または適用が進行中か
    pub(in crate::app) fn is_busy(&self) -> bool {
        matches!(
            self.phase,
            UpdateUiPhase::Checking | UpdateUiPhase::Downloading { .. }
        )
    }

    /// 更新の適用が始まり、アプリの終了へ向かっているか
    pub(in crate::app) fn is_applying(&self) -> bool {
        matches!(
            self.phase,
            UpdateUiPhase::Downloading { .. } | UpdateUiPhase::MsiStarted
        )
    }
}
