//! 更新フローの結合テスト
//!
//! ローカルの HTTP サーバーを GitHub の代わりに立て、
//! 最新リリースの取得からアセット・署名のダウンロード、SHA-256 と minisign 署名の検証までを通しで確認する

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::io::Cursor;
use std::net::TcpListener;
use std::path::{Path, PathBuf};

use minisign::KeyPair;
use sha2::{Digest, Sha256};

use super::check::check_for_update_at;
use super::error::UpdateError;
use super::prepare::prepare_update_at;
use super::signature::expected_trusted_comment;
use super::types::{AvailableUpdate, PreparedPackage, UpdateCheckOutcome, UpdatePackageKind};
use super::{RELEASE_OWNER, RELEASE_REPO};
use server::{Reply, TestServer};

mod server;

// ======================================================================
// 定数
// ======================================================================
/// テスト用リリースの CPU アーキテクチャ
const ARCH: &str = "x86_64";
/// 更新確認の起点とする現在バージョン
const CURRENT_VERSION: &str = "1.0.0";

// ======================================================================
// テスト用リリース
// ======================================================================
/// 1 つのリリースを GitHub の応答として組み立てるための材料
struct Release {
    version: String,
    asset_name: String,
    msi: Vec<u8>,
    key_pair: KeyPair,
}

impl Release {
    /// 指定バージョンのリリースを作る (鍵ペアは毎回新規に生成する)
    fn new(version: &str) -> Self {
        Self {
            version: version.to_string(),
            asset_name: format!("edge-cut-{version}-{ARCH}.msi"),
            msi: format!("msi payload for {version}")
                .repeat(2000)
                .into_bytes(),
            key_pair: KeyPair::generate_unencrypted_keypair().expect("keypair"),
        }
    }

    /// リリースタグ
    fn tag(&self) -> String {
        format!("v{}", self.version)
    }

    /// 署名検証に渡す公開鍵 (Base64)
    fn public_key(&self) -> String {
        self.key_pair.pk.to_base64()
    }

    /// 最新リリース API のパス
    fn latest_release_path() -> String {
        format!("/repos/{RELEASE_OWNER}/{RELEASE_REPO}/releases/latest")
    }

    /// アセット (署名は末尾に `.minisig` を付ける) のダウンロードパス
    fn asset_path(&self) -> String {
        format!(
            "/{RELEASE_OWNER}/{RELEASE_REPO}/releases/download/{}/{}",
            self.tag(),
            self.asset_name
        )
    }

    /// 署名ファイルのダウンロードパス
    fn signature_path(&self) -> String {
        format!("{}.minisig", self.asset_path())
    }

    /// 指定した宣言サイズ・digest を持つ最新リリース API の応答本体
    fn release_json(&self, size: u64, digest: &str) -> Vec<u8> {
        serde_json::json!({
            "tag_name": self.tag(),
            "assets": [{ "name": self.asset_name, "size": size, "digest": digest }],
        })
        .to_string()
        .into_bytes()
    }

    /// 本体に対する正しい digest 文字列
    fn digest_of(body: &[u8]) -> String {
        let hex = Sha256::digest(body)
            .iter()
            .fold(String::new(), |mut hex, byte| {
                let _ = write!(hex, "{byte:02x}");
                hex
            });
        format!("sha256:{hex}")
    }

    /// 指定した鍵・本体・信頼コメントで署名ファイルの内容を作る
    fn sign(key_pair: &KeyPair, body: &[u8], trusted_comment: &str) -> Vec<u8> {
        minisign::sign(
            Some(&key_pair.pk),
            &key_pair.sk,
            Cursor::new(body),
            Some(trusted_comment),
            None,
        )
        .expect("sign")
        .into_string()
        .into_bytes()
    }

