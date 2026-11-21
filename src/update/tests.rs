//! 更新フローの結合テスト
//!
//! ローカルの HTTP サーバーを gh-release-bridge の代わりに立て、
//! version API の取得からアセット・署名のダウンロード、SHA-256 と minisign 署名の検証までを通しで確認する

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::io::{Cursor, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use minisign::KeyPair;
use sha2::{Digest, Sha256};

use super::apply::apply_update_at;
use super::check::check_for_update_at;
use super::error::UpdateError;
use super::signature::{download_signature, expected_trusted_comment};
use super::types::{AvailableUpdate, UpdateApplyOutcome, UpdateCheckOutcome};
use super::verify::download_and_verify_with_key;
use super::{RELEASE_OWNER, RELEASE_REPO};

// ======================================================================
// 定数
// ======================================================================
/// テスト用リリースの CPU アーキテクチャ
const ARCH: &str = "x86_64";
/// 更新確認の起点とする現在バージョン
const CURRENT_VERSION: &str = "1.0.0";
/// 接続ごとのリクエスト読み取りタイムアウト
const SERVER_READ_TIMEOUT: Duration = Duration::from_secs(5);
/// リクエストヘッダーとして読み込む最大バイト数
const MAX_REQUEST_HEAD_BYTES: usize = 16 * 1024;

// ======================================================================
// テスト用 HTTP サーバー
// ======================================================================
/// サーバーが受け取ったリクエストの記録
#[derive(Debug, Clone)]
struct RecordedRequest {
    /// リクエストパス
    path: String,
    /// `Authorization` ヘッダーの値
    authorization: Option<String>,
}

/// パスごとに返す応答
#[derive(Debug, Clone)]
struct Reply {
    /// HTTP ステータスコード
    status: u16,
    /// 応答本体
    body: Vec<u8>,
    /// `Content-Length` に宣言する値 (`None` なら本体の長さ)
    ///
    /// 本体より大きい値を宣言すると、途中で接続が切れた応答を再現できる
    declared_len: Option<usize>,
    /// 要求する `Authorization` ヘッダーの値 (一致しなければ `401` を返す)
    required_authorization: Option<String>,
}

impl Reply {
    /// 本体付きの `200` 応答を作る
    fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            body: body.into(),
            declared_len: None,
            required_authorization: None,
        }
    }

    /// 本体なしのエラー応答を作る
    fn status(status: u16) -> Self {
        Self {
            status,
            ..Self::ok(Vec::new())
        }
    }

    /// 指定した `Authorization` ヘッダーを要求する
    fn requiring(mut self, authorization: &str) -> Self {
        self.required_authorization = Some(authorization.to_string());
        self
    }
}

/// `127.0.0.1` の空きポートで待ち受ける最小の HTTP/1.1 サーバー
///
/// 接続ごとに 1 リクエストだけ処理して閉じる。破棄時にスレッドを止める
struct TestServer {
    base_url: String,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    stop: Arc<AtomicBool>,
    addr: std::net::SocketAddr,
    handle: Option<JoinHandle<()>>,
}

impl TestServer {
    /// 指定した応答表でサーバーを起動する
    fn start(routes: HashMap<String, Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local_addr");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));

        let handle = {
            let requests = Arc::clone(&requests);
            let stop = Arc::clone(&stop);
            thread::spawn(move || {
                for stream in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    if let Ok(stream) = stream {
                        serve_connection(stream, &routes, &requests);
                    }
                }
            })
        };

