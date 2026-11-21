use std::env;
use std::fmt::Write;
use std::fs;
use std::path::Path;

const LEGAL_COPYRIGHT: &str = "Copyright (c) 2026 Yuto Hirata";

const APP_NAME: &str = "EdgeCut";

/// exe に埋め込むアイコン
const ICON_PATH: &str = "assets/icons/icon.ico";

/// 更新署名の公開鍵 (minisign 形式 `*.pub`) を置くディレクトリ
const UPDATE_SIGNING_KEYS_DIR: &str = "keys/update-signing";

/// minisign 公開鍵の Base64 長 (署名方式 2 + 鍵 ID 8 + Ed25519 公開鍵 32 バイト)
const MINISIGN_PUBLIC_KEY_B64_LEN: usize = 56;

fn main() {
    // 更新ダウンロードキー (gh-release-bridge) の埋め込み対象
    println!("cargo:rerun-if-env-changed=EDGE_CUT_KEY");
    println!("cargo:rerun-if-changed={UPDATE_SIGNING_KEYS_DIR}");

    embed_update_signing_keys();

    // exe にリソース情報を埋め込む
    // アイコンパスを再実行条件に追加
    println!("cargo:rerun-if-changed={ICON_PATH}");

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
