use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};

use super::error::UpdateError;
use super::http;

// ======================================================================
// 定数
// ======================================================================
/// SHA-256 ダイジェストの16進文字列長
const SHA256_HEX_LEN: usize = 64;
/// 更新アセットダウンロードの絶対サイズ上限 (512 MiB)
///
/// MSI の想定サイズを大きく上回る応答でディスクを圧迫しないための上限
pub(crate) const MAX_UPDATE_DOWNLOAD_BYTES: u64 = 512 * 1024 * 1024;
/// アセット名の最大バイト数
const MAX_ASSET_NAME_LEN: usize = 200;
/// Windows の予約デバイス名 (拡張子付きでもデバイスとして解釈されうる)
const RESERVED_DEVICE_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

// ======================================================================
// パブリック関数
// ======================================================================
/// アセットの完全性文字列を正規化した SHA-256 16進へ変換する
///
/// GitHub Release Asset の `digest` (`sha256:<hex>`) と素の16進の両方を受け付ける
///
/// # Arguments
/// * `raw` - `digest` / `sha256` フィールドの生文字列
///
/// # Returns
/// * `Result<String, UpdateError>` - 小文字16進 64 桁、またはエラー
pub(crate) fn parse_asset_digest(raw: &str) -> Result<String, UpdateError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(UpdateError::DigestEmpty);
    }

    let hex = trimmed
        .strip_prefix("sha256:")
        .or_else(|| trimmed.strip_prefix("SHA256:"))
        .unwrap_or(trimmed)
        .trim();

    if hex.len() != SHA256_HEX_LEN || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(UpdateError::DigestInvalid {
            raw: trimmed.to_string(),
        });
    }

    Ok(hex.to_ascii_lowercase())
}

/// アセット名が単一のファイル名として安全に扱えるか検証する
///
/// アセット名は作業ディレクトリ配下の保存先パスと bridge のダウンロード URL に
/// そのまま使うため、ASCII 英数字と `.` `-` `_` だけからなる名前に限る
/// パス区切り・ドライブ指定・`..`・URL で意味を持つ文字・Windows の予約デバイス名を拒否する
///
/// # Arguments
/// * `name` - version API が報告するアセット名
///
/// # Returns
/// * `Result<(), UpdateError>` - 安全なファイル名なら `Ok(())`
pub(crate) fn validate_asset_name(name: &str) -> Result<(), UpdateError> {
    let valid_chars = name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'));
    // 先頭 `.` は `.` / `..` と隠しファイル、末尾 `.` は Windows の名前正規化を避ける
    let valid = !name.is_empty()
        && name.len() <= MAX_ASSET_NAME_LEN
        && valid_chars
        && !name.starts_with('.')
        && !name.ends_with('.')
        && !is_reserved_device_name(name);
    if !valid {
        return Err(UpdateError::AssetNameInvalid);
    }
    Ok(())
}

/// アセットの宣言サイズがダウンロード可能な範囲か検証する
///
/// # Arguments
/// * `expected_size` - version API が報告するバイト数
///
/// # Returns
/// * `Result<(), UpdateError>` - 許容範囲なら `Ok(())`
pub(crate) fn validate_declared_download_size(expected_size: u64) -> Result<(), UpdateError> {
    if expected_size == 0 {
        return Err(UpdateError::SizeZero);
    }
    if expected_size > MAX_UPDATE_DOWNLOAD_BYTES {
        return Err(UpdateError::SizeExceedsMax {
            max: MAX_UPDATE_DOWNLOAD_BYTES,
        });
    }
    Ok(())
}

/// 指定キーで URL からファイルへダウンロードし、サイズと SHA-256 を検証する
///
/// 期待サイズ超過または絶対上限超過の時点で読み取りを打ち切る
/// 検証に失敗した場合は保存済みファイルを削除する
///
/// # Arguments
/// * `url` - ダウンロード URL
/// * `dest` - 保存先パス
/// * `expected_size` - 期待バイト数 (0 は拒否)
/// * `expected_sha256` - 期待する SHA-256 (小文字16進 64 桁)
/// * `download_key` - bridge ダウンロードキー (`None` なら無認証)
///
/// # Returns
/// * `Result<(), UpdateError>` - 成功時 `Ok(())`
pub(crate) fn download_and_verify_with_key(
    url: &str,
    dest: &Path,
    expected_size: u64,
    expected_sha256: &str,
    download_key: Option<&str>,
) -> Result<(), UpdateError> {
    let result = download_and_verify_inner(url, dest, expected_size, expected_sha256, download_key);
    if result.is_err() {
        let _ = fs::remove_file(dest);
    }
    result
}

