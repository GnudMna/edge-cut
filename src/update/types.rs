//! 更新確認・適用で扱う型
//!
//! GitHub API の最新リリース、確認済みの更新 (`AvailableUpdate`)、
//! 確認の結果 (`UpdateCheckOutcome`)、配布物の種類と検証済みの配布物を定義する。

use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::error::UpdateError;
use crate::install_layout::is_portable_dir;

// ======================================================================
// リリース照会
// ======================================================================
/// GitHub API の最新リリース (使う項目のみ)
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct LatestRelease {
    /// リリースタグ (例: `v0.1.1`)
    pub tag_name: String,
    /// リリースアセット一覧
    #[serde(default)]
    pub assets: Vec<ReleaseAsset>,
}

/// GitHub リリースのアセット要素
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ReleaseAsset {
    /// アセットファイル名
    pub name: String,
    /// バイトサイズ
    #[serde(default)]
    pub size: u64,
    /// アセットの digest (例: `sha256:<hex>`)
    #[serde(default)]
    pub digest: Option<String>,
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

// ======================================================================
// 配布物
// ======================================================================
/// インストール形態に応じて選ぶ配布物の種類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UpdatePackageKind {
    /// MSI インストーラー
    Msi,
    /// ポータブル ZIP
    PortableZip,
}

impl UpdatePackageKind {
    /// 本体の実行フォルダから配布物の種類を返す
    ///
    /// # Arguments
    /// * `install_dir` - `EdgeCut.exe` のあるフォルダ
    ///
    /// # Returns
    /// * `Self` - `portable` マーカーがあれば `PortableZip`、それ以外は `Msi`
    pub(crate) fn for_install_dir(install_dir: &Path) -> Self {
        if is_portable_dir(install_dir) {
            Self::PortableZip
        } else {
            Self::Msi
        }
    }

    /// アセット名がこの種類の配布物か判定する
    ///
    /// # Arguments
    /// * `name` - アセット名
    ///
    /// # Returns
    /// * `bool` - 種類が一致すれば `true` (署名ファイル `.minisig` は一致しない)
    pub(crate) fn matches_asset_name(self, name: &str) -> bool {
        match self {
            Self::Msi => Path::new(name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("msi")),
            Self::PortableZip => name.to_ascii_lowercase().ends_with("-portable.zip"),
        }
    }

    /// 利用者向けの名前
    ///
    /// # Returns
    /// * `&'static str` - エラーメッセージに使う名前
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Msi => "MSI",
            Self::PortableZip => "ポータブル版 ZIP",
        }
    }
}

/// 検証を終え、インストールできる状態の配布物
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PreparedPackage {
    /// 検証済みの MSI
    Msi {
        /// ダウンロードした MSI
        path: PathBuf,
    },
    /// 展開済みのポータブル配布物
    Portable {
        /// 展開ルート (新しい `EdgeCut.exe` があるフォルダ)
        source_dir: PathBuf,
    },
}

/// 配布物のインストール結果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstallOutcome {
    /// インストールが完了した
    Installed,
    /// 完了に Windows の再起動が必要
    RebootRequired,
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// アセット名の拡張子・接尾辞で配布物の種類を判定すること
    #[test]
    fn matches_asset_name_follows_kind() {
        let msi = UpdatePackageKind::Msi;
        let zip = UpdatePackageKind::PortableZip;
        assert!(msi.matches_asset_name("edge-cut-2.1.0-x86_64.msi"));
        assert!(msi.matches_asset_name("edge-cut-2.1.0-x86_64.MSI"));
        assert!(!msi.matches_asset_name("edge-cut-2.1.0-x86_64.msi.minisig"));
        assert!(zip.matches_asset_name("edge-cut-2.1.0-x86_64-portable.zip"));
        assert!(!zip.matches_asset_name("edge-cut-2.1.0-x86_64-portable.zip.minisig"));
        assert!(!zip.matches_asset_name("edge-cut-2.1.0-x86_64.zip"));
    }

    /// `portable` マーカーの有無で配布物の種類を判定すること
    #[test]
    fn package_kind_follows_portable_marker() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            UpdatePackageKind::for_install_dir(tmp.path()),
            UpdatePackageKind::Msi
        );
        std::fs::write(
            tmp.path().join(crate::install_layout::PORTABLE_MARKER_FILE),
            b"",
        )
        .expect("marker");
        assert_eq!(
            UpdatePackageKind::for_install_dir(tmp.path()),
            UpdatePackageKind::PortableZip
        );
    }
}
