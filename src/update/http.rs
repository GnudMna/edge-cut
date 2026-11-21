//! 更新処理で使う HTTP クライアント設定
//!
//! ネットワークが詰まっても確認中・ダウンロード中のまま戻らないよう、全リクエストにタイムアウトを設定する

use std::io;
use std::time::Duration;

use ureq::Agent;

// ======================================================================
// 定数
// ======================================================================
/// 接続確立 (DNS 解決・TCP 接続・TLS ハンドシェイク) のタイムアウト
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// 小さな応答 (version API・署名ファイル) の全体タイムアウト
const SMALL_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// 更新ファイル取得で応答ヘッダーを受け取るまでのタイムアウト
const DOWNLOAD_RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
/// 更新ファイル本体の受信タイムアウト (受信開始からの合計時間)
///
/// 低速回線でも MSI を受信しきれる余裕を持たせつつ、無期限には待たない
const DOWNLOAD_BODY_TIMEOUT: Duration = Duration::from_mins(10);

// ======================================================================
// パブリック関数
// ======================================================================
/// version API・署名ファイルなど小さな応答の取得用 `Agent` を作る
///
/// # Returns
/// * `Agent` - 接続と全体にタイムアウトを設定した `Agent`
pub(crate) fn small_request_agent() -> Agent {
    Agent::config_builder()
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_global(Some(SMALL_REQUEST_TIMEOUT))
        .build()
        .new_agent()
}

/// 更新ファイル (MSI) のダウンロード用 `Agent` を作る
///
/// # Returns
/// * `Agent` - 接続・応答ヘッダー・本体受信にタイムアウトを設定した `Agent`
pub(crate) fn download_agent() -> Agent {
    Agent::config_builder()
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_recv_response(Some(DOWNLOAD_RESPONSE_TIMEOUT))
        .timeout_recv_body(Some(DOWNLOAD_BODY_TIMEOUT))
        .build()
        .new_agent()
}

/// ureq のエラーがタイムアウトか判定する
pub(crate) fn is_timeout(err: &ureq::Error) -> bool {
    match err {
        ureq::Error::Timeout(_) => true,
        ureq::Error::Io(io_err) => io_err.kind() == io::ErrorKind::TimedOut,
        _ => false,
    }
}

/// 本体読み取り時の `io::Error` がタイムアウトか判定する
///
/// ureq は本体読み取り中のタイムアウトを `io::Error` に包んで返すため、包みを解いて判定する
pub(crate) fn is_timeout_io(err: io::Error) -> bool {
    is_timeout(&ureq::Error::from(err))
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// ureq のタイムアウトエラーをタイムアウトと判定すること
    #[test]
    fn is_timeout_detects_ureq_timeout() {
        assert!(is_timeout(&ureq::Error::Timeout(ureq::Timeout::RecvBody)));
        // 本体読み取りで `io::Error` に包まれた場合も判定できる
        assert!(is_timeout_io(
            ureq::Error::Timeout(ureq::Timeout::RecvBody).into_io()
        ));
    }

    /// OS レベルの `TimedOut` をタイムアウトと判定すること
    #[test]
    fn is_timeout_detects_io_timed_out() {
        assert!(is_timeout_io(io::Error::from(io::ErrorKind::TimedOut)));
    }

    /// タイムアウト以外のエラーはタイムアウトと判定しないこと
    #[test]
    fn is_timeout_rejects_other_errors() {
        assert!(!is_timeout(&ureq::Error::StatusCode(500)));
        assert!(!is_timeout_io(io::Error::from(
            io::ErrorKind::ConnectionReset
        )));
    }
}
