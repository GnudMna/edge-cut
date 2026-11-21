//! gh-release-bridge version API による更新確認
//!
//! 取得・`SemVer` 比較・アセット選択・digest / サイズ検証を担う

use std::path::Path;

use super::error::UpdateError;
use super::http;
use super::types::{AvailableUpdate, UpdateCheckOutcome, VersionApiAsset, VersionApiResponse};
use super::verify::{parse_asset_digest, validate_asset_name, validate_declared_download_size};
use super::{BRIDGE_BASE_URL, RELEASE_OWNER, RELEASE_REPO, current_app_version};

use semver::Version;

// ======================================================================
// パブリック関数
// ======================================================================
/// gh-release-bridge の version API で更新の有無を確認する
///
/// # Returns
/// * `UpdateCheckOutcome` - 最新 / 更新あり / エラー
pub(crate) fn check_for_update() -> UpdateCheckOutcome {
    check_for_update_at(
        BRIDGE_BASE_URL,
        current_app_version(),
        std::env::consts::ARCH,
    )
}

/// 指定した bridge ベース URL で更新の有無を確認する
///
/// # Arguments
/// * `base_url` - bridge のベース URL (末尾スラッシュなし)
/// * `current_raw` - 現在のアプリバージョン文字列
/// * `arch` - CPU アーキテクチャ文字列 (例: `x86_64`)
///
/// # Returns
/// * `UpdateCheckOutcome` - 最新 / 更新あり / エラー
pub(crate) fn check_for_update_at(
    base_url: &str,
    current_raw: &str,
    arch: &str,
) -> UpdateCheckOutcome {
    let Ok(current) = parse_semver(current_raw) else {
        return UpdateError::CurrentVersionInvalid {
            version: current_raw.to_string(),
        }
        .into();
    };

    let url = version_api_url(base_url);
    let response = match fetch_version_api(&url) {
        Ok(response) => response,
        Err(err) => return err.into(),
    };

    evaluate_update(current_raw, &current, &response, arch)
}

/// version API の URL を組み立てる
fn version_api_url(base_url: &str) -> String {
    format!("{base_url}/{RELEASE_OWNER}/{RELEASE_REPO}/version")
}

/// version API を取得してデシリアライズする
fn fetch_version_api(url: &str) -> Result<VersionApiResponse, UpdateError> {
    let mut response = http::small_request_agent()
        .get(url)
        .call()
        .map_err(|err| match err {
            ureq::Error::StatusCode(status) => UpdateError::CheckHttp { status },
            err if http::is_timeout(&err) => UpdateError::NetworkTimeout,
            _ => UpdateError::CheckNetwork,
        })?;

    response
        .body_mut()
        .read_json::<VersionApiResponse>()
        .map_err(|err| {
            if http::is_timeout(&err) {
                UpdateError::NetworkTimeout
            } else {
                UpdateError::InvalidJson
            }
        })
}

// ======================================================================
// SemVer / アセット選択 / 判定
// ======================================================================
/// タグやバージョン文字列から先頭の `v` / `V` を除去する
fn strip_version_prefix(value: &str) -> &str {
    value
        .strip_prefix('v')
        .or_else(|| value.strip_prefix('V'))
        .unwrap_or(value)
}

/// `SemVer` 文字列をパースする (`v` 付きでも可)
fn parse_semver(value: &str) -> Result<Version, semver::Error> {
    Version::parse(strip_version_prefix(value.trim()))
}

/// MSI かつアーキテクチャに合うアセットを選ぶ
///
/// 種別のみ一致してアーキテクチャが一致しない場合は `None` を返す
fn select_msi_asset<'a>(assets: &'a [VersionApiAsset], arch: &str) -> Option<&'a VersionApiAsset> {
    assets
        .iter()
        .find(|asset| asset_is_msi(asset) && asset.name.contains(arch))
}

/// アセット名が MSI か判定する
fn asset_is_msi(asset: &VersionApiAsset) -> bool {
    Path::new(&asset.name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("msi"))
}

