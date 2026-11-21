use std::fs;
use std::path::{Path, PathBuf};

use super::error::UpdateError;
use super::signature::{
    download_signature, embedded_signing_keys, signature_asset_name, verify_file_signature,
};
use super::types::{AvailableUpdate, UpdateApplyOutcome};
use super::verify::{download_and_verify_with_key, validate_asset_name};
use super::{BRIDGE_BASE_URL, RELEASE_OWNER, RELEASE_REPO, bridge_download_key};

use known_folders::{KnownFolder, get_known_folder_path};

// ======================================================================
// パブリック関数
// ======================================================================
/// 更新アセットをダウンロードして MSI を適用する
///
/// 署名・サイズ・SHA-256 を検証してから `msiexec` を起動する
///
/// # Arguments
/// * `update` - 確認済みの更新アセット情報
///
/// # Returns
/// * `UpdateApplyOutcome` - 適用結果
pub(crate) fn apply_update(update: &AvailableUpdate) -> UpdateApplyOutcome {
    apply_update_at(update, BRIDGE_BASE_URL, true, embedded_signing_keys())
}

/// 指定した bridge ベース URL で更新アセットをダウンロードして適用する
///
/// # Arguments
/// * `update` - 確認済みの更新アセット情報
/// * `base_url` - bridge のベース URL (末尾スラッシュなし)
/// * `launch_msi` - `msiexec` を起動するか
/// * `trusted_keys` - 署名検証に使う minisign 公開鍵 (Base64)
///
/// # Returns
/// * `UpdateApplyOutcome` - 適用結果
pub(crate) fn apply_update_at(
    update: &AvailableUpdate,
    base_url: &str,
    launch_msi: bool,
    trusted_keys: &[&str],
) -> UpdateApplyOutcome {
    // 確認時にも検証済みだが、保存先パスと URL に使う直前でも再確認する
    if let Err(err) = validate_asset_name(&update.asset_name) {
        return err.into();
    }
    let work_dir = match update_work_dir(&update.latest_version) {
        Ok(dir) => dir,
        Err(err) => return err.into(),
    };

    let archive_path = work_dir.join(&update.asset_name);
    if let Err(err) = download_verified_asset(
        update,
        base_url,
        &archive_path,
        bridge_download_key(),
        trusted_keys,
    ) {
        return err.into();
    }

    if launch_msi {
        return launch_msiexec(&archive_path);
    }
    UpdateApplyOutcome::MsiStarted
}

/// アセットと署名をダウンロードし、サイズ・SHA-256・署名を検証する
///
/// 署名は小さいため先に取得し、欠落時はアセット本体をダウンロードしない
/// 署名検証に失敗した場合は保存済みアセットを削除する
///
/// # Arguments
/// * `update` - 確認済みの更新アセット情報
/// * `base_url` - bridge のベース URL (末尾スラッシュなし)
/// * `archive_path` - アセットの保存先
/// * `download_key` - bridge ダウンロードキー (`None` なら無認証)
/// * `trusted_keys` - 署名検証に使う minisign 公開鍵 (Base64)
///
/// # Returns
/// * `Result<(), UpdateError>` - すべての検証に成功した場合は `Ok(())`
fn download_verified_asset(
    update: &AvailableUpdate,
    base_url: &str,
    archive_path: &Path,
    download_key: Option<&str>,
    trusted_keys: &[&str],
) -> Result<(), UpdateError> {
    if trusted_keys.is_empty() {
        return Err(UpdateError::SignatureKeyMissing);
    }

    let signature_url = asset_download_url(
        base_url,
        &update.tag,
        &signature_asset_name(&update.asset_name),
    );
    let signature = download_signature(&signature_url, download_key)?;

    let download_url = asset_download_url(base_url, &update.tag, &update.asset_name);
    download_and_verify_with_key(
        &download_url,
        archive_path,
        update.size,
        &update.sha256,
        download_key,
    )?;

    let verified = verify_file_signature(
        archive_path,
        &signature,
        &update.latest_version,
        &update.asset_name,
        trusted_keys,
    );
    if verified.is_err() {
        let _ = fs::remove_file(archive_path);
    }
    verified
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
///
/// 実行ファイルと同じフォルダの同名ファイルを拾わないよう、システムフォルダの絶対パスで起動する
fn launch_msiexec(msi_path: &Path) -> UpdateApplyOutcome {
    let Some(path_str) = msi_path.to_str() else {
        return UpdateError::MsiPathInvalid.into();
    };
    let Some(msiexec) = system_msiexec_path() else {
        return UpdateError::MsiLaunch.into();
    };

    match std::process::Command::new(msiexec)
        .args(["/i", path_str])
        .spawn()
    {
        Ok(_) => UpdateApplyOutcome::MsiStarted,
        Err(_) => UpdateError::MsiLaunch.into(),
    }
}

/// システムフォルダ (`System32`) の `msiexec.exe` の絶対パスを返す
///
/// 環境変数に依存しないよう Known Folders API (`FOLDERID_System`) で解決する
///
/// # Returns
/// * `Option<PathBuf>` - 取得できない場合は `None`
fn system_msiexec_path() -> Option<PathBuf> {
    get_known_folder_path(KnownFolder::System).map(|dir| dir.join("msiexec.exe"))
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

    /// テスト用の更新アセット情報を返す
    fn sample_update(asset_name: &str) -> AvailableUpdate {
        AvailableUpdate {
            current_version: "0.1.2".to_string(),
            latest_version: "0.1.3".to_string(),
            tag: "v0.1.3".to_string(),
            asset_name: asset_name.to_string(),
            size: 1,
            sha256: "a".repeat(64),
        }
    }

    /// `msiexec` をシステムフォルダの絶対パスで解決できるようにすること
    #[test]
    fn system_msiexec_path_is_absolute_and_exists() {
        let path = system_msiexec_path().expect("msiexec path");
        assert!(path.is_absolute());
        assert!(path.is_file(), "{}", path.display());
    }

    /// 不正なアセット名ではダウンロードせずに失敗すること
    #[test]
    fn apply_update_rejects_path_like_asset_name() {
        let update = sample_update("../edge-cut-0.1.3-x86_64.msi");

        // 接続できない URL でも検証で先に失敗するためネットワークへは出ない
        let outcome = apply_update_at(&update, "http://127.0.0.1:9", false, &["unused"]);
        match outcome {
            UpdateApplyOutcome::Error { message } => {
                assert_eq!(message, UpdateError::AssetNameInvalid.to_string());
            }
            other @ UpdateApplyOutcome::MsiStarted => panic!("想定外: {other:?}"),
        }
    }

    /// 公開鍵が埋め込まれていないときはダウンロードせずに失敗すること
    #[test]
    fn download_verified_asset_requires_trusted_key() {
        let update = sample_update("edge-cut-0.1.3-x86_64.msi");
        let archive_path =
            std::env::temp_dir().join(format!("edge-cut-apply-no-key-{}.msi", std::process::id()));

        // 接続できない URL でも鍵の確認で先に失敗するためネットワークへは出ない
        let result =
            download_verified_asset(&update, "http://127.0.0.1:9", &archive_path, None, &[]);
        assert_eq!(result, Err(UpdateError::SignatureKeyMissing));
        assert!(!archive_path.exists());
    }
}
