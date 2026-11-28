//! GitHub Releases によるアプリ更新確認・適用
//!
//! 本体は更新の有無を確かめ (`check`)、更新プログラム (`EdgeCutUpdater.exe`) を起動する (`session`)。
//! 更新プログラムはダウンロード・検証 (`prepare`)、本体の終了待ち、無人インストール (`install`)、
//! 再起動までを進める (`flow` / `ops`)。画面は `app::updater` が担う
//!
//! GitHub API が返す各アセットに `digest` (`sha256:<hex>`) が含まれることを前提とする。
//! ダウンロード中は宣言サイズ超過または絶対上限で打ち切り、完了後にサイズと SHA-256 を検証する
//! 適用前に `<asset>.minisig` の minisign 署名をビルド時埋め込み公開鍵で検証し、
//! 配信経路や API の digest を差し替えられても正規の秘密鍵で署名されていない配布物は適用しない

mod check;
mod error;
mod extract;
pub(crate) mod flow;
mod http;
mod install;
mod ops;
mod prepare;
mod session;
mod signature;
mod types;
mod verify;

pub(crate) use check::check_for_update;
pub(crate) use error::UpdateError;
pub(crate) use ops::{SystemOps, UpdaterArgs};
pub(crate) use prepare::cleanup_stale_update_dirs;
pub(crate) use session::{UpdaterEvent, UpdaterSession, launch_updater};
pub(crate) use types::{AvailableUpdate, UpdateCheckOutcome, UpdatePackageKind};

// ======================================================================
// 定数
// ======================================================================
/// GitHub REST API のベース URL (最新リリースの照会)
pub(crate) const GITHUB_API_BASE_URL: &str = "https://api.github.com";
/// GitHub のベース URL (リリースアセットのダウンロード)
pub(crate) const GITHUB_BASE_URL: &str = "https://github.com";
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
