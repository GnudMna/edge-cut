//! 更新アセットのダウンロード・検証・展開
//!
//! 一時ディレクトリ配下の版ごとの作業フォルダへアセットと署名を取得し、署名・サイズ・SHA-256 を検証する。
//! ポータブル ZIP は展開して配布物を確かめる。インストール先には触れないため、ここで失敗・中止しても
//! 動作中の `EdgeCut` に影響しない。現在の版以外の作業フォルダは本体の起動時に削除する。

use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use super::error::UpdateError;
use super::extract::extract_portable_zip;
use super::signature::{
    download_signature, embedded_signing_keys, signature_asset_name, verify_file_signature,
};
use super::types::{AvailableUpdate, PreparedPackage, UpdatePackageKind};
use super::verify::{download_and_verify, validate_asset_name};
use super::{GITHUB_BASE_URL, RELEASE_OWNER, RELEASE_REPO};
use crate::install_layout::{APP_EXE_NAME, UPDATER_EXE_NAME};

// ======================================================================
// 定数
// ======================================================================
/// 更新作業フォルダ名 (`%TEMP%` 直下)
const UPDATE_WORK_DIR_NAME: &str = "edge-cut-update";
/// 更新プログラムのコピー先フォルダ名の接頭辞 (更新作業フォルダの直下)
pub(crate) const UPDATER_SESSION_PREFIX: &str = "updater-";

// ======================================================================
// パブリック関数
// ======================================================================
/// 更新アセットをダウンロードし、検証と展開まで済ませる
///
/// 署名・サイズ・SHA-256 のすべての検証に通ったものだけを返す
///
/// # Arguments
/// * `update` - 確認済みの更新アセット情報
/// * `on_progress` - 受信済みバイト数の通知先。`false` を返すと中止する
///
/// # Returns
/// * `Result<PreparedPackage, UpdateError>` - インストールできる状態の配布物
pub(crate) fn prepare_update(
    update: &AvailableUpdate,
    on_progress: &mut dyn FnMut(u64) -> bool,
) -> Result<PreparedPackage, UpdateError> {
    prepare_update_at(
        update,
        GITHUB_BASE_URL,
        &update_work_root(),
        embedded_signing_keys(),
        on_progress,
    )
}

/// 指定したベース URL から更新アセットをダウンロードし、検証と展開まで済ませる
///
/// # Arguments
/// * `update` - 確認済みの更新アセット情報
/// * `base_url` - GitHub のベース URL (末尾スラッシュなし)
/// * `work_root` - バージョンごとの作業フォルダを置く親ディレクトリ
/// * `trusted_keys` - 署名検証に使う minisign 公開鍵 (Base64)
/// * `on_progress` - 受信済みバイト数の通知先。`false` を返すと中止する
///
/// # Returns
/// * `Result<PreparedPackage, UpdateError>` - インストールできる状態の配布物
pub(crate) fn prepare_update_at(
    update: &AvailableUpdate,
    base_url: &str,
    work_root: &Path,
    trusted_keys: &[&str],
    on_progress: &mut dyn FnMut(u64) -> bool,
) -> Result<PreparedPackage, UpdateError> {
    // 確認時にも検証済みだが、保存先パスと URL に使う直前でも再確認する
    validate_asset_name(&update.asset_name)?;
    let work_dir = update_work_dir(work_root, &update.latest_version)?;

    let archive_path = work_dir.join(&update.asset_name);
    download_verified_asset(update, base_url, &archive_path, trusted_keys, on_progress)?;

    open_verified_asset(update, &archive_path, &work_dir)
}

/// 過去の更新で残った作業フォルダのうち、現在のバージョン以外を削除する
///
/// 本体の起動時に呼ぶ。削除に失敗したフォルダ (使用中など) は次回起動時に再試行する
pub(crate) fn cleanup_stale_update_dirs() {
    remove_stale_work_dirs(&update_work_root(), super::current_app_version());
}

