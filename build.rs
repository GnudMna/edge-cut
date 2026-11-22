//! Windows リソース埋め込み、gh-release-bridge キーの暗号化、更新署名の公開鍵埋め込み
//!
//! `EDGE_CUT_KEY_FILE` が指すファイルのキーを ChaCha20-Poly1305 で暗号化して `OUT_DIR` へ書き出す
//! `keys/update-signing/*.pub` の minisign 公開鍵を `OUT_DIR` へ書き出す

#[path = "src/update/bridge_key_obfuscate.rs"]
mod bridge_key_obfuscate;

use std::env;
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use zeroize::{Zeroize, Zeroizing};

use bridge_key_obfuscate::{OBFUSCATE_MASK_LEN, xor_obfuscate};

const LEGAL_COPYRIGHT: &str = "Copyright (c) 2026 Yuto Hirata";

const APP_NAME: &str = "EdgeCut";

/// exe に埋め込むアイコン
const ICON_PATH: &str = "assets/icons/icon.ico";

/// ChaCha20-Poly1305 の nonce 長
const NONCE_LEN: usize = 12;
/// bridge ダウンロードキーの最大バイト数
const MAX_BRIDGE_KEY_BYTES: usize = 1024;
/// bridge ダウンロードキーを保存したファイルのパスを渡す環境変数
const BRIDGE_KEY_FILE_ENV: &str = "EDGE_CUT_KEY_FILE";
/// 廃止した、キーを直接渡す環境変数
const LEGACY_BRIDGE_KEY_ENV: &str = "EDGE_CUT_KEY";
/// UTF-8 の BOM
const UTF8_BOM: &[u8] = &[0xef, 0xbb, 0xbf];

/// 更新署名の公開鍵 (minisign 形式 `*.pub`) を置くディレクトリ
const UPDATE_SIGNING_KEYS_DIR: &str = "keys/update-signing";

/// minisign 公開鍵の Base64 長 (署名方式 2 + 鍵 ID 8 + Ed25519 公開鍵 32 バイト)
const MINISIGN_PUBLIC_KEY_B64_LEN: usize = 56;

fn main() {
    // 更新ダウンロードキー (gh-release-bridge) の埋め込み対象
    println!("cargo::rerun-if-changed=src/update/bridge_key_obfuscate.rs");
    println!("cargo::rerun-if-env-changed={BRIDGE_KEY_FILE_ENV}");
    println!("cargo::rerun-if-changed={UPDATE_SIGNING_KEYS_DIR}");

    warn_if_crt_not_static();
    embed_bridge_key();
    embed_update_signing_keys();

    // exe にリソース情報を埋め込む
    // アイコンパスを再実行条件に追加
    println!("cargo::rerun-if-changed={ICON_PATH}");

    // Windows リソースを設定
    let mut res = winresource::WindowsResource::new();

    // アイコン
    res.set_icon(ICON_PATH);

    // アプリ情報
    res.set("ProductName", APP_NAME);
    res.set("FileDescription", APP_NAME);

    // バージョン情報
    res.set("FileVersion", env!("CARGO_PKG_VERSION"));
    res.set("ProductVersion", env!("CARGO_PKG_VERSION"));

    // 著作権情報
    res.set("LegalCopyright", LEGAL_COPYRIGHT);

    if let Err(error) = res.compile() {
        panic!("Windows リソースの埋め込みに失敗: {error}");
    }
}

// ======================================================================
// CRT の静的リンク
// ======================================================================
/// CRT が静的リンクされない設定でビルドしている場合に警告する
///
/// 環境変数 `RUSTFLAGS` などがあると `.cargo/config.toml` の rustflags は使われず、
/// 気付かないまま CRT が動的リンクになる (配布先に VC++ ランタイムが必要になる)
fn warn_if_crt_not_static() {
    let crt_static = env::var("CARGO_CFG_TARGET_FEATURE")
        .is_ok_and(|features| features.split(',').any(|feature| feature == "crt-static"));
    if !crt_static {
        println!(
            "cargo::warning=CRT が静的リンクされません。環境変数 RUSTFLAGS などがあると .cargo/config.toml の rustflags は使われないため、必要なら -C target-feature=+crt-static を加えてください"
        );
    }
}

