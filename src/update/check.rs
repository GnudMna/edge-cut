//! GitHub API の最新リリースによる更新確認
//!
//! 取得・`SemVer` 比較・アセット選択・digest / サイズ検証を担う

use semver::Version;

use super::error::UpdateError;
use super::http;
use super::types::{
    AvailableUpdate, LatestRelease, ReleaseAsset, UpdateCheckOutcome, UpdatePackageKind,
};
use super::verify::{parse_asset_digest, validate_asset_name, validate_declared_download_size};
use super::{GITHUB_API_BASE_URL, RELEASE_OWNER, RELEASE_REPO, current_app_version};

// ======================================================================
// パブリック関数
// ======================================================================
/// GitHub の最新リリースで更新の有無を確認する
///
/// ドラフトとプレリリースは GitHub API 側で対象外になる
///
/// # Arguments
/// * `kind` - 選ぶ配布物の種類 (MSI / ポータブル ZIP)
///
/// # Returns
/// * `UpdateCheckOutcome` - 最新 / 更新あり / エラー
pub(crate) fn check_for_update(kind: UpdatePackageKind) -> UpdateCheckOutcome {
    check_for_update_at(
        GITHUB_API_BASE_URL,
        current_app_version(),
        std::env::consts::ARCH,
        kind,
    )
}

/// 指定した API ベース URL で更新の有無を確認する
///
/// # Arguments
/// * `base_url` - GitHub API のベース URL (末尾スラッシュなし)
/// * `current_raw` - 現在のアプリバージョン文字列
/// * `arch` - CPU アーキテクチャ文字列 (例: `x86_64`)
/// * `kind` - 選ぶ配布物の種類
///
/// # Returns
/// * `UpdateCheckOutcome` - 最新 / 更新あり / エラー
pub(crate) fn check_for_update_at(
    base_url: &str,
    current_raw: &str,
    arch: &str,
    kind: UpdatePackageKind,
) -> UpdateCheckOutcome {
    let Ok(current) = parse_semver(current_raw) else {
        return UpdateError::CurrentVersionInvalid {
            version: current_raw.to_string(),
        }
        .into();
    };

    let url = latest_release_url(base_url);
    let response = match fetch_latest_release(&url) {
        Ok(response) => response,
        Err(err) => return err.into(),
    };

    evaluate_update(current_raw, &current, &response, arch, kind)
}

/// 最新リリースの API URL を組み立てる
fn latest_release_url(base_url: &str) -> String {
    format!("{base_url}/repos/{RELEASE_OWNER}/{RELEASE_REPO}/releases/latest")
}

/// 最新リリースを取得してデシリアライズする
///
/// `X-GitHub-Api-Version` は送らない。版のサポート終了後は `410` になり、
/// 配布済みのアプリが更新できなくなるため
fn fetch_latest_release(url: &str) -> Result<LatestRelease, UpdateError> {
    let mut response = http::small_request_agent(url)
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|err| match err {
            // 認証なしの GitHub API は接続元 IP ごとの回数制限の超過を 403 / 429 で返す
            ureq::Error::StatusCode(status @ (403 | 429)) => {
                UpdateError::CheckRateLimited { status }
            }
            ureq::Error::StatusCode(status) => UpdateError::CheckHttp { status },
            err if http::is_timeout(&err) => UpdateError::NetworkTimeout,
            _ => UpdateError::CheckNetwork,
        })?;

    response
        .body_mut()
        .read_json::<LatestRelease>()
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

/// 配布物の種類とアーキテクチャに合うアセットを選ぶ
///
/// 種類のみ一致してアーキテクチャが一致しない場合は `None` を返す
fn select_asset<'a>(
    assets: &'a [ReleaseAsset],
    kind: UpdatePackageKind,
    arch: &str,
) -> Option<&'a ReleaseAsset> {
    assets
        .iter()
        .find(|asset| kind.matches_asset_name(&asset.name) && asset.name.contains(arch))
}