/// 更新に使った作業フォルダ (ダウンロード・展開先) を削除する
///
/// 削除できなければ残し、次回の本体の起動時に掃除する
///
/// # Arguments
/// * `version` - 更新先のバージョン
pub(crate) fn remove_work_dir(version: &str) {
    let dir = update_work_root().join(version);
    if let Err(err) = fs::remove_dir_all(&dir)
        && err.kind() != io::ErrorKind::NotFound
    {
        eprintln!("更新作業フォルダの削除に失敗: {err}");
    }
}

/// 更新作業フォルダの親 (`%TEMP%\edge-cut-update`) を返す
///
/// # Returns
/// * `PathBuf` - バージョンごとのフォルダと更新プログラムのコピー先を置く場所
pub(crate) fn update_work_root() -> PathBuf {
    std::env::temp_dir().join(UPDATE_WORK_DIR_NAME)
}

/// 実行ファイルが起動中で書き込めないか
///
/// 起動中のプログラムのファイルは書き込み用に開けないことを使い、プロセスを調べずに判定する
///
/// # Arguments
/// * `path` - 実行ファイル
///
/// # Returns
/// * `bool` - ファイルがあり、書き込み用に開けなければ `true`
pub(crate) fn is_exe_in_use(path: &Path) -> bool {
    path.is_file() && OpenOptions::new().write(true).open(path).is_err()
}

// ======================================================================
// ダウンロードと検証
// ======================================================================
/// アセットと署名をダウンロードし、サイズ・SHA-256・署名を検証する
///
/// 署名は小さいため先に取得し、欠落時はアセット本体をダウンロードしない
/// 署名検証に失敗した場合は保存済みアセットを削除する
///
/// # Arguments
/// * `update` - 確認済みの更新アセット情報
/// * `base_url` - GitHub のベース URL (末尾スラッシュなし)
/// * `archive_path` - アセットの保存先
/// * `trusted_keys` - 署名検証に使う minisign 公開鍵 (Base64)
/// * `on_progress` - 受信済みバイト数の通知先
///
/// # Returns
/// * `Result<(), UpdateError>` - すべての検証に成功した場合は `Ok(())`
fn download_verified_asset(
    update: &AvailableUpdate,
    base_url: &str,
    archive_path: &Path,
    trusted_keys: &[&str],
    on_progress: &mut dyn FnMut(u64) -> bool,
) -> Result<(), UpdateError> {
    if trusted_keys.is_empty() {
        return Err(UpdateError::SignatureKeyMissing);
    }

    let signature_url = asset_download_url(
        base_url,
        &update.tag,
        &signature_asset_name(&update.asset_name),
    );
    let signature = download_signature(&signature_url)?;

    let download_url = asset_download_url(base_url, &update.tag, &update.asset_name);
    download_and_verify(
        &download_url,
        archive_path,
        update.size,
        &update.sha256,
        on_progress,
    )?;

    let verified = verify_file_signature(
        archive_path,
        &signature,
        &update.latest_version,
        &update.asset_name,
        trusted_keys,
    );
    if verified.is_err() {
        let _ = fs::remove_file(archive_path);
    }
    verified
}

/// 検証済みアセットを種類に応じてインストールできる状態にする
///
/// MSI はそのまま使い、ポータブル ZIP は展開して本体・更新プログラム・マーカーが揃うか確かめる
fn open_verified_asset(
    update: &AvailableUpdate,
    archive_path: &Path,
    work_dir: &Path,
) -> Result<PreparedPackage, UpdateError> {
    if UpdatePackageKind::Msi.matches_asset_name(&update.asset_name) {
        return Ok(PreparedPackage::Msi {
            path: archive_path.to_path_buf(),
        });
    }
    if UpdatePackageKind::PortableZip.matches_asset_name(&update.asset_name) {
        let source_dir = extract_portable_zip(archive_path, &work_dir.join("extracted"))?;
        let complete = [APP_EXE_NAME, UPDATER_EXE_NAME]
            .iter()
            .all(|name| source_dir.join(name).is_file())
            && crate::install_layout::is_portable_dir(&source_dir);
        if !complete {
            return Err(UpdateError::PortableInvalid);
        }
        return Ok(PreparedPackage::Portable { source_dir });
    }
    Err(UpdateError::AssetNameInvalid)
}