// ======================================================================
// 更新ダウンロードキー
// ======================================================================
/// `EDGE_CUT_KEY_FILE` が指すファイルのキーを暗号化して `OUT_DIR` へ書き出す
///
/// キーを環境変数で受け取ると `rerun-if-env-changed` の値が Cargo の fingerprint に平文で残るため、
/// ファイルのパスだけを受け取る。未設定なら空の埋め込みにする
///
/// # Panics
/// `OUT_DIR` が無い、キーファイルが読めない・不正、または生成ファイルの書き込みに失敗した場合
fn embed_bridge_key() {
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR");
    let dest = Path::new(&out_dir).join("bridge_key_blob.rs");

    let source = match env::var_os(BRIDGE_KEY_FILE_ENV) {
        Some(path) if !path.is_empty() => {
            let path = PathBuf::from(path);
            println!("cargo::rerun-if-changed={}", path.display());
            let key = read_bridge_key_file(&path);
            encrypt_bridge_key(&key)
        }
        _ => {
            // 値を fingerprint に残さないよう、廃止した変数は rerun-if-env-changed で監視しない
            if env::var_os(LEGACY_BRIDGE_KEY_ENV).is_some() {
                println!(
                    "cargo::warning=環境変数 {LEGACY_BRIDGE_KEY_ENV} は廃止しました。キーは埋め込まれません。キーをファイルに保存し、{BRIDGE_KEY_FILE_ENV} にそのパスを設定してください"
                );
            }
            empty_bridge_key_source()
        }
    };
    fs::write(&dest, source).unwrap_or_else(|err| {
        panic!("bridge キー埋め込みファイルの書き込みに失敗: {err}");
    });
}

/// キーファイルを読み、BOM と前後の空白・改行を除いたキーを返す
///
/// # Arguments
/// * `path` - キーファイル
///
/// # Returns
/// * `Zeroizing<String>` - キー (破棄時にゼロ化する)
///
/// # Panics
/// ファイルが読めない、UTF-16 または UTF-8 でない、または空の場合
fn read_bridge_key_file(path: &Path) -> Zeroizing<String> {
    let mut bytes = fs::read(path).unwrap_or_else(|err| {
        panic!(
            "エラー: {BRIDGE_KEY_FILE_ENV} のキーファイルを読めません: {}: {err}",
            path.display()
        );
    });
    assert!(
        !bytes.starts_with(&[0xff, 0xfe]) && !bytes.starts_with(&[0xfe, 0xff]),
        "エラー: キーファイルが UTF-16 です。UTF-8 で保存してください: {}",
        path.display()
    );

    let body = bytes.strip_prefix(UTF8_BOM).unwrap_or(&bytes);
    let key = std::str::from_utf8(body)
        .map(|text| Zeroizing::new(text.trim().to_owned()))
        .ok();
    bytes.zeroize();

    let key = key.unwrap_or_else(|| {
        panic!(
            "エラー: キーファイルが UTF-8 ではありません: {}",
            path.display()
        )
    });
    assert!(
        !key.is_empty(),
        "エラー: キーファイルが空です: {}",
        path.display()
    );
    key
}

/// キーが無いときの埋め込み用ソースを返す
///
/// # Returns
/// * `String` - `BRIDGE_KEY_PRESENT = false` の空の埋め込み
fn empty_bridge_key_source() -> String {
    format!(
        "// build.rs が生成する。平文のダウンロードキーは含めない。\n\
         const BRIDGE_KEY_PRESENT: bool = false;\n\
         static BRIDGE_KEY_BLOB: &[u8] = &[];\n\
         const BRIDGE_KEY_WRAP: [u8; {OBFUSCATE_MASK_LEN}] = [0u8; {OBFUSCATE_MASK_LEN}];\n\
         const BRIDGE_KEY_MASK: [u8; {OBFUSCATE_MASK_LEN}] = [0u8; {OBFUSCATE_MASK_LEN}];\n"
    )
}

