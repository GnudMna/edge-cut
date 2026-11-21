use super::error::UpdateError;

use serde::Deserialize;

// ======================================================================
// リリース照会
// ======================================================================
/// gh-release-bridge の version API レスポンス
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct VersionApiResponse {
    /// リリースタグ (例: `v0.1.1`)
    pub tag: String,
    /// 許可されたアセット一覧
    #[serde(default)]
    pub assets: Vec<VersionApiAsset>,
}

/// version API のアセット要素
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct VersionApiAsset {
    /// アセットファイル名
    pub name: String,
    /// バイトサイズ
    #[serde(default)]
    pub size: u64,
    /// GitHub Release Asset の digest (例: `sha256:<hex>`)
    #[serde(default)]
    pub digest: Option<String>,
    /// digest の代替フィールド (素の16進も可)
    #[serde(default)]
    pub sha256: Option<String>,
}

impl VersionApiAsset {
    /// 完全性検証に使う生文字列を返す
    ///
    /// # Arguments
    /// * `self` - 自身
    ///
    /// # Returns
    /// * `Option<&str>` - 値がある場合は `Some`、ない場合は `None`
    pub fn integrity_raw(&self) -> Option<&str> {
        self.digest
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.sha256
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
            })
    }
}

/// 適用可能な更新アセット
#[derive(Debug, Clone)]
pub struct AvailableUpdate {
    /// 現在のアプリバージョン (`SemVer`、`v` なし)
    pub current_version: String,
    /// 最新バージョン (`SemVer`、`v` なし)
    pub latest_version: String,
    /// リリースタグ (例: `v0.1.1`)
    pub tag: String,
    /// ダウンロードするアセット名
    pub asset_name: String,
    /// アセットサイズ (バイト)
    pub size: u64,
    /// 期待する SHA-256 (小文字16進 64 桁)
    pub sha256: String,
}

/// 更新確認の結果
#[derive(Debug, Clone)]
pub enum UpdateCheckOutcome {
    /// 最新版を利用中
    UpToDate {
        /// 現在のバージョン
        current_version: String,
        /// リモートの最新バージョン
        latest_version: String,
    },
    /// 新しい版が利用可能
    Available(AvailableUpdate),
    /// 確認に失敗
    Error {
        /// ユーザー向けメッセージ (`UpdateError` の `Display`)
        message: String,
    },
}

impl UpdateCheckOutcome {
    /// 型付きエラーから確認失敗 Outcome を組み立てる
    ///
    /// # Arguments
    /// * `err` - 更新確認の失敗理由
    ///
    /// # Returns
    /// * `Self` - `Error { message }`
    pub(crate) fn from_error(err: &UpdateError) -> Self {
        Self::Error {
            message: err.to_string(),
        }
    }
}

impl From<UpdateError> for UpdateCheckOutcome {
    fn from(err: UpdateError) -> Self {
        Self::from_error(&err)
    }
}

impl From<&UpdateError> for UpdateCheckOutcome {
    fn from(err: &UpdateError) -> Self {
        Self::from_error(err)
    }
}

/// 更新適用の結果
#[derive(Debug, Clone)]
pub enum UpdateApplyOutcome {
    /// MSI インストーラーを起動した (アプリ終了が必要)
    MsiStarted,
    /// 適用に失敗
    Error {
        /// ユーザー向けメッセージ (`UpdateError` の `Display`)
        message: String,
    },
}

impl UpdateApplyOutcome {
    /// 型付きエラーから適用失敗 Outcome を組み立てる
    ///
    /// # Arguments
    /// * `err` - 更新適用の失敗理由
    ///
    /// # Returns
    /// * `Self` - `Error { message }`
    pub(crate) fn from_error(err: &UpdateError) -> Self {
        Self::Error {
            message: err.to_string(),
        }
    }
}

impl From<UpdateError> for UpdateApplyOutcome {
    fn from(err: UpdateError) -> Self {
        Self::from_error(&err)
    }
}

impl From<&UpdateError> for UpdateApplyOutcome {
    fn from(err: &UpdateError) -> Self {
        Self::from_error(err)
    }
}