    /// すべて正しい署名・digest・サイズの応答表を作る
    fn routes(&self) -> HashMap<String, Reply> {
        let signature = Self::sign(
            &self.key_pair,
            &self.msi,
            &expected_trusted_comment(&self.version, &self.asset_name),
        );
        let release_json = self.release_json(self.msi.len() as u64, &Self::digest_of(&self.msi));
        HashMap::from([
            (Self::latest_release_path(), Reply::ok(release_json)),
            (self.asset_path(), Reply::ok(self.msi.clone())),
            (self.signature_path(), Reply::ok(signature)),
        ])
    }
}

/// テストごとに独立した更新作業フォルダの親を用意し、破棄時に削除する
///
/// 実環境の `%TEMP%\edge-cut-update` を使わないため、並列実行や起動中のアプリと衝突しない
struct WorkDirGuard {
    root: tempfile::TempDir,
    dir: PathBuf,
}

impl WorkDirGuard {
    /// 指定バージョンの作業フォルダを持つ一時ディレクトリを作る
    fn new(version: &str) -> Self {
        let root = tempfile::tempdir().expect("tempdir");
        let dir = root.path().join(version);
        Self { root, dir }
    }

    /// 作業フォルダの親ディレクトリ
    fn root(&self) -> &Path {
        self.root.path()
    }

    /// 作業フォルダ内のアセットのパス
    fn archive(&self, asset_name: &str) -> PathBuf {
        self.dir.join(asset_name)
    }
}

/// 更新確認の結果が更新ありであることを確かめて中身を返す
fn expect_available(outcome: UpdateCheckOutcome) -> AvailableUpdate {
    match outcome {
        UpdateCheckOutcome::Available(update) => update,
        other => panic!("更新ありを期待したが {other:?}"),
    }
}

/// 更新確認の結果が指定したエラーであることを確かめる
fn assert_check_error(outcome: &UpdateCheckOutcome, expected: &UpdateError) {
    match outcome {
        UpdateCheckOutcome::Error { message } => assert_eq!(message, &expected.to_string()),
        other => panic!("{expected:?} を期待したが {other:?}"),
    }
}

/// ダウンロード・検証の結果が指定したエラーであることを確かめる
fn assert_apply_error(outcome: &Result<PreparedPackage, UpdateError>, expected: &UpdateError) {
    assert_eq!(outcome, &Err(expected.clone()));
}

/// 更新確認からダウンロード・検証までを通しで実行する
fn check_and_apply(
    server: &TestServer,
    guard: &WorkDirGuard,
    trusted_keys: &[&str],
) -> (AvailableUpdate, Result<PreparedPackage, UpdateError>) {
    let update = expect_available(check_for_update_at(
        &server.base_url,
        CURRENT_VERSION,
        ARCH,
        UpdatePackageKind::Msi,
    ));
    let outcome = prepare_update_at(
        &update,
        &server.base_url,
        guard.root(),
        trusted_keys,
        &mut |_| true,
    );
    (update, outcome)
}

// ======================================================================
// テスト: 正常系
// ======================================================================
/// 最新リリース API から署名検証まで通り、検証済みの MSI が保存されること
///
/// 署名はアセット本体より先に取得する
#[test]
fn full_update_flow_saves_verified_msi() {
    let release = Release::new("90.0.1");
    let guard = WorkDirGuard::new(&release.version);
    let server = TestServer::start(release.routes());

    let (update, outcome) = check_and_apply(&server, &guard, &[&release.public_key()]);

    // 確認結果が 最新リリース API の内容と一致する
    assert_eq!(update.current_version, CURRENT_VERSION);
    assert_eq!(update.latest_version, release.version);
    assert_eq!(update.tag, release.tag());
    assert_eq!(update.asset_name, release.asset_name);
    assert_eq!(update.size, release.msi.len() as u64);
    assert_eq!(
        format!("sha256:{}", update.sha256),
        Release::digest_of(&release.msi)
    );
    // 適用に成功し、検証済みのバイト列がそのまま保存される
    assert_eq!(
        outcome,
        Ok(PreparedPackage::Msi {
            path: guard.archive(&release.asset_name)
        })
    );
    assert_eq!(
        fs::read(guard.archive(&release.asset_name)).expect("read archive"),
        release.msi
    );
    // 署名 → アセットの順に取得する
    assert_eq!(
        server.requested_paths(),
        [
            Release::latest_release_path(),
            release.signature_path(),
            release.asset_path(),
        ]
    );
}