/// アセットダウンロード URL を組み立てる
fn asset_download_url(base_url: &str, tag: &str, asset_name: &str) -> String {
    format!("{base_url}/{RELEASE_OWNER}/{RELEASE_REPO}/releases/download/{tag}/{asset_name}")
}

// ======================================================================
// 作業フォルダ
// ======================================================================
/// 更新作業用ディレクトリを用意する
fn update_work_dir(root: &Path, version: &str) -> Result<PathBuf, UpdateError> {
    let base = root.join(version);
    fs::create_dir_all(&base).map_err(|_| UpdateError::WorkDirCreate)?;
    Ok(base)
}

/// `root` 直下のフォルダのうち、`keep_version` と起動中の更新プログラムのコピー先以外を削除する
///
/// フォルダ以外とシンボリックリンクは対象外。個々の削除失敗は無視する
///
/// # Arguments
/// * `root` - バージョンごとのフォルダを置く親ディレクトリ
/// * `keep_version` - 残すバージョンのフォルダ名
fn remove_stale_work_dirs(root: &Path, keep_version: &str) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        // DirEntry::file_type はリンクをたどらないため、リンク先は消さない
        let is_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
        if !is_dir || entry.file_name() == keep_version {
            continue;
        }
        // 更新プログラムが動いている間は、画面 (`WebView2`) のデータを消さない
        let session = entry
            .file_name()
            .to_string_lossy()
            .starts_with(UPDATER_SESSION_PREFIX);
        if session && is_exe_in_use(&entry.path().join(UPDATER_EXE_NAME)) {
            continue;
        }
        let _ = fs::remove_dir_all(entry.path());
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::Write;

    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    use super::*;
    use crate::install_layout::PORTABLE_MARKER_FILE;

    /// アセットダウンロード URL が GitHub Releases のパスに従うようにすること
    #[test]
    fn asset_download_url_matches_github_releases() {
        let url = asset_download_url(GITHUB_BASE_URL, "v0.1.1", "edge-cut-0.1.1-x86_64.msi");
        assert_eq!(
            url,
            "https://github.com/GnudMna/edge-cut/releases/download/v0.1.1/edge-cut-0.1.1-x86_64.msi"
        );
    }

    /// テスト用の更新アセット情報を返す
    fn sample_update(asset_name: &str) -> AvailableUpdate {
        AvailableUpdate {
            current_version: "0.1.2".to_string(),
            latest_version: "0.1.3".to_string(),
            tag: "v0.1.3".to_string(),
            asset_name: asset_name.to_string(),
            size: 1,
            sha256: "a".repeat(64),
        }
    }

    /// 指定したファイルを `root/` 配下に持つ ZIP を作る
    fn write_zip(path: &Path, root: &str, files: &[&str]) {
        let file = File::create(path).expect("zip create");
        let mut writer = ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for name in files {
            writer
                .start_file(format!("{root}/{name}"), options)
                .expect("start file");
            writer.write_all(name.as_bytes()).expect("write");
        }
        writer.finish().expect("finish zip");
    }

    /// 現在バージョン以外のフォルダだけを削除すること
    ///
    /// 現在バージョンのフォルダとフォルダ以外のファイルは残す
    #[test]
    fn remove_stale_work_dirs_keeps_only_current_version() {
        let root = tempfile::tempdir().expect("tempdir");
        for version in ["0.4.0", "0.3.0", "0.2.0", "updater-1-2"] {
            let dir = root.path().join(version);
            fs::create_dir_all(&dir).expect("create dir");
            fs::write(dir.join("edge-cut.msi"), b"msi").expect("write msi");
        }
        fs::write(root.path().join("note.txt"), b"x").expect("write file");

        remove_stale_work_dirs(root.path(), "0.4.0");

        // 現在バージョンのフォルダと中身は残る
        assert!(root.path().join("0.4.0").join("edge-cut.msi").is_file());
        // それ以外のバージョンと、終了した更新プログラムのフォルダは消える
        assert!(!root.path().join("0.3.0").exists());
        assert!(!root.path().join("0.2.0").exists());
        assert!(!root.path().join("updater-1-2").exists());
        // フォルダ以外は対象外
        assert!(root.path().join("note.txt").is_file());
    }

    /// 親ディレクトリが存在しなくても何もせず終わること
    #[test]
    fn remove_stale_work_dirs_ignores_missing_root() {
        let root = tempfile::tempdir().expect("tempdir");
        remove_stale_work_dirs(&root.path().join("missing"), "0.4.0");
    }

    /// 起動中の実行ファイルは使用中、ただのファイルは使用中でないと判定すること
    #[test]
    fn is_exe_in_use_detects_running_exe() {
        let exe = std::env::current_exe().expect("current exe");
        assert!(is_exe_in_use(&exe));

        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("idle.exe");
        fs::write(&file, b"x").expect("write");
        assert!(!is_exe_in_use(&file));
        assert!(!is_exe_in_use(&tmp.path().join("missing.exe")));
    }

    /// 不正なアセット名ではダウンロードせずに失敗すること
    #[test]
    fn prepare_update_rejects_path_like_asset_name() {
        let update = sample_update("../edge-cut-0.1.3-x86_64.msi");

        // 接続できない URL でも検証で先に失敗するためネットワークへは出ない
        let result = prepare_update_at(
            &update,
            "http://127.0.0.1:9",
            Path::new("unused-work-root"),
            &["unused"],
            &mut |_| true,
        );
        assert_eq!(result, Err(UpdateError::AssetNameInvalid));
    }

    /// 公開鍵が埋め込まれていないときはダウンロードせずに失敗すること
    #[test]
    fn download_verified_asset_requires_trusted_key() {
        let update = sample_update("edge-cut-0.1.3-x86_64.msi");
        let archive_path =
            std::env::temp_dir().join(format!("edge-cut-apply-no-key-{}.msi", std::process::id()));

        // 接続できない URL でも鍵の確認で先に失敗するためネットワークへは出ない
        let result = download_verified_asset(
            &update,
            "http://127.0.0.1:9",
            &archive_path,
            &[],
            &mut |_| true,
        );
        assert_eq!(result, Err(UpdateError::SignatureKeyMissing));
        assert!(!archive_path.exists());
    }

    /// MSI は展開せず、ダウンロードしたファイルをそのまま返すこと
    #[test]
    fn open_verified_msi_returns_archive_path() {
        let update = sample_update("edge-cut-0.1.3-x86_64.msi");
        let path = PathBuf::from("unused.msi");

        let package = open_verified_asset(&update, &path, Path::new("unused-work"));
        assert_eq!(package, Ok(PreparedPackage::Msi { path }));
    }

    /// ポータブル ZIP を展開し、配布物のルートを返すこと
    #[test]
    fn open_verified_portable_extracts_payload() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let zip_path = tmp.path().join("edge-cut-0.1.3-x86_64-portable.zip");
        write_zip(
            &zip_path,
            "edge-cut-0.1.3-x86_64-portable",
            &[APP_EXE_NAME, UPDATER_EXE_NAME, PORTABLE_MARKER_FILE],
        );

        let update = sample_update("edge-cut-0.1.3-x86_64-portable.zip");
        match open_verified_asset(&update, &zip_path, tmp.path()) {
            Ok(PreparedPackage::Portable { source_dir }) => {
                assert!(source_dir.join(APP_EXE_NAME).is_file());
                assert!(source_dir.ends_with("edge-cut-0.1.3-x86_64-portable"));
            }
            other => panic!("ポータブル版を期待したが {other:?}"),
        }
    }

    /// 本体やマーカーが欠けたポータブル ZIP は拒否すること
    #[test]
    fn open_verified_portable_requires_complete_payload() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let zip_path = tmp.path().join("edge-cut-0.1.3-x86_64-portable.zip");
        write_zip(&zip_path, "pkg", &[APP_EXE_NAME, UPDATER_EXE_NAME]);

        let update = sample_update("edge-cut-0.1.3-x86_64-portable.zip");
        let result = open_verified_asset(&update, &zip_path, tmp.path());
        assert_eq!(result, Err(UpdateError::PortableInvalid));
    }
}
