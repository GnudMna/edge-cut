//! 更新フローの結合テスト用の HTTP サーバー
//!
//! パスごとに用意した応答を返し、受け取ったリクエストのパスを記録する

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

// ======================================================================
// 定数
// ======================================================================
/// 接続ごとのリクエスト読み取りタイムアウト
const SERVER_READ_TIMEOUT: Duration = Duration::from_secs(5);
/// リクエストヘッダーとして読み込む最大バイト数
const MAX_REQUEST_HEAD_BYTES: usize = 16 * 1024;

// ======================================================================
// テスト用 HTTP サーバー
// ======================================================================
/// サーバーが受け取ったリクエストの記録
#[derive(Debug, Clone)]
pub(super) struct RecordedRequest {
    /// リクエストパス
    path: String,
}

/// パスごとに返す応答
#[derive(Debug, Clone)]
pub(super) struct Reply {
    /// HTTP ステータスコード
    status: u16,
    /// 応答本体
    body: Vec<u8>,
    /// `Content-Length` に宣言する値 (`None` なら本体の長さ)
    ///
    /// 本体より大きい値を宣言すると、途中で接続が切れた応答を再現できる
    pub(super) declared_len: Option<usize>,
}

impl Reply {
    /// 本体付きの `200` 応答を作る
    pub(super) fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            body: body.into(),
            declared_len: None,
        }
    }

    /// 本体なしのエラー応答を作る
    pub(super) fn status(status: u16) -> Self {
        Self {
            status,
            ..Self::ok(Vec::new())
        }
    }
}

/// `127.0.0.1` の空きポートで待ち受ける最小の HTTP/1.1 サーバー
///
/// 接続ごとに 1 リクエストだけ処理して閉じる。破棄時にスレッドを止める
pub(super) struct TestServer {
    pub(super) base_url: String,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    stop: Arc<AtomicBool>,
    addr: std::net::SocketAddr,
    handle: Option<JoinHandle<()>>,
}

impl TestServer {
    /// 指定した応答表でサーバーを起動する
    pub(super) fn start(routes: HashMap<String, Reply>) -> Self {
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
    pub(super) fn requested_paths(&self) -> Vec<String> {
        self.requests
            .lock()
            .expect("lock")
            .iter()
            .map(|request| request.path.clone())
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

    let path = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or_default()
        .to_string();
    requests
        .lock()
        .expect("lock")
        .push(RecordedRequest { path: path.clone() });

    let reply = routes
        .get(&path)
        .cloned()
        .unwrap_or_else(|| Reply::status(404));

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