/// 現在より古い・同じバージョンの最新タグは更新なしと判定すること
#[test]
fn check_reports_up_to_date_for_same_or_older_release() {
    let release = Release::new("90.0.2");
    let server = TestServer::start(release.routes());

    for current in ["90.0.2", "v90.0.2", "90.1.0"] {
        let outcome = check_for_update_at(&server.base_url, current, ARCH, UpdatePackageKind::Msi);
        match outcome {
            UpdateCheckOutcome::UpToDate { latest_version, .. } => {
                assert_eq!(latest_version, "90.0.2", "{current}");
            }
            other => panic!("{current}: 更新なしを期待したが {other:?}"),
        }
    }
}

// ======================================================================
// テスト: 最新リリース API の異常系
// ======================================================================
/// 最新リリース API が HTTP エラーを返したら、ステータスつきの確認エラーになること
#[test]
fn check_reports_http_status_from_latest_release_api() {
    for status in [404, 500, 503] {
        let routes = HashMap::from([(Release::latest_release_path(), Reply::status(status))]);
        let server = TestServer::start(routes);

        let outcome = check_for_update_at(
            &server.base_url,
            CURRENT_VERSION,
            ARCH,
            UpdatePackageKind::Msi,
        );
        assert_check_error(&outcome, &UpdateError::CheckHttp { status });
    }
}

/// 回数制限などによる拒否 (403 / 429) は、時間をおくよう案内する確認エラーになること
#[test]
fn check_reports_rate_limit_from_latest_release_api() {
    for status in [403, 429] {
        let routes = HashMap::from([(Release::latest_release_path(), Reply::status(status))]);
        let server = TestServer::start(routes);

        let outcome = check_for_update_at(
            &server.base_url,
            CURRENT_VERSION,
            ARCH,
            UpdatePackageKind::Msi,
        );
        assert_check_error(&outcome, &UpdateError::CheckRateLimited { status });
    }
}

/// 最新リリース API が JSON として解釈できない応答を返したら、形式不正のエラーになること
#[test]
fn check_reports_invalid_json_from_latest_release_api() {
    for body in [
        "",
        "not json",
        "{\"assets\": []}",
        "<html>bad gateway</html>",
    ] {
        let routes = HashMap::from([(Release::latest_release_path(), Reply::ok(body))]);
        let server = TestServer::start(routes);

        let outcome = check_for_update_at(
            &server.base_url,
            CURRENT_VERSION,
            ARCH,
            UpdatePackageKind::Msi,
        );
        assert_check_error(&outcome, &UpdateError::InvalidJson);
    }
}

/// 接続できないサーバーでは、ネットワーク失敗の確認エラーになること
#[test]
fn check_reports_network_error_when_server_is_down() {
    // 一度待ち受けたポートを閉じて、接続拒否になるアドレスを得る
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let base_url = format!("http://{}", listener.local_addr().expect("local_addr"));
    drop(listener);

    let outcome = check_for_update_at(&base_url, CURRENT_VERSION, ARCH, UpdatePackageKind::Msi);
    assert_check_error(&outcome, &UpdateError::CheckNetwork);
}

