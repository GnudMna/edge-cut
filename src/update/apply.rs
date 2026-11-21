use std::fs;
use std::path::{Path, PathBuf};

use super::error::UpdateError;
use super::types::{AvailableUpdate, UpdateApplyOutcome};
use super::verify::download_and_verify;
use super::{BRIDGE_BASE_URL, RELEASE_OWNER, RELEASE_REPO};

// ======================================================================
// パブリック関数
// ======================================================================
/// 更新アセットをダウンロードして MSI を適用する
///
/// # Arguments
/// * `update` - 確認済みの更新アセット情報
///
/// # Returns
/// * `UpdateApplyOutcome` - 適用結果
pub(crate) fn apply_update(update: &AvailableUpdate) -> UpdateApplyOutcome {
    apply_update_at(update, BRIDGE_BASE_URL, true)
}

/// 指定した bridge ベース URL で更新アセットをダウンロードして適用する
///
/// # Arguments
/// * `update` - 確認済みの更新アセット情報
/// * `base_url` - bridge のベース URL (末尾スラッシュなし)
/// * `launch_msi` - `msiexec` を起動するか
///
/// # Returns
/// * `UpdateApplyOutcome` - 適用結果
pub(crate) fn apply_update_at(
    update: &AvailableUpdate,
    base_url: &str,
    launch_msi: bool,
) -> UpdateApplyOutcome {
    let download_url = asset_download_url(base_url, &update.tag, &update.asset_name);
    let work_dir = match update_work_dir(&update.latest_version) {
        Ok(dir) => dir,
        Err(err) => return err.into(),
    };

    let archive_path = work_dir.join(&update.asset_name);
    if let Err(err) = download_and_verify(&download_url, &archive_path, update.size, &update.sha256)
    {
        return err.into();
    }

    if launch_msi {
        return launch_msiexec(&archive_path);
    }
    UpdateApplyOutcome::MsiStarted
}

/// アセットダウンロード URL を組み立てる
fn asset_download_url(base_url: &str, tag: &str, asset_name: &str) -> String {
    format!("{base_url}/{RELEASE_OWNER}/{RELEASE_REPO}/{tag}/{asset_name}")
}

// ======================================================================
// プライベート関数
// ======================================================================
/// 更新作業用ディレクトリを用意する
fn update_work_dir(version: &str) -> Result<PathBuf, UpdateError> {
    let base = std::env::temp_dir().join("edge-cut-update").join(version);
    fs::create_dir_all(&base).map_err(|_| UpdateError::WorkDirCreate)?;
    Ok(base)
}

/// MSI を `msiexec` で起動する
fn launch_msiexec(msi_path: &Path) -> UpdateApplyOutcome {
    let Some(path_str) = msi_path.to_str() else {
        return UpdateError::MsiPathInvalid.into();
    };

    match std::process::Command::new("msiexec")
        .args(["/i", path_str])
        .spawn()
    {
        Ok(_) => UpdateApplyOutcome::MsiStarted,
        Err(_) => UpdateError::MsiLaunch.into(),
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// アセットダウンロード URL が bridge パス規約に従うようにすること
    #[test]
    fn asset_download_url_matches_bridge_convention() {
        let url = asset_download_url(
            "https://gh-release-bridge.example.workers.dev",
            "v0.1.1",
            "edge-cut-0.1.1-x86_64.msi",
        );
        assert_eq!(
            url,
            "https://gh-release-bridge.example.workers.dev/GnudMna/edge-cut/v0.1.1/edge-cut-0.1.1-x86_64.msi"
        );
    }
}
