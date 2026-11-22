//! gh-release-bridge 経由のアプリ更新確認・適用
//!
//! version API の各アセットに GitHub Release Asset の `digest` (`sha256:<hex>`) が
//! 含まれることを前提とする。ダウンロード中は宣言サイズ超過または絶対上限で打ち切り、
//! 完了後にサイズと SHA-256 を検証する
//! 適用前に `<asset>.minisig` の minisign 署名をビルド時埋め込み公開鍵で検証し、
//! 配信経路や version API の digest を差し替えられても正規の秘密鍵で署名されていない MSI は適用しない
//! 保護対象タグのアセット DL にはビルド時埋め込みキー (`EDGE_CUT_KEY_FILE` が指すファイル) を
//! ChaCha20-Poly1305 で暗号化して埋め込み、復号した値を `Authorization: Bearer` で付与する

mod apply;
mod bridge_key;
mod bridge_key_obfuscate;
mod check;
mod cipher;
mod error;
mod http;
mod signature;
mod types;
mod verify;

pub(crate) use apply::{apply_update, cleanup_stale_update_dirs};
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

#[cfg(test)]
mod tests;