// ======================================================================
// プライベート関数
// ======================================================================
/// ダウンロード本体 (失敗時の削除は呼び出し側)
fn download_and_verify_inner(
    url: &str,
    dest: &Path,
    expected_size: u64,
    expected_sha256: &str,
    download_key: Option<&str>,
) -> Result<(), UpdateError> {
    validate_declared_download_size(expected_size)?;

    let mut request = http::download_agent().get(url);
    if let Some(key) = download_key {
        request = request.header("Authorization", &format!("Bearer {key}"));
    }
    let mut response = request.call().map_err(|err| match err {
        ureq::Error::StatusCode(401) => UpdateError::DownloadUnauthorized,
        ureq::Error::StatusCode(status) => UpdateError::DownloadHttp { status },
        err if http::is_timeout(&err) => UpdateError::NetworkTimeout,
        _ => UpdateError::DownloadFailed,
    })?;

    let mut reader = response.body_mut().as_reader();
    let mut file = File::create(dest).map_err(|_| UpdateError::SaveFailed)?;

    let mut hasher = Sha256::new();
    let written = copy_and_hash_limited(&mut reader, &mut file, &mut hasher, expected_size)?;
    file.flush().map_err(|_| UpdateError::SaveFailed)?;
    // 以降は読み取り検証のみにする
    drop(file);

    if written != expected_size {
        return Err(UpdateError::SizeMismatch {
            expected: expected_size,
            actual: written,
        });
    }

    let actual_sha256 = to_hex(&hasher.finalize());
    if actual_sha256 != expected_sha256 {
        return Err(UpdateError::IntegrityFailed);
    }

    Ok(())
}

/// Reader から Writer へコピーしつつ SHA-256 を更新し、上限超過で打ち切る
fn copy_and_hash_limited<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    hasher: &mut Sha256,
    cap: u64,
) -> Result<u64, UpdateError> {
    let mut written = 0u64;
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buffer).map_err(|err| {
            if http::is_timeout_io(err) {
                UpdateError::NetworkTimeout
            } else {
                UpdateError::DownloadFailed
            }
        })?;
        if n == 0 {
            break;
        }

        let n_u64 = n as u64;
        let Some(next) = written.checked_add(n_u64) else {
            return Err(UpdateError::SizeCapOverflow);
        };
        if next > cap {
            return Err(UpdateError::SizeMismatchTruncated { expected: cap });
        }

        writer
            .write_all(&buffer[..n])
            .map_err(|_| UpdateError::SaveFailed)?;
        hasher.update(&buffer[..n]);
        written = next;
    }
    Ok(written)
}

/// 最初の `.` より前が Windows の予約デバイス名か判定する
fn is_reserved_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name);
    RESERVED_DEVICE_NAMES
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
}