/// 最新リリース API の内容が不正なら、ダウンロードへ進まず確認エラーになること
///
/// タグ・アセット名・digest・サイズ・アーキテクチャの各不正を試す
#[test]
fn check_rejects_invalid_release_metadata() {
    let release = Release::new("90.0.3");
    let digest = Release::digest_of(&release.msi);
    let size = release.msi.len() as u64;

    let check = |body: Vec<u8>| {
        let routes = HashMap::from([(Release::latest_release_path(), Reply::ok(body))]);
        let server = TestServer::start(routes);
        check_for_update_at(
            &server.base_url,
            CURRENT_VERSION,
            ARCH,
            UpdatePackageKind::Msi,
        )
    };

    // タグが SemVer ではない
    let bad_tag = serde_json::json!({ "tag_name": "latest", "assets": [] }).to_string();
    assert_check_error(
        &check(bad_tag.into_bytes()),
        &UpdateError::LatestVersionInvalid {
            tag: "latest".to_string(),
        },
    );
    // MSI アセットが無い
    let no_msi = serde_json::json!({
        "tag_name": release.tag(),
        "assets": [{ "name": "readme.txt", "size": 1, "digest": digest }],
    })
    .to_string();
    assert_check_error(
        &check(no_msi.into_bytes()),
        &UpdateError::AssetMissing {
            kind: "MSI",
            tag: release.tag(),
        },
    );
    // この CPU 向けの MSI が無い
    let other_arch = serde_json::json!({
        "tag_name": release.tag(),
        "assets": [{ "name": "edge-cut-90.0.3-aarch64.msi", "size": size, "digest": digest }],
    })
    .to_string();
    assert_check_error(
        &check(other_arch.into_bytes()),
        &UpdateError::AssetArchMissing {
            kind: "MSI",
            arch: ARCH.to_string(),
            tag: release.tag(),
        },
    );
    // アセット名がパスを含む
    let traversal = serde_json::json!({
        "tag_name": release.tag(),
        "assets": [{ "name": "..\\x86_64.msi", "size": size, "digest": digest }],
    })
    .to_string();
    assert_check_error(
        &check(traversal.into_bytes()),
        &UpdateError::AssetNameInvalid,
    );
    // digest が無い
    let no_digest = serde_json::json!({
        "tag_name": release.tag(),
        "assets": [{ "name": release.asset_name, "size": size }],
    })
    .to_string();
    assert_check_error(
        &check(no_digest.into_bytes()),
        &UpdateError::DigestMissing {
            asset: release.asset_name.clone(),
        },
    );
    // digest が 16 進 64 桁ではない
    assert_check_error(
        &check(release.release_json(size, "sha256:zzzz")),
        &UpdateError::DigestInvalid {
            raw: "sha256:zzzz".to_string(),
        },
    );
    // 宣言サイズが 0
    assert_check_error(
        &check(release.release_json(0, &digest)),
        &UpdateError::SizeZero,
    );
}

// ======================================================================
// テスト: 署名ファイルの異常系
// ======================================================================
/// 署名ファイルが無いときは、アセット本体をダウンロードせずに失敗すること
#[test]
fn missing_signature_blocks_asset_download() {
    let release = Release::new("90.0.4");
    let guard = WorkDirGuard::new(&release.version);
    let mut routes = release.routes();
    routes.remove(&release.signature_path());
    let server = TestServer::start(routes);

    let (_, outcome) = check_and_apply(&server, &guard, &[&release.public_key()]);

    assert_apply_error(
        &outcome,
        &UpdateError::SignatureDownloadHttp { status: 404 },
    );
    // アセット本体は要求されず、保存もされない
    assert!(!server.requested_paths().contains(&release.asset_path()));
    assert!(!guard.archive(&release.asset_name).exists());
}

/// 署名ファイルが上限を超えて大きいときは、取得失敗として中断すること
#[test]
fn oversized_signature_is_rejected() {
    let release = Release::new("90.0.5");
    let guard = WorkDirGuard::new(&release.version);
    let mut routes = release.routes();
    routes.insert(release.signature_path(), Reply::ok(vec![b'a'; 64 * 1024]));
    let server = TestServer::start(routes);

    let (_, outcome) = check_and_apply(&server, &guard, &[&release.public_key()]);

    assert_apply_error(&outcome, &UpdateError::SignatureDownloadFailed);
    assert!(!server.requested_paths().contains(&release.asset_path()));
    assert!(!guard.archive(&release.asset_name).exists());
}