/// 平文キーを ChaCha20-Poly1305 で暗号化し、埋め込み用ソースを返す
///
/// 暗号化した blob と復号鍵は、同じ乱数マスクで XOR 難読化してから書き出す
///
/// # Arguments
/// * `plaintext` - 平文キー
///
/// # Returns
/// * `String` - 埋め込み用ソース
///
/// # Panics
/// キーが長すぎる、乱数取得に失敗、または暗号化往復検証に失敗した場合
fn encrypt_bridge_key(plaintext: &str) -> String {
    let plain_bytes = plaintext.as_bytes();
    assert!(
        plain_bytes.len() <= MAX_BRIDGE_KEY_BYTES,
        "エラー: bridge ダウンロードキーが長すぎます (最大 {MAX_BRIDGE_KEY_BYTES} バイト)"
    );

    let mut wrap_key = [0u8; OBFUSCATE_MASK_LEN];
    let mut mask = [0u8; OBFUSCATE_MASK_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::fill(&mut wrap_key).expect("wrap key 乱数");
    getrandom::fill(&mut mask).expect("mask 乱数");
    getrandom::fill(&mut nonce).expect("nonce 乱数");

    let mut blob = aead_encrypt(&wrap_key, &nonce, plain_bytes);
    nonce.zeroize();

    let mut wrap_stored = wrap_key;
    xor_obfuscate(&mut blob, &mask);
    xor_obfuscate(&mut wrap_stored, &mask);

    verify_roundtrip(plaintext.as_bytes(), &blob, &wrap_stored, &mask);

    let source = format!(
        "// build.rs が生成する。平文のダウンロードキーは含めない。\n\
         const BRIDGE_KEY_PRESENT: bool = true;\n\
         static BRIDGE_KEY_BLOB: &[u8] = {};\n\
         const BRIDGE_KEY_WRAP: [u8; {OBFUSCATE_MASK_LEN}] = {};\n\
         const BRIDGE_KEY_MASK: [u8; {OBFUSCATE_MASK_LEN}] = {};\n",
        format_byte_slice(&blob),
        format_byte_array_32(&wrap_stored),
        format_byte_array_32(&mask),
    );

    wrap_key.zeroize();
    wrap_stored.zeroize();
    mask.zeroize();
    blob.zeroize();
    source
}

/// 埋め込み blob が平文へ戻ることをビルド時に検証する
///
/// # Arguments
/// * `plaintext` - 平文キー
/// * `blob` - 難読化した `nonce || ciphertext`
/// * `wrap_stored` - 難読化した復号鍵
/// * `mask` - XOR マスク
///
/// # Panics
/// 復号失敗、または平文と一致しない場合
fn verify_roundtrip(
    plaintext: &[u8],
    blob: &[u8],
    wrap_stored: &[u8; OBFUSCATE_MASK_LEN],
    mask: &[u8; OBFUSCATE_MASK_LEN],
) {
    let mut blob_check = blob.to_vec();
    let mut wrap_check = *wrap_stored;
    xor_obfuscate(&mut blob_check, mask);
    xor_obfuscate(&mut wrap_check, mask);
    let decrypted = aead_decrypt(&wrap_check, &blob_check)
        .expect("bridge ダウンロードキーの暗号化往復に失敗しました");
    assert_eq!(
        decrypted, plaintext,
        "bridge ダウンロードキーの暗号化往復結果が一致しません"
    );
    wrap_check.zeroize();
    blob_check.zeroize();
}

/// `nonce || ciphertext` を組み立てる
///
/// # Arguments
/// * `key` - ChaCha20-Poly1305 鍵
/// * `nonce` - nonce
/// * `plaintext` - 平文
///
/// # Returns
/// * `Vec<u8>` - `nonce || ciphertext`
///
/// # Panics
/// 鍵長・nonce 長が不正、または AEAD 暗号化に失敗した場合
fn aead_encrypt(
    key: &[u8; OBFUSCATE_MASK_LEN],
    nonce: &[u8; NONCE_LEN],
    plaintext: &[u8],
) -> Vec<u8> {
    let key = Key::try_from(key.as_slice()).expect("key len");
    let cipher = ChaCha20Poly1305::new(&key);
    let nonce_arr = Nonce::try_from(nonce.as_slice()).expect("nonce len");
    let ciphertext = cipher.encrypt(&nonce_arr, plaintext).expect("aead encrypt");
    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(nonce);
    out.extend_from_slice(&ciphertext);
    out
}

/// `nonce || ciphertext` を復号する
///
/// # Arguments
/// * `key` - ChaCha20-Poly1305 鍵
/// * `blob` - `nonce || ciphertext`
///
/// # Returns
/// * `Option<Vec<u8>>` - 平文。形式不正または認証失敗なら `None`
fn aead_decrypt(key: &[u8; OBFUSCATE_MASK_LEN], blob: &[u8]) -> Option<Vec<u8>> {
    if blob.len() <= NONCE_LEN {
        return None;
    }
    let (nonce, ciphertext) = blob.split_at(NONCE_LEN);
    let key = Key::try_from(key.as_slice()).ok()?;
    let cipher = ChaCha20Poly1305::new(&key);
    let nonce = Nonce::try_from(nonce).ok()?;
    cipher.decrypt(&nonce, ciphertext).ok()
}

/// バイト列を Rust のスライスリテラルにする
fn format_byte_slice(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::from("&[]");
    }
    let mut out = String::from("&[\n");
    append_hex_bytes(&mut out, bytes, 12);
    out.push_str("\n]");
    out
}

