//! gh-release-bridge 経由のアプリ更新確認・適用
//!
//! version API の各アセットに GitHub Release Asset の `digest` (`sha256:<hex>`) が
//! 含まれることを前提とする。ダウンロード中は宣言サイズ超過または絶対上限で打ち切り、
//! 完了後にサイズと SHA-256 を検証する
//! 適用前に `<asset>.minisig` の minisign 署名をビルド時埋め込み公開鍵で検証し、
//! 配信経路や version API の digest を差し替えられても正規の秘密鍵で署名されていない MSI は適用しない
//! 保護対象タグのアセット DL にはビルド時埋め込みキー (`EDGE_CUT_KEY`) を
//! `Authorization: Bearer` で付与する

mod apply;
mod check;
mod error;
mod signature;
mod types;
mod verify;

pub(crate) use apply::apply_update;
pub(crate) use check::check_for_update;
pub(crate) use types::{AvailableUpdate, UpdateApplyOutcome, UpdateCheckOutcome};

// ======================================================================
// 定数
// ======================================================================
/// gh-release-bridge のベース URL
pub(crate) const BRIDGE_BASE_URL: &str = "https://gh-release-bridge.gnudlemna-4656.workers.dev";
/// GitHub リポジトリ所有者
pub(crate) const RELEASE_OWNER: &str = "GnudMna";
/// GitHub リポジトリ名
pub(crate) const RELEASE_REPO: &str = "edge-cut";

/// ビルド時に埋め込まれたアプリバージョンを返す
///
/// # Returns
/// * `&'static str` - Cargo パッケージバージョン
pub(crate) fn current_app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// ビルド時に埋め込まれた bridge ダウンロードキーを返す
///
/// 環境変数 `EDGE_CUT_KEY` がビルド時に設定されていればその値
/// 未設定または空文字のときは `None` (無認証 GET)
///
/// # Returns
/// * `Option<&'static str>` - 埋め込みキー
pub(crate) fn bridge_download_key() -> Option<&'static str> {
    match option_env!("EDGE_CUT_KEY") {
        Some(key) if !key.is_empty() => Some(key),
        _ => None,
    }
}
