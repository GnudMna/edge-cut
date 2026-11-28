//! 更新処理で使う HTTP クライアント設定
//!
//! ネットワークが詰まっても確認中・ダウンロード中のまま戻らないよう、全リクエストにタイムアウトを設定する
//! HTTPS の URL から始めた通信は、リダイレクト先も HTTPS に限る

use std::io;
use std::time::Duration;

use ureq::Agent;

// ======================================================================
// 定数
// ======================================================================
/// 接続確立 (DNS 解決・TCP 接続・TLS ハンドシェイク) のタイムアウト
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// 小さな応答 (最新リリース情報・署名ファイル) の全体タイムアウト
const SMALL_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// 更新ファイル取得で応答ヘッダーを受け取るまでのタイムアウト
const DOWNLOAD_RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
/// 更新ファイル本体の受信タイムアウトの下限 (受信開始からの合計時間)
///
/// 小さなファイルでも回線の一時的な詰まりで失敗しないよう、これより短くしない
const DOWNLOAD_BODY_TIMEOUT_MIN: Duration = Duration::from_mins(10);
/// 本体受信タイムアウトを求めるときに想定する最低の転送速度 (バイト/秒)
///
/// ureq には無通信時間のタイムアウトがなく、本体受信は合計時間でしか制限できないため、
/// 宣言サイズをこの速度で受信しきれる時間を上限とし、低速回線でも途中で打ち切らない
const DOWNLOAD_MIN_BYTES_PER_SEC: u64 = 16 * 1024;

// ======================================================================
// パブリック関数
// ======================================================================
/// 最新リリース情報・署名ファイルなど小さな応答の取得用 `Agent` を作る
///
/// # Arguments
/// * `url` - 要求する URL (HTTPS ならリダイレクト先も HTTPS に限る)
///
/// # Returns
/// * `Agent` - 接続と全体にタイムアウトを設定した `Agent`
pub(crate) fn small_request_agent(url: &str) -> Agent {
    Agent::config_builder()
        .https_only(is_https(url))
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_global(Some(SMALL_REQUEST_TIMEOUT))
        .build()
        .new_agent()
}

/// 更新ファイル (MSI) のダウンロード用 `Agent` を作る
///
/// 本体受信のタイムアウトは宣言サイズから `download_body_timeout` で求める
///
/// # Arguments
/// * `url` - 要求する URL (HTTPS ならリダイレクト先も HTTPS に限る)
/// * `expected_size` - 受信する本体の宣言サイズ (バイト)
///
/// # Returns
/// * `Agent` - 接続・応答ヘッダー・本体受信にタイムアウトを設定した `Agent`
pub(crate) fn download_agent(url: &str, expected_size: u64) -> Agent {
    Agent::config_builder()
        .https_only(is_https(url))
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_recv_response(Some(DOWNLOAD_RESPONSE_TIMEOUT))
        .timeout_recv_body(Some(download_body_timeout(expected_size)))
        .build()
        .new_agent()
}

/// 宣言サイズの本体を受信しきるまでに待つ時間を求める
///
/// `DOWNLOAD_MIN_BYTES_PER_SEC` で受信した場合の所要時間とし、`DOWNLOAD_BODY_TIMEOUT_MIN` を下限とする
///
/// # Arguments
/// * `expected_size` - 受信する本体の宣言サイズ (バイト)
///
/// # Returns
/// * `Duration` - 本体受信のタイムアウト
fn download_body_timeout(expected_size: u64) -> Duration {
    Duration::from_secs(expected_size.div_ceil(DOWNLOAD_MIN_BYTES_PER_SEC))
        .max(DOWNLOAD_BODY_TIMEOUT_MIN)
}

/// HTTPS の URL か判定する
///
/// 統合テストのローカル HTTP サーバーだけが HTTPS 以外になる
fn is_https(url: &str) -> bool {
    url.starts_with("https://")
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

    /// HTTPS の URL 用の `Agent` が http への要求を拒否すること
    ///
    /// リダイレクト先にも同じ検査がかかるため、HTTP への格下げを防げる
    #[test]
    fn agents_for_https_url_reject_http() {
        let target = "http://127.0.0.1:9/";
        let err = small_request_agent("https://api.github.com/")
            .get(target)
            .call()
            .unwrap_err();
        assert!(matches!(err, ureq::Error::RequireHttpsOnly(_)));
        let err = download_agent("https://github.com/", 1)
            .get(target)
            .call()
            .unwrap_err();
        assert!(matches!(err, ureq::Error::RequireHttpsOnly(_)));
    }

    /// 本体受信のタイムアウトが宣言サイズに比例し、下限を下回らないこと
    #[test]
    fn download_body_timeout_scales_with_size() {
        // 小さなファイルは下限の 10 分
        assert_eq!(download_body_timeout(0), DOWNLOAD_BODY_TIMEOUT_MIN);
        assert_eq!(
            download_body_timeout(4 * 1024 * 1024),
            DOWNLOAD_BODY_TIMEOUT_MIN
        );
        // 下限を超える大きさでは最低速度で受信しきれる時間になる (端数は切り上げ)
        let size = DOWNLOAD_MIN_BYTES_PER_SEC * 3600 + 1;
        assert_eq!(download_body_timeout(size), Duration::from_secs(3601));
        // 上限サイズでも桁あふれしない
        let max = super::super::verify::MAX_UPDATE_DOWNLOAD_BYTES;
        assert_eq!(
            download_body_timeout(max),
            Duration::from_secs(max / DOWNLOAD_MIN_BYTES_PER_SEC)
        );
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