/// 最新リリースの情報から更新結果を判定する
fn evaluate_update(
    current_raw: &str,
    current: &Version,
    response: &LatestRelease,
    arch: &str,
    kind: UpdatePackageKind,
) -> UpdateCheckOutcome {
    // 前後の空白を除去したタグだけを以降の判定・保存先・URL に使う
    let tag = response.tag_name.trim();
    let latest_raw = strip_version_prefix(tag);
    let Ok(latest) = parse_semver(latest_raw) else {
        return UpdateError::LatestVersionInvalid {
            tag: tag.to_string(),
        }
        .into();
    };

    if latest <= *current {
        return UpdateCheckOutcome::UpToDate {
            current_version: current_raw.to_string(),
            latest_version: latest_raw.to_string(),
        };
    }

    let Some(asset) = select_asset(&response.assets, kind, arch) else {
        let has_kind = response
            .assets
            .iter()
            .any(|asset| kind.matches_asset_name(&asset.name));
        let err = if has_kind {
            UpdateError::AssetArchMissing {
                kind: kind.label(),
                arch: arch.to_string(),
                tag: tag.to_string(),
            }
        } else {
            UpdateError::AssetMissing {
                kind: kind.label(),
                tag: tag.to_string(),
            }
        };
        return err.into();
    };

    if let Err(err) = validate_asset_name(&asset.name) {
        return err.into();
    }
    let Some(raw_digest) = asset.digest.as_deref() else {
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
        tag: tag.to_string(),
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

    fn asset(name: &str, size: u64) -> ReleaseAsset {
        ReleaseAsset {
            name: name.to_string(),
            size,
            digest: Some(format!("sha256:{}", "a".repeat(64))),
        }
    }

    fn asset_without_digest(name: &str, size: u64) -> ReleaseAsset {
        ReleaseAsset {
            name: name.to_string(),
            size,
            digest: None,
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

    /// 最新リリースの URL が GitHub API のパスに従うようにすること
    #[test]
    fn latest_release_url_matches_github_api() {
        assert_eq!(
            latest_release_url(GITHUB_API_BASE_URL),
            "https://api.github.com/repos/GnudMna/edge-cut/releases/latest"
        );
    }

    /// GitHub API の JSON をデシリアライズできるようにすること
    #[test]
    fn latest_release_deserializes_github_json() {
        let json = r#"{
            "url":"https://api.github.com/repos/GnudMna/edge-cut/releases/1",
            "tag_name":"v0.1.1",
            "name":"v0.1.1",
            "draft":false,
            "prerelease":false,
            "published_at":"2026-07-04T12:00:00Z",
            "assets":[{"name":"edge-cut-0.1.1-x86_64.msi","label":null,"size":3776512,"content_type":"application/octet-stream","digest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}]
        }"#;
        let release: LatestRelease = serde_json::from_str(json).expect("release JSON");
        assert_eq!(release.tag_name, "v0.1.1");
        assert_eq!(release.assets.len(), 1);
        assert_eq!(release.assets[0].name, "edge-cut-0.1.1-x86_64.msi");
        assert_eq!(release.assets[0].size, 3_776_512);
        assert_eq!(
            release.assets[0].digest.as_deref(),
            Some("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
    }

    /// digest が無い・`null` のアセットもデシリアライズできるようにすること
    #[test]
    fn latest_release_deserializes_without_digest() {
        let json = r#"{
            "tag_name":"v0.1.1",
            "assets":[
                {"name":"edge-cut-0.1.1-x86_64.msi","size":1},
                {"name":"edge-cut-0.1.1-x86_64.msi.minisig","size":1,"digest":null}
            ]
        }"#;
        let release: LatestRelease = serde_json::from_str(json).expect("release JSON");
        assert!(release.assets.iter().all(|asset| asset.digest.is_none()));
    }

    /// 同一バージョンは最新と判定すること
    #[test]
    fn evaluate_update_marks_equal_version_up_to_date() {
        let current = Version::parse("0.1.1").expect("semver");
        let response = LatestRelease {
            tag_name: "v0.1.1".to_string(),
            assets: vec![asset("edge-cut-0.1.1-x86_64.msi", 100)],
        };

        let outcome = evaluate_update(
            "0.1.1",
            &current,
            &response,
            "x86_64",
            UpdatePackageKind::Msi,
        );
        assert!(matches!(outcome, UpdateCheckOutcome::UpToDate { .. }));
    }

    /// 新しい版があればアセット付きで Available になるようにすること
    #[test]
    fn evaluate_update_returns_available_when_newer() {
        let current = Version::parse("0.1.0").expect("semver");
        let response = LatestRelease {
            tag_name: "v0.1.1".to_string(),
            assets: vec![asset("edge-cut-0.1.1-x86_64.msi", 42)],
        };

        let outcome = evaluate_update(
            "0.1.0",
            &current,
            &response,
            "x86_64",
            UpdatePackageKind::Msi,
        );
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

    /// タグ前後の空白を除去した値を `latest_version` と `tag` に使うこと
    #[test]
    fn evaluate_update_trims_tag_whitespace() {
        let current = Version::parse("0.1.0").expect("semver");
        let response = LatestRelease {
            tag_name: " v0.1.1\n".to_string(),
            assets: vec![asset("edge-cut-0.1.1-x86_64.msi", 42)],
        };

        let outcome = evaluate_update(
            "0.1.0",
            &current,
            &response,
            "x86_64",
            UpdatePackageKind::Msi,
        );
        match outcome {
            UpdateCheckOutcome::Available(update) => {
                assert_eq!(update.latest_version, "0.1.1");
                assert_eq!(update.tag, "v0.1.1");
            }
            other => panic!("想定外: {other:?}"),
        }
    }

    /// パスとして解釈されうるアセット名は Available にしないようにすること
    #[test]
    fn evaluate_update_errors_when_asset_name_is_path_like() {
        let current = Version::parse("0.1.0").expect("semver");
        let response = LatestRelease {
            tag_name: "v0.1.1".to_string(),
            assets: vec![asset("../edge-cut-0.1.1-x86_64.msi", 1024)],
        };

        let outcome = evaluate_update(
            "0.1.0",
            &current,
            &response,
            "x86_64",
            UpdatePackageKind::Msi,
        );
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
        let response = LatestRelease {
            tag_name: "v0.1.1".to_string(),
            assets: vec![asset("edge-cut-0.1.1-x86_64.msi", 0)],
        };

        let outcome = evaluate_update(
            "0.1.0",
            &current,
            &response,
            "x86_64",
            UpdatePackageKind::Msi,
        );
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
        let response = LatestRelease {
            tag_name: "v0.1.1".to_string(),
            assets: vec![asset(
                "edge-cut-0.1.1-x86_64.msi",
                MAX_UPDATE_DOWNLOAD_BYTES + 1,
            )],
        };

        let outcome = evaluate_update(
            "0.1.0",
            &current,
            &response,
            "x86_64",
            UpdatePackageKind::Msi,
        );
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
        let response = LatestRelease {
            tag_name: "v0.1.1".to_string(),
            assets: vec![asset_without_digest("edge-cut-0.1.1-x86_64.msi", 42)],
        };

        let outcome = evaluate_update(
            "0.1.0",
            &current,
            &response,
            "x86_64",
            UpdatePackageKind::Msi,
        );
        match outcome {
            UpdateCheckOutcome::Error { message } => {
                assert!(message.contains("digest"));
            }
            other => panic!("想定外: {other:?}"),
        }
    }

    /// MSI アセットが無い場合はエラーになるようにすること
    #[test]
    fn evaluate_update_errors_when_msi_missing() {
        let current = Version::parse("0.1.0").expect("semver");
        let response = LatestRelease {
            tag_name: "v0.1.1".to_string(),
            assets: vec![asset("edge-cut-0.1.1-x86_64.tar.gz", 1)],
        };

        let outcome = evaluate_update(
            "0.1.0",
            &current,
            &response,
            "x86_64",
            UpdatePackageKind::Msi,
        );
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
        let response = LatestRelease {
            tag_name: "v0.1.1".to_string(),
            assets: vec![asset("edge-cut-0.1.1-aarch64.msi", 1)],
        };

        let outcome = evaluate_update(
            "0.1.0",
            &current,
            &response,
            "x86_64",
            UpdatePackageKind::Msi,
        );
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
        let response = LatestRelease {
            tag_name: "v0.1.1".to_string(),
            assets: vec![asset("edge-cut-0.1.1-x86_64.msi", 1)],
        };

        let outcome = evaluate_update(
            "0.2.0",
            &current,
            &response,
            "x86_64",
            UpdatePackageKind::Msi,
        );
        assert!(matches!(outcome, UpdateCheckOutcome::UpToDate { .. }));
    }

    /// 不正なタグはエラーになるようにすること
    #[test]
    fn evaluate_update_errors_on_invalid_tag() {
        let current = Version::parse("0.1.0").expect("semver");
        let response = LatestRelease {
            tag_name: "not-a-version".to_string(),
            assets: vec![asset("edge-cut-0.1.1-x86_64.msi", 1)],
        };

        let outcome = evaluate_update(
            "0.1.0",
            &current,
            &response,
            "x86_64",
            UpdatePackageKind::Msi,
        );
        assert!(matches!(outcome, UpdateCheckOutcome::Error { .. }));
    }

    /// アーキテクチャが一致する MSI を選ぶようにすること
    #[test]
    fn select_msi_asset_requires_matching_arch() {
        let assets = [
            asset("edge-cut-0.1.1-aarch64.msi", 1),
            asset("edge-cut-0.1.1-x86_64.msi", 2),
        ];
        let selected = select_asset(&assets, UpdatePackageKind::Msi, "x86_64").expect("msi");
        assert_eq!(selected.name, "edge-cut-0.1.1-x86_64.msi");
    }

    /// アーキテクチャ不一致時は先頭候補へフォールバックしないようにすること
    #[test]
    fn select_msi_asset_returns_none_when_arch_mismatches() {
        let assets = [asset("edge-cut-0.1.1-aarch64.msi", 1)];
        assert!(select_asset(&assets, UpdatePackageKind::Msi, "x86_64").is_none());
    }

    /// `.MSI` など大文字拡張子も選ぶようにすること
    #[test]
    fn select_msi_asset_accepts_uppercase_extension() {
        let assets = [asset("edge-cut-0.1.1-x86_64.MSI", 3)];
        let selected = select_asset(&assets, UpdatePackageKind::Msi, "x86_64").expect("msi");
        assert_eq!(selected.name, "edge-cut-0.1.1-x86_64.MSI");
    }

    /// ポータブル版は署名ファイルが混ざっていても `-portable.zip` を選ぶようにすること
    #[test]
    fn select_asset_picks_portable_zip() {
        let assets = [
            asset("edge-cut-0.1.1-x86_64.msi", 1),
            asset("edge-cut-0.1.1-x86_64-portable.zip.minisig", 2),
            asset("edge-cut-0.1.1-x86_64-portable.zip", 3),
        ];
        let selected =
            select_asset(&assets, UpdatePackageKind::PortableZip, "x86_64").expect("zip");
        assert_eq!(selected.size, 3);
        let msi = select_asset(&assets, UpdatePackageKind::Msi, "x86_64").expect("msi");
        assert_eq!(msi.size, 1);
    }
}
