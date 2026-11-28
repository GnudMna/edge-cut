//! 更新プログラムの画面状態とコマンド (UI とやり取りする JSON)
//!
//! 型は `ui/src/updater/types.ts` と揃える (項目名は camelCase)

use serde::{Deserialize, Serialize};

use crate::update::flow::{Action, Page};

// ======================================================================
// 画面状態
// ======================================================================
/// UI へ送る画面状態
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct UpdaterSnapshot {
    /// 現在のバージョン
    pub version: &'static str,
    /// ポータブル版を更新するか
    pub portable: bool,
    /// 表示するページ
    pub page: PageView,
}

/// 表示するページ
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(super) enum PageView {
    /// 確認中
    Checking,
    /// 最新版
    UpToDate { current: String },
    /// 確認に失敗
    CheckFailed { reason: String },
    /// 更新あり
    Available {
        current: String,
        latest: String,
        size: u64,
    },
    /// ダウンロード・検証中
    Downloading {
        latest: String,
        received: u64,
        total: u64,
    },
    /// 本体の終了待ち
    WaitingExit,
    /// インストール中
    Installing { latest: String, restart: bool },
    /// インストールが終わった (本体は起動していない)
    Installed { latest: String },
    /// 新しい版を起動中
    Starting,
    /// Windows の再起動が必要
    RebootRequired { latest: String },
    /// 中止した
    Canceled,
    /// 失敗した
    Failed {
        reason: String,
        log: Option<String>,
        can_launch: bool,
    },
}

impl From<&Page> for PageView {
    fn from(page: &Page) -> Self {
        match page.clone() {
            Page::Checking => Self::Checking,
            Page::UpToDate { current } => Self::UpToDate { current },
            Page::CheckFailed { reason } => Self::CheckFailed { reason },
            Page::Available {
                current,
                latest,
                size,
            } => Self::Available {
                current,
                latest,
                size,
            },
            Page::Downloading {
                latest,
                received,
                total,
            } => Self::Downloading {
                latest,
                received,
                total,
            },
            Page::WaitingExit => Self::WaitingExit,
            Page::Installing { latest, restart } => Self::Installing { latest, restart },
            Page::Installed { latest } => Self::Installed { latest },
            Page::Starting => Self::Starting,
            Page::RebootRequired { latest } => Self::RebootRequired { latest },
            Page::Canceled => Self::Canceled,
            Page::Failed {
                reason,
                log,
                can_launch,
            } => Self::Failed {
                reason,
                log: log.map(|path| path.display().to_string()),
                can_launch,
            },
        }
    }
}

// ======================================================================
// コマンド
// ======================================================================
/// UI から届くコマンド
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(super) enum UpdaterCommand {
    /// UI の初期化が終わった
    Ready,
    /// 操作を選んだ
    Choose { action: ActionView },
    /// ダウンロードを中止する
    Cancel,
}

/// 利用者の操作
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum ActionView {
    /// 更新する
    Update,
    /// 閉じる
    Close,
    /// `EdgeCut` を起動して閉じる
    LaunchApp,
}

impl From<ActionView> for Action {
    fn from(action: ActionView) -> Self {
        match action {
            ActionView::Update => Self::Update,
            ActionView::Close => Self::Close,
            ActionView::LaunchApp => Self::LaunchApp,
        }
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// ページを `kind` 付きの camelCase の JSON にすること
    #[test]
    fn page_view_serializes_with_kind() {
        let page = Page::Failed {
            reason: "失敗".into(),
            log: Some(PathBuf::from(r"C:\logs\updater-msi.log")),
            can_launch: true,
        };
        let json = serde_json::to_value(PageView::from(&page)).expect("json");
        assert_eq!(
            json,
            serde_json::json!({
                "kind": "failed",
                "reason": "失敗",
                "log": r"C:\logs\updater-msi.log",
                "canLaunch": true,
            })
        );

        let json = serde_json::to_value(PageView::from(&Page::WaitingExit)).expect("json");
        assert_eq!(json, serde_json::json!({ "kind": "waitingExit" }));
    }

    /// UI のコマンドを解釈すること
    #[test]
    fn command_parses_from_json() {
        let parse = |json: &str| serde_json::from_str::<UpdaterCommand>(json).expect("parse");
        assert_eq!(parse(r#"{"type":"ready"}"#), UpdaterCommand::Ready);
        assert_eq!(parse(r#"{"type":"cancel"}"#), UpdaterCommand::Cancel);
        assert_eq!(
            parse(r#"{"type":"choose","action":"launchApp"}"#),
            UpdaterCommand::Choose {
                action: ActionView::LaunchApp
            }
        );
        assert!(serde_json::from_str::<UpdaterCommand>(r#"{"type":"other"}"#).is_err());
    }
}