/// version API レスポンスから更新結果を判定する
fn evaluate_update(
    current_raw: &str,
    current: &Version,
    response: &VersionApiResponse,
    arch: &str,
) -> UpdateCheckOutcome {
    let latest_raw = strip_version_prefix(&response.tag);
    let Ok(latest) = parse_semver(latest_raw) else {
        return UpdateError::LatestVersionInvalid {
            tag: response.tag.clone(),
        }
        .into();
    };

    if latest <= *current {
        return UpdateCheckOutcome::UpToDate {
            current_version: current_raw.to_string(),
            latest_version: latest_raw.to_string(),
        };
    }

    let Some(asset) = select_msi_asset(&response.assets, arch) else {
        let has_msi = response.assets.iter().any(asset_is_msi);
        let err = if has_msi {
            UpdateError::AssetArchMissing {
                arch: arch.to_string(),
                tag: response.tag.clone(),
            }
        } else {
            UpdateError::AssetMissing {
                tag: response.tag.clone(),
            }
        };
        return err.into();
    };

    if let Err(err) = validate_asset_name(&asset.name) {
        return err.into();
    }
    let Some(raw_digest) = asset.integrity_raw() else {
        return UpdateError::DigestMissing {
            asset: asset.name.clone(),
        }
        .into();
    };
    let sha256 = match parse_asset_digest(raw_digest) {
        Ok(digest) => digest,
        Err(err) => return err.into(),
    };
    if let Err(err) = validate_declared_download_size(asset.size) {
        return err.into();
    }

    UpdateCheckOutcome::Available(AvailableUpdate {
        current_version: current_raw.to_string(),
        latest_version: latest_raw.to_string(),
        tag: response.tag.clone(),
        asset_name: asset.name.clone(),
        size: asset.size,
        sha256,
    })
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::super::verify::MAX_UPDATE_DOWNLOAD_BYTES;
    use super::*;

    fn asset(name: &str, size: u64) -> VersionApiAsset {
        VersionApiAsset {
            name: name.to_string(),
            size,
            digest: Some(format!("sha256:{}", "a".repeat(64))),
            sha256: None,
        }
    }

    fn asset_without_digest(name: &str, size: u64) -> VersionApiAsset {
        VersionApiAsset {
            name: name.to_string(),
            size,
            digest: None,
            sha256: None,
        }
    }

    /// タグ先頭の `v` を除去できるようにすること
    #[test]
    fn strip_version_prefix_removes_leading_v() {
        assert_eq!(strip_version_prefix("v0.1.1"), "0.1.1");
        assert_eq!(strip_version_prefix("0.1.1"), "0.1.1");
        assert_eq!(strip_version_prefix("V1.0.0"), "1.0.0");
    }

    /// `SemVer` パースが `v` 付きを受け付けるようにすること
    #[test]
    fn parse_semver_accepts_v_prefix() {
        let version = parse_semver("v1.2.3").expect("semver");
        assert_eq!(version.to_string(), "1.2.3");
    }

    /// version API JSON をデシリアライズできるようにすること
    #[test]
    fn version_api_response_deserializes_bridge_json() {
        let json = r#"{
            "owner":"GnudMna",
            "repo":"edge-cut",
            "tag":"v0.1.1",
            "name":"v0.1.1",
            "prerelease":false,
            "published_at":"2026-07-04T12:00:00Z",
            "assets":[{"name":"edge-cut-0.1.1-x86_64.msi","size":3776512,"content_type":"application/octet-stream","digest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}]
        }"#;
        let response: VersionApiResponse = serde_json::from_str(json).expect("version API JSON");
        assert_eq!(response.tag, "v0.1.1");
        assert_eq!(response.assets.len(), 1);
        assert_eq!(response.assets[0].name, "edge-cut-0.1.1-x86_64.msi");
        assert_eq!(response.assets[0].size, 3_776_512);
        assert_eq!(
            response.assets[0].integrity_raw(),
            Some("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
    }

    /// 同一バージョンは最新と判定すること
    #[test]
    fn evaluate_update_marks_equal_version_up_to_date() {
        let current = Version::parse("0.1.1").expect("semver");
        let response = VersionApiResponse {
            tag: "v0.1.1".to_string(),
            assets: vec![asset("edge-cut-0.1.1-x86_64.msi", 100)],
        };

        let outcome = evaluate_update("0.1.1", &current, &response, "x86_64");
        assert!(matches!(outcome, UpdateCheckOutcome::UpToDate { .. }));
    }

    /// 新しい版があればアセット付きで Available になるようにすること
    #[test]
    fn evaluate_update_returns_available_when_newer() {
        let current = Version::parse("0.1.0").expect("semver");
        let response = VersionApiResponse {
            tag: "v0.1.1".to_string(),
            assets: vec![asset("edge-cut-0.1.1-x86_64.msi", 42)],
        };

        let outcome = evaluate_update("0.1.0", &current, &response, "x86_64");
        match outcome {
            UpdateCheckOutcome::Available(update) => {
                assert_eq!(update.latest_version, "0.1.1");
                assert_eq!(update.tag, "v0.1.1");
                assert_eq!(update.asset_name, "edge-cut-0.1.1-x86_64.msi");
                assert_eq!(update.size, 42);
                assert_eq!(update.sha256, "a".repeat(64));
            }
            other => panic!("想定外: {other:?}"),
        }
    }

    /// パスとして解釈されうるアセット名は Available にしないようにすること
    #[test]
    fn evaluate_update_errors_when_asset_name_is_path_like() {
        let current = Version::parse("0.1.0").expect("semver");
        let response = VersionApiResponse {
            tag: "v0.1.1".to_string(),
            assets: vec![asset("../edge-cut-0.1.1-x86_64.msi", 1024)],
        };

        let outcome = evaluate_update("0.1.0", &current, &response, "x86_64");
        match outcome {
            UpdateCheckOutcome::Error { message } => {
                assert_eq!(message, UpdateError::AssetNameInvalid.to_string());
            }
            other => panic!("想定外: {other:?}"),
        }
    }

    /// 宣言サイズ 0 のアセットは Available にしないようにすること
    #[test]
    fn evaluate_update_errors_when_size_is_zero() {
        let current = Version::parse("0.1.0").expect("semver");
        let response = VersionApiResponse {
            tag: "v0.1.1".to_string(),
            assets: vec![asset("edge-cut-0.1.1-x86_64.msi", 0)],
        };

        let outcome = evaluate_update("0.1.0", &current, &response, "x86_64");
        match outcome {
            UpdateCheckOutcome::Error { message } => {
                assert!(message.contains("サイズ") || message.contains('0'));
            }
            other => panic!("想定外: {other:?}"),
        }
    }

    /// 絶対上限を超える宣言サイズは Available にしないようにすること
    #[test]
    fn evaluate_update_errors_when_size_exceeds_absolute_max() {
        let current = Version::parse("0.1.0").expect("semver");
        let response = VersionApiResponse {
            tag: "v0.1.1".to_string(),
            assets: vec![asset(
                "edge-cut-0.1.1-x86_64.msi",
                MAX_UPDATE_DOWNLOAD_BYTES + 1,
            )],
        };

        let outcome = evaluate_update("0.1.0", &current, &response, "x86_64");
        match outcome {
            UpdateCheckOutcome::Error { message } => {
                assert!(message.contains("上限"));
            }
            other => panic!("想定外: {other:?}"),
        }
    }

    /// digest が無いアセットは Available にしないようにすること
    #[test]
    fn evaluate_update_errors_when_digest_missing() {
        let current = Version::parse("0.1.0").expect("semver");
        let response = VersionApiResponse {
            tag: "v0.1.1".to_string(),
            assets: vec![asset_without_digest("edge-cut-0.1.1-x86_64.msi", 42)],
        };

        let outcome = evaluate_update("0.1.0", &current, &response, "x86_64");
        match outcome {
            UpdateCheckOutcome::Error { message } => {
                assert!(message.contains("digest"));
            }
            other => panic!("想定外: {other:?}"),
        }
    }

    /// `sha256` フィールドだけでも Available になるようにすること
    #[test]
    fn evaluate_update_accepts_sha256_field() {
        let current = Version::parse("0.1.0").expect("semver");
        let hex = "b".repeat(64);
        let response = VersionApiResponse {
            tag: "v0.1.1".to_string(),
            assets: vec![VersionApiAsset {
                name: "edge-cut-0.1.1-x86_64.msi".to_string(),
                size: 10,
                digest: None,
                sha256: Some(hex.clone()),
            }],
        };

        let outcome = evaluate_update("0.1.0", &current, &response, "x86_64");
        match outcome {
            UpdateCheckOutcome::Available(update) => {
                assert_eq!(update.sha256, hex);
            }
            other => panic!("想定外: {other:?}"),
        }
    }

    /// MSI アセットが無い場合はエラーになるようにすること
    #[test]
    fn evaluate_update_errors_when_msi_missing() {
        let current = Version::parse("0.1.0").expect("semver");
        let response = VersionApiResponse {
            tag: "v0.1.1".to_string(),
            assets: vec![asset("edge-cut-0.1.1-x86_64.tar.gz", 1)],
        };

        let outcome = evaluate_update("0.1.0", &current, &response, "x86_64");
        match outcome {
            UpdateCheckOutcome::Error { message } => {
                assert!(message.contains("MSI"));
                assert!(!message.contains("CPU"));
            }
            other => panic!("想定外: {other:?}"),
        }
    }

    /// MSI はあるがアーキテクチャ不一致の場合は CPU 向けエラーになるようにすること
    #[test]
    fn evaluate_update_errors_when_arch_mismatches() {
        let current = Version::parse("0.1.0").expect("semver");
        let response = VersionApiResponse {
            tag: "v0.1.1".to_string(),
            assets: vec![asset("edge-cut-0.1.1-aarch64.msi", 1)],
        };

        let outcome = evaluate_update("0.1.0", &current, &response, "x86_64");
        match outcome {
            UpdateCheckOutcome::Error { message } => {
                assert!(message.contains("CPU"));
                assert!(message.contains("x86_64"));
                assert!(message.contains("MSI"));
            }
            other => panic!("想定外: {other:?}"),
        }
    }

    /// リモートが古い場合も最新と判定すること
    #[test]
    fn evaluate_update_marks_older_remote_up_to_date() {
        let current = Version::parse("0.2.0").expect("semver");
        let response = VersionApiResponse {
            tag: "v0.1.1".to_string(),
            assets: vec![asset("edge-cut-0.1.1-x86_64.msi", 1)],
        };

        let outcome = evaluate_update("0.2.0", &current, &response, "x86_64");
        assert!(matches!(outcome, UpdateCheckOutcome::UpToDate { .. }));
    }

    /// 不正なタグはエラーになるようにすること
    #[test]
    fn evaluate_update_errors_on_invalid_tag() {
        let current = Version::parse("0.1.0").expect("semver");
        let response = VersionApiResponse {
            tag: "not-a-version".to_string(),
            assets: vec![asset("edge-cut-0.1.1-x86_64.msi", 1)],
        };

        let outcome = evaluate_update("0.1.0", &current, &response, "x86_64");
        assert!(matches!(outcome, UpdateCheckOutcome::Error { .. }));
    }

    /// アーキテクチャが一致する MSI を選ぶようにすること
    #[test]
    fn select_msi_asset_requires_matching_arch() {
        let assets = [
            asset("edge-cut-0.1.1-aarch64.msi", 1),
            asset("edge-cut-0.1.1-x86_64.msi", 2),
        ];
        let selected = select_msi_asset(&assets, "x86_64").expect("msi");
        assert_eq!(selected.name, "edge-cut-0.1.1-x86_64.msi");
    }

    /// アーキテクチャ不一致時は先頭候補へフォールバックしないようにすること
    #[test]
    fn select_msi_asset_returns_none_when_arch_mismatches() {
        let assets = [asset("edge-cut-0.1.1-aarch64.msi", 1)];
        assert!(select_msi_asset(&assets, "x86_64").is_none());
    }

    /// `.MSI` など大文字拡張子も選ぶようにすること
    #[test]
    fn select_msi_asset_accepts_uppercase_extension() {
        let assets = [asset("edge-cut-0.1.1-x86_64.MSI", 3)];
        let selected = select_msi_asset(&assets, "x86_64").expect("msi");
        assert_eq!(selected.name, "edge-cut-0.1.1-x86_64.MSI");
    }
}