        Self {
            base_url: format!("http://{addr}"),
            requests,
            stop,
            addr,
            handle: Some(handle),
        }
    }

    /// 受け取ったリクエストのパスを到着順に返す
    fn requested_paths(&self) -> Vec<String> {
        self.requests
            .lock()
            .expect("lock")
            .iter()
            .map(|request| request.path.clone())
            .collect()
    }

    /// 受け取ったリクエストの `Authorization` ヘッダーを到着順に返す
    fn requested_authorizations(&self) -> Vec<Option<String>> {
        self.requests
            .lock()
            .expect("lock")
            .iter()
            .map(|request| request.authorization.clone())
            .collect()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // `accept` で待機中のスレッドを起こして終了させる
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// 1 接続分のリクエストを読み、応答表に従って返す
fn serve_connection(
    mut stream: TcpStream,
    routes: &HashMap<String, Reply>,
    requests: &Mutex<Vec<RecordedRequest>>,
) {
    let _ = stream.set_read_timeout(Some(SERVER_READ_TIMEOUT));
    let Some(head) = read_request_head(&mut stream) else {
        return;
    };

    let mut lines = head.lines();
    let path = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or_default()
        .to_string();
    let authorization = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
        .map(|(_, value)| value.trim().to_string());
    requests.lock().expect("lock").push(RecordedRequest {
        path: path.clone(),
        authorization: authorization.clone(),
    });

    let reply = match routes.get(&path) {
        None => Reply::status(404),
        Some(reply)
            if reply
                .required_authorization
                .as_ref()
                .is_some_and(|required| authorization.as_ref() != Some(required)) =>
        {
            Reply::status(401)
        }
        Some(reply) => reply.clone(),
    };

    let declared = reply.declared_len.unwrap_or(reply.body.len());
    let head = format!(
        "HTTP/1.1 {} Test\r\nContent-Length: {declared}\r\nConnection: close\r\n\r\n",
        reply.status,
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&reply.body);
    let _ = stream.flush();
    let _ = stream.shutdown(Shutdown::Both);
}

/// 空行までのリクエストヘッダーを文字列で読む
fn read_request_head(stream: &mut TcpStream) -> Option<String> {
    let mut head = Vec::new();
    let mut chunk = [0u8; 1024];
    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 || head.len() > MAX_REQUEST_HEAD_BYTES {
            return None;
        }
        head.extend_from_slice(&chunk[..n]);
    }
    Some(String::from_utf8_lossy(&head).into_owned())
}