/// バイト列を小文字16進文字列へ変換する
fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    /// 配布物の命名規約に沿ったアセット名を受け付けること
    #[test]
    fn validate_asset_name_accepts_release_names() {
        for name in [
            "edge-cut-0.1.3-x86_64.msi",
            "edge-cut-0.1.3-x86_64.msi.minisig",
            "EdgeCut_1.0.0.MSI",
        ] {
            assert!(validate_asset_name(name).is_ok(), "{name}");
        }
    }

    /// パスとして解釈されうるアセット名を拒否すること
    #[test]
    fn validate_asset_name_rejects_path_like_names() {
        let too_long = format!("{}.msi", "a".repeat(MAX_ASSET_NAME_LEN));
        for name in [
            "",
            ".",
            "..",
            "../evil.msi",
            "..\\evil.msi",
            "sub/evil.msi",
            "sub\\evil.msi",
            "C:\\Windows\\evil.msi",
            "C:evil.msi",
            "\\\\server\\share\\evil.msi",
            "evil.msi:stream",
            ".hidden.msi",
            "evil.msi.",
            "evil msi.msi",
            "evil%2F.msi",
            "evil.msi?x=1",
            "evil.msi#frag",
            "評価.msi",
            too_long.as_str(),
        ] {
            assert_eq!(
                validate_asset_name(name),
                Err(UpdateError::AssetNameInvalid),
                "{name:?}"
            );
        }
    }

    /// Windows の予約デバイス名を拡張子付きでも拒否すること
    #[test]
    fn validate_asset_name_rejects_reserved_device_names() {
        for name in ["CON", "nul.msi", "Com1.x86_64.msi"] {
            assert_eq!(
                validate_asset_name(name),
                Err(UpdateError::AssetNameInvalid),
                "{name}"
            );
        }

        // 最初の `.` より前が予約名と一致しなければ許可する
        assert!(validate_asset_name("lpt9-x86_64.msi").is_ok());
        assert!(validate_asset_name("console.msi").is_ok());
    }

    /// GitHub 形式の `sha256:` 付き digest を正規化できるようにすること
    #[test]
    fn parse_asset_digest_accepts_github_prefix() {
        let hex = "a".repeat(SHA256_HEX_LEN);
        let parsed = parse_asset_digest(&format!("sha256:{hex}")).expect("digest");
        assert_eq!(parsed, hex);
    }

    /// 素の16進と大文字プレフィックスを正規化できるようにすること
    #[test]
    fn parse_asset_digest_normalizes_hex_case() {
        let upper = "B".repeat(SHA256_HEX_LEN);
        let parsed = parse_asset_digest(&format!("SHA256:{upper}")).expect("digest");
        assert_eq!(parsed, "b".repeat(SHA256_HEX_LEN));

        let bare = parse_asset_digest(&upper).expect("bare");
        assert_eq!(bare, "b".repeat(SHA256_HEX_LEN));
    }

    /// 不正な digest は拒否すること
    #[test]
    fn parse_asset_digest_rejects_invalid() {
        assert!(parse_asset_digest("").is_err());
        assert!(parse_asset_digest("sha256:abcd").is_err());
        assert!(parse_asset_digest(&"g".repeat(SHA256_HEX_LEN)).is_err());
    }

    /// 宣言サイズ 0 と絶対上限超過を拒否すること
    #[test]
    fn validate_declared_download_size_rejects_zero_and_over_max() {
        assert!(validate_declared_download_size(0).is_err());
        assert!(validate_declared_download_size(MAX_UPDATE_DOWNLOAD_BYTES).is_ok());
        assert!(validate_declared_download_size(MAX_UPDATE_DOWNLOAD_BYTES + 1).is_err());
    }

    /// 上限ちょうどの入力はコピーできるようにすること
    #[test]
    fn copy_and_hash_limited_accepts_exact_cap() {
        let payload = b"exact-cap-payload";
        let mut reader = Cursor::new(payload.as_slice());
        let mut out = Vec::new();
        let mut hasher = Sha256::new();

        let written =
            copy_and_hash_limited(&mut reader, &mut out, &mut hasher, payload.len() as u64)
                .expect("copy");
        assert_eq!(written, payload.len() as u64);
        assert_eq!(out, payload);
        assert_eq!(to_hex(&hasher.finalize()), to_hex(&Sha256::digest(payload)));
    }

    /// 上限を超える応答は書き込み前に打ち切るようにすること
    #[test]
    fn copy_and_hash_limited_aborts_before_writing_overflow() {
        let payload = b"0123456789";
        let mut reader = Cursor::new(payload.as_slice());
        let mut out = Vec::new();
        let mut hasher = Sha256::new();

        let err = copy_and_hash_limited(&mut reader, &mut out, &mut hasher, 4).expect_err("cap");
        assert!(err.to_string().contains("サイズ"));
        assert!(out.is_empty());
    }

    /// 複数チャンクでも累積が上限を超えた時点で打ち切るようにすること
    #[test]
    fn copy_and_hash_limited_aborts_after_partial_write() {
        struct Chunked<'a> {
            data: &'a [u8],
            pos: usize,
            chunk: usize,
        }

        impl Read for Chunked<'_> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if self.pos >= self.data.len() {
                    return Ok(0);
                }
                let remain = self.data.len() - self.pos;
                let n = remain.min(self.chunk).min(buf.len());
                buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
                self.pos += n;
                Ok(n)
            }
        }

        let payload = b"abcdefgh";
        let mut reader = Chunked {
            data: payload,
            pos: 0,
            chunk: 3,
        };
        let mut out = Vec::new();
        let mut hasher = Sha256::new();

        let err = copy_and_hash_limited(&mut reader, &mut out, &mut hasher, 6).expect_err("cap");
        assert!(err.to_string().contains("サイズ"));
        assert_eq!(out, b"abcdef");
    }
}