/// 署名ファイルが minisign 形式ではないときは、署名不正で失敗し MSI を残さないこと
#[test]
fn malformed_signature_deletes_downloaded_msi() {
    let release = Release::new("90.0.6");
    let guard = WorkDirGuard::new(&release.version);
    let mut routes = release.routes();
    routes.insert(release.signature_path(), Reply::ok("garbage signature"));
    let server = TestServer::start(routes);

    let (_, outcome) = check_and_apply(&server, &guard, &[&release.public_key()]);

    assert_apply_error(&outcome, &UpdateError::SignatureInvalid);
    assert!(!guard.archive(&release.asset_name).exists());
}

// ======================================================================
// テスト: アセット本体の異常系
// ======================================================================
/// アセットのダウンロードが HTTP エラーになったら、ステータスつきの失敗になること
#[test]
fn asset_http_error_is_reported() {
    let release = Release::new("90.0.7");
    let guard = WorkDirGuard::new(&release.version);
    let mut routes = release.routes();
    routes.insert(release.asset_path(), Reply::status(500));
    let server = TestServer::start(routes);

    let (_, outcome) = check_and_apply(&server, &guard, &[&release.public_key()]);

    assert_apply_error(&outcome, &UpdateError::DownloadHttp { status: 500 });
    assert!(!guard.archive(&release.asset_name).exists());
}

/// 同じサイズに改ざんされたアセットは、SHA-256 不一致で失敗し MSI を残さないこと
#[test]
fn tampered_asset_fails_integrity_check() {
    let release = Release::new("90.0.8");
    let guard = WorkDirGuard::new(&release.version);
    let mut tampered = release.msi.clone();
    tampered[0] ^= 0xFF;
    let mut routes = release.routes();
    routes.insert(release.asset_path(), Reply::ok(tampered));
    let server = TestServer::start(routes);

    let (_, outcome) = check_and_apply(&server, &guard, &[&release.public_key()]);

    assert_apply_error(&outcome, &UpdateError::IntegrityFailed);
    assert!(!guard.archive(&release.asset_name).exists());
}

/// 宣言より小さいアセットは、サイズ不一致で失敗し MSI を残さないこと
#[test]
fn shorter_asset_fails_size_check() {
    let release = Release::new("90.0.9");
    let guard = WorkDirGuard::new(&release.version);
    let short = release.msi[..release.msi.len() - 10].to_vec();
    let actual = short.len() as u64;
    let mut routes = release.routes();
    routes.insert(release.asset_path(), Reply::ok(short));
    let server = TestServer::start(routes);

    let (update, outcome) = check_and_apply(&server, &guard, &[&release.public_key()]);

    assert_apply_error(
        &outcome,
        &UpdateError::SizeMismatch {
            expected: update.size,
            actual,
        },
    );
    assert!(!guard.archive(&release.asset_name).exists());
}

/// 宣言より大きいアセットは、宣言サイズで読み取りを打ち切って失敗すること
#[test]
fn longer_asset_is_cut_off_at_declared_size() {
    let release = Release::new("90.0.10");
    let guard = WorkDirGuard::new(&release.version);
    let mut longer = release.msi.clone();
    longer.extend_from_slice(&[0u8; 4096]);
    let mut routes = release.routes();
    routes.insert(release.asset_path(), Reply::ok(longer));
    let server = TestServer::start(routes);

    let (update, outcome) = check_and_apply(&server, &guard, &[&release.public_key()]);

    assert_apply_error(
        &outcome,
        &UpdateError::SizeMismatchTruncated {
            expected: update.size,
        },
    );
    assert!(!guard.archive(&release.asset_name).exists());
}