// ======================================================================
// テスト用リリース
// ======================================================================
/// 1 つのリリースを bridge の応答として組み立てるための材料
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

    /// version API のパス
    fn version_path() -> String {
        format!("/{RELEASE_OWNER}/{RELEASE_REPO}/version")
    }

    /// アセット (署名は末尾に `.minisig` を付ける) のダウンロードパス
    fn asset_path(&self) -> String {
        format!(
            "/{RELEASE_OWNER}/{RELEASE_REPO}/{}/{}",
            self.tag(),
            self.asset_name
        )
    }

    /// 署名ファイルのダウンロードパス
    fn signature_path(&self) -> String {
        format!("{}.minisig", self.asset_path())
    }

    /// 指定した宣言サイズ・digest を持つ version API の応答本体
    fn version_json(&self, size: u64, digest: &str) -> Vec<u8> {
        serde_json::json!({
            "tag": self.tag(),
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
        let version_json = self.version_json(self.msi.len() as u64, &Self::digest_of(&self.msi));
        HashMap::from([
            (Self::version_path(), Reply::ok(version_json)),
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

/// 更新適用の結果が指定したエラーであることを確かめる
fn assert_apply_error(outcome: &UpdateApplyOutcome, expected: &UpdateError) {
    match outcome {
        UpdateApplyOutcome::Error { message } => assert_eq!(message, &expected.to_string()),
        UpdateApplyOutcome::MsiStarted => panic!("{expected:?} を期待したが適用に成功した"),
    }
}

/// 更新確認から適用 (`msiexec` は起動しない) までを通しで実行する
fn check_and_apply(
    server: &TestServer,
    guard: &WorkDirGuard,
    trusted_keys: &[&str],
) -> (AvailableUpdate, UpdateApplyOutcome) {
    let update = expect_available(check_for_update_at(&server.base_url, CURRENT_VERSION, ARCH));
    let outcome = apply_update_at(&update, &server.base_url, guard.root(), false, trusted_keys);
    (update, outcome)
}

// ======================================================================
// テスト: 正常系
// ======================================================================
/// version API から署名検証まで通り、検証済みの MSI が保存されること
///
/// 署名はアセット本体より先に取得する
#[test]
fn full_update_flow_saves_verified_msi() {
    let release = Release::new("90.0.1");
    let guard = WorkDirGuard::new(&release.version);
    let server = TestServer::start(release.routes());

    let (update, outcome) = check_and_apply(&server, &guard, &[&release.public_key()]);

    // 確認結果が version API の内容と一致する
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
    assert!(
        matches!(outcome, UpdateApplyOutcome::MsiStarted),
        "{outcome:?}"
    );
    assert_eq!(
        fs::read(guard.archive(&release.asset_name)).expect("read archive"),
        release.msi
    );
    // 署名 → アセットの順に取得する
    assert_eq!(
        server.requested_paths(),
        [
            Release::version_path(),
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
        let outcome = check_for_update_at(&server.base_url, current, ARCH);
        match outcome {
            UpdateCheckOutcome::UpToDate { latest_version, .. } => {
                assert_eq!(latest_version, "90.0.2", "{current}");
            }
            other => panic!("{current}: 更新なしを期待したが {other:?}"),
        }
    }
}

// ======================================================================
// テスト: version API の異常系
// ======================================================================
/// version API が HTTP エラーを返したら、ステータスつきの確認エラーになること
#[test]
fn check_reports_http_status_from_version_api() {
    for status in [404, 500, 503] {
        let routes = HashMap::from([(Release::version_path(), Reply::status(status))]);
        let server = TestServer::start(routes);

        let outcome = check_for_update_at(&server.base_url, CURRENT_VERSION, ARCH);
        assert_check_error(&outcome, &UpdateError::CheckHttp { status });
    }
}

/// version API が JSON として解釈できない応答を返したら、形式不正のエラーになること
#[test]
fn check_reports_invalid_json_from_version_api() {
    for body in [
        "",
        "not json",
        "{\"assets\": []}",
        "<html>bad gateway</html>",
    ] {
        let routes = HashMap::from([(Release::version_path(), Reply::ok(body))]);
        let server = TestServer::start(routes);

        let outcome = check_for_update_at(&server.base_url, CURRENT_VERSION, ARCH);
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

    let outcome = check_for_update_at(&base_url, CURRENT_VERSION, ARCH);
    assert_check_error(&outcome, &UpdateError::CheckNetwork);
}

/// version API の内容が不正なら、ダウンロードへ進まず確認エラーになること
///
/// タグ・アセット名・digest・サイズ・アーキテクチャの各不正を試す
#[test]
fn check_rejects_invalid_release_metadata() {
    let release = Release::new("90.0.3");
    let digest = Release::digest_of(&release.msi);
    let size = release.msi.len() as u64;

    let check = |body: Vec<u8>| {
        let routes = HashMap::from([(Release::version_path(), Reply::ok(body))]);
        let server = TestServer::start(routes);
        check_for_update_at(&server.base_url, CURRENT_VERSION, ARCH)
    };

    // タグが SemVer ではない
    let bad_tag = serde_json::json!({ "tag": "latest", "assets": [] }).to_string();
    assert_check_error(
        &check(bad_tag.into_bytes()),
        &UpdateError::LatestVersionInvalid {
            tag: "latest".to_string(),
        },
    );
    // MSI アセットが無い
    let no_msi = serde_json::json!({
        "tag": release.tag(),
        "assets": [{ "name": "readme.txt", "size": 1, "digest": digest }],
    })
    .to_string();
    assert_check_error(
        &check(no_msi.into_bytes()),
        &UpdateError::AssetMissing { tag: release.tag() },
    );
    // この CPU 向けの MSI が無い
    let other_arch = serde_json::json!({
        "tag": release.tag(),
        "assets": [{ "name": "edge-cut-90.0.3-aarch64.msi", "size": size, "digest": digest }],
    })
    .to_string();
    assert_check_error(
        &check(other_arch.into_bytes()),
        &UpdateError::AssetArchMissing {
            arch: ARCH.to_string(),
            tag: release.tag(),
        },
    );
    // アセット名がパスを含む
    let traversal = serde_json::json!({
        "tag": release.tag(),
        "assets": [{ "name": "..\\x86_64.msi", "size": size, "digest": digest }],
    })
    .to_string();
    assert_check_error(
        &check(traversal.into_bytes()),
        &UpdateError::AssetNameInvalid,
    );
    // digest が無い
    let no_digest = serde_json::json!({
        "tag": release.tag(),
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
        &check(release.version_json(size, "sha256:zzzz")),
        &UpdateError::DigestInvalid {
            raw: "sha256:zzzz".to_string(),
        },
    );
    // 宣言サイズが 0
    assert_check_error(
        &check(release.version_json(0, &digest)),
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

/// version API の digest ごと差し替えられた MSI は、署名不一致で拒否すること
///
/// 配信経路を握る攻撃者が改ざんした MSI の digest を version API に書き込んでも、
/// 正規の秘密鍵による署名は作れないため適用されない
#[test]
fn replaced_asset_with_matching_digest_fails_signature() {
    let release = Release::new("90.0.13");
    let guard = WorkDirGuard::new(&release.version);
    let mut malicious = release.msi.clone();
    malicious[0] ^= 0xFF;
    let mut routes = release.routes();
    routes.insert(
        Release::version_path(),
        Reply::ok(release.version_json(malicious.len() as u64, &Release::digest_of(&malicious))),
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
    // 確認用の version API 以外は要求されない
    assert_eq!(server.requested_paths(), [Release::version_path()]);
    assert!(!guard.archive(&release.asset_name).exists());
}

// ======================================================================
// テスト: ダウンロードキー
// ======================================================================
/// ダウンロードキーを `Bearer` で署名とアセットの両方に付け、無い・違うキーは認証失敗になること
#[test]
fn download_key_is_sent_as_bearer_and_enforced() {
    let release = Release::new("90.0.16");
    let guard = WorkDirGuard::new(&release.version);
    let bearer = "Bearer secret-key";
    let mut routes = release.routes();
    for path in [release.signature_path(), release.asset_path()] {
        let reply = routes.remove(&path).expect("route").requiring(bearer);
        routes.insert(path, reply);
    }
    let server = TestServer::start(routes);
    let signature_url = format!("{}{}", server.base_url, release.signature_path());
    let asset_url = format!("{}{}", server.base_url, release.asset_path());
    let dest = guard.archive(&release.asset_name);
    fs::create_dir_all(&guard.dir).expect("create work dir");

    // キーなしと誤ったキーは認証失敗になる
    assert_eq!(
        download_signature(&signature_url, None),
        Err(UpdateError::DownloadUnauthorized)
    );
    assert_eq!(
        download_and_verify_with_key(
            &asset_url,
            &dest,
            release.msi.len() as u64,
            &Release::digest_of(&release.msi)["sha256:".len()..],
            Some("wrong-key"),
        ),
        Err(UpdateError::DownloadUnauthorized)
    );
    assert!(!dest.exists());
    // 正しいキーなら取得でき、署名の内容とアセットの検証を通る
    assert!(download_signature(&signature_url, Some("secret-key")).is_ok());
    assert_eq!(
        download_and_verify_with_key(
            &asset_url,
            &dest,
            release.msi.len() as u64,
            &Release::digest_of(&release.msi)["sha256:".len()..],
            Some("secret-key"),
        ),
        Ok(())
    );
    assert_eq!(fs::read(&dest).expect("read archive"), release.msi);
    // 送ったヘッダーはキー指定の有無どおりになる
    assert_eq!(
        server.requested_authorizations(),
        [
            None,
            Some("Bearer wrong-key".to_string()),
            Some(bearer.to_string()),
            Some(bearer.to_string()),
        ]
    );
}