/// 32 バイトの配列を Rust の配列リテラルにする
fn format_byte_array_32(bytes: &[u8; OBFUSCATE_MASK_LEN]) -> String {
    let mut out = String::from("[\n");
    append_hex_bytes(&mut out, bytes, 8);
    out.push_str("\n]");
    out
}

/// バイト列を 16 進表記で `per_line` 個ずつ改行して追記する
fn append_hex_bytes(out: &mut String, bytes: &[u8], per_line: usize) {
    for (i, byte) in bytes.iter().enumerate() {
        if i % per_line == 0 {
            if i > 0 {
                out.push('\n');
            }
            out.push_str("    ");
        } else {
            out.push(' ');
        }
        write!(out, "0x{byte:02x},").expect("write hex");
    }
}

// ======================================================================
// 更新署名の公開鍵
// ======================================================================
/// `keys/update-signing/*.pub` の公開鍵を `OUT_DIR` へ書き出す
///
/// ファイル名順に並べる。ディレクトリや `*.pub` が無ければ空の一覧になる (署名検証は常に失敗)
fn embed_update_signing_keys() {
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR");
    let dest = Path::new(&out_dir).join("update_signing_keys.rs");

    let mut paths = match fs::read_dir(UPDATE_SIGNING_KEYS_DIR) {
        Ok(entries) => entries
            .map(|entry| entry.expect("更新署名鍵ディレクトリの列挙").path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "pub"))
            .collect::<Vec<_>>(),
        Err(_) => Vec::new(),
    };
    paths.sort();

    let mut source = String::from(
        "// build.rs が生成する。keys/update-signing/*.pub の minisign 公開鍵 (Base64)\n\
         static UPDATE_SIGNING_KEYS: &[&str] = &[\n",
    );
    for path in &paths {
        let key = read_minisign_public_key(path);
        writeln!(source, "    \"{key}\",").expect("write key");
    }
    source.push_str("];\n");

    fs::write(&dest, source).unwrap_or_else(|err| {
        panic!("更新署名鍵埋め込みファイルの書き込みに失敗: {err}");
    });
}

/// minisign 形式の公開鍵ファイルから Base64 行を取り出す
///
/// 1 行目は `untrusted comment:`、2 行目が鍵本体。形式が不正ならビルドを止める
///
/// # Arguments
/// * `path` - `*.pub` ファイル
///
/// # Returns
/// * `String` - 公開鍵の Base64 文字列
fn read_minisign_public_key(path: &Path) -> String {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("更新署名鍵の読み込みに失敗: {}: {err}", path.display()));
    let mut lines = text.lines();
    let comment = lines.next().unwrap_or_default();
    let key = lines.next().unwrap_or_default().trim();
    let valid = comment.starts_with("untrusted comment:")
        && key.len() == MINISIGN_PUBLIC_KEY_B64_LEN
        && key.starts_with("RW")
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=');
    assert!(
        valid,
        "エラー: minisign 公開鍵の形式が不正です: {}",
        path.display()
    );
    key.to_string()
}