/// 転送の途中で接続が切れたアセットは、ダウンロード失敗になり MSI を残さないこと
#[test]
fn interrupted_asset_download_fails() {
    let release = Release::new("90.0.11");
    let guard = WorkDirGuard::new(&release.version);
    let mut routes = release.routes();
    let mut interrupted = Reply::ok(release.msi[..release.msi.len() / 2].to_vec());
    interrupted.declared_len = Some(release.msi.len());
    routes.insert(release.asset_path(), interrupted);
    let server = TestServer::start(routes);

    let (_, outcome) = check_and_apply(&server, &guard, &[&release.public_key()]);

    assert_apply_error(&outcome, &UpdateError::DownloadFailed);
    assert!(!guard.archive(&release.asset_name).exists());
}

// ======================================================================
// テスト: 署名検証の異常系
// ======================================================================
/// 信頼しない鍵で署名されたアセットは、digest が一致しても拒否して MSI を残さないこと
#[test]
fn asset_signed_by_untrusted_key_is_rejected() {
    let release = Release::new("90.0.12");
    let guard = WorkDirGuard::new(&release.version);
    let attacker = KeyPair::generate_unencrypted_keypair().expect("keypair");
    let mut routes = release.routes();
    routes.insert(
        release.signature_path(),
        Reply::ok(Release::sign(
            &attacker,
            &release.msi,
            &expected_trusted_comment(&release.version, &release.asset_name),
        )),
    );
    let server = TestServer::start(routes);

    let (_, outcome) = check_and_apply(&server, &guard, &[&release.public_key()]);

    assert_apply_error(&outcome, &UpdateError::SignatureUntrustedKey);
    assert!(!guard.archive(&release.asset_name).exists());
}

/// 最新リリース API の digest ごと差し替えられた MSI は、署名不一致で拒否すること
///
/// 配信経路を握る攻撃者が改ざんした MSI の digest を 最新リリース API に書き込んでも、
/// 正規の秘密鍵による署名は作れないため適用されない
#[test]
fn replaced_asset_with_matching_digest_fails_signature() {
    let release = Release::new("90.0.13");
    let guard = WorkDirGuard::new(&release.version);
    let mut malicious = release.msi.clone();
    malicious[0] ^= 0xFF;
    let mut routes = release.routes();
    routes.insert(
        Release::latest_release_path(),
        Reply::ok(release.release_json(malicious.len() as u64, &Release::digest_of(&malicious))),
    );
    routes.insert(release.asset_path(), Reply::ok(malicious));
    let server = TestServer::start(routes);

    let (_, outcome) = check_and_apply(&server, &guard, &[&release.public_key()]);

    // サイズと SHA-256 は通るが、署名が元の MSI のものなので拒否される
    assert_apply_error(&outcome, &UpdateError::SignatureInvalid);
    assert!(!guard.archive(&release.asset_name).exists());
}

/// 別バージョン向けに署名された正規の署名は、リプレイとして拒否すること
#[test]
fn signature_for_another_version_is_rejected() {
    let release = Release::new("90.0.14");
    let guard = WorkDirGuard::new(&release.version);
    let mut routes = release.routes();
    routes.insert(
        release.signature_path(),
        Reply::ok(Release::sign(
            &release.key_pair,
            &release.msi,
            &expected_trusted_comment("89.0.0", &release.asset_name),
        )),
    );
    let server = TestServer::start(routes);

    let (_, outcome) = check_and_apply(&server, &guard, &[&release.public_key()]);

    assert_apply_error(&outcome, &UpdateError::SignatureMetadataMismatch);
    assert!(!guard.archive(&release.asset_name).exists());
}

/// 公開鍵が 1 つも無いビルドでは、ネットワークへ出ずに適用を拒否すること
#[test]
fn apply_without_trusted_keys_makes_no_download_request() {
    let release = Release::new("90.0.15");
    let guard = WorkDirGuard::new(&release.version);
    let server = TestServer::start(release.routes());

    let (_, outcome) = check_and_apply(&server, &guard, &[]);

    assert_apply_error(&outcome, &UpdateError::SignatureKeyMissing);
    // 確認用の 最新リリース API 以外は要求されない
    assert_eq!(server.requested_paths(), [Release::latest_release_path()]);
    assert!(!guard.archive(&release.asset_name).exists());
}
