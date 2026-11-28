//! Windows リソース埋め込み、更新署名の公開鍵埋め込み、UI のビルドと埋め込み
//!
//! `keys/update-signing/*.pub` の minisign 公開鍵を `OUT_DIR` へ書き出す。
//! `ui/` の TypeScript を pnpm でビルドし、出力を `include_bytes!` する一覧を `OUT_DIR` へ書き出す

#![expect(
    clippy::expect_used,
    reason = "ビルドスクリプトは失敗時に panic してビルドを中断するのが正規の挙動"
)]

use std::env;
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const LEGAL_COPYRIGHT: &str = "Copyright (c) 2026 Yuto Hirata";

const APP_NAME: &str = "EdgeCut";

/// exe に埋め込むアイコン
const ICON_PATH: &str = "assets/icons/icon.ico";

/// 更新署名の公開鍵 (minisign 形式 `*.pub`) を置くディレクトリ
const UPDATE_SIGNING_KEYS_DIR: &str = "keys/update-signing";

/// UI (TypeScript) のソースディレクトリ
const UI_DIR: &str = "ui";

/// ビルド済みの UI を使う場合に出力ディレクトリを指定する環境変数 (pnpm でのビルドを省く)
const UI_DIST_ENV: &str = "EDGECUT_UI_DIST";

/// UI の再ビルド条件にするファイル・ディレクトリ (`ui/` からの相対パス)
const UI_INPUTS: &[&str] = &[
    "src",
    "index.html",
    "package.json",
    "pnpm-lock.yaml",
    "tsconfig.json",
    "vite.config.ts",
];

/// minisign 公開鍵の Base64 長 (署名方式 2 + 鍵 ID 8 + Ed25519 公開鍵 32 バイト)
const MINISIGN_PUBLIC_KEY_B64_LEN: usize = 56;

fn main() {
    println!("cargo::rerun-if-changed={UPDATE_SIGNING_KEYS_DIR}");

    warn_if_crt_not_static();
    embed_update_signing_keys();
    embed_ui();

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

// ======================================================================
// UI
// ======================================================================
/// UI をビルドし、出力ファイルを埋め込む一覧を `OUT_DIR/ui_assets.rs` へ書き出す
///
/// `EDGECUT_UI_DIST` があればそのディレクトリをビルド済みの出力として使う
fn embed_ui() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    println!("cargo::rerun-if-env-changed={UI_DIST_ENV}");

    let dist = if let Some(dist) = env::var_os(UI_DIST_ENV) {
        PathBuf::from(dist)
    } else {
        for input in UI_INPUTS {
            println!("cargo::rerun-if-changed={UI_DIR}/{input}");
        }
        let dist = out_dir.join("ui-dist");
        build_ui(&dist);
        dist
    };

    let mut files = Vec::new();
    collect_files(&dist, &mut files);
    files.sort();
    assert!(
        files.iter().any(|f| f.ends_with("index.html")),
        "UI のビルド結果に index.html がありません: {}",
        dist.display()
    );

    let mut source = String::from(
        "// build.rs が生成する。UI のビルド結果 (パス, MIME タイプ, 内容)\n\
         static UI_ASSETS: &[(&str, &str, &[u8])] = &[\n",
    );
    let dist = dist.canonicalize().expect("UI の出力ディレクトリ");
    for file in &files {
        let abs = file.canonicalize().expect("UI の出力ファイルのパス");
        let rel = abs
            .strip_prefix(&dist)
            .expect("出力ディレクトリ配下")
            .to_string_lossy()
            .replace('\\', "/");
        writeln!(
            source,
            "    ({rel:?}, {:?}, include_bytes!({:?})),",
            mime_type(&rel),
            abs.display().to_string()
        )
        .expect("write asset");
    }
    source.push_str("];\n");
    fs::write(out_dir.join("ui_assets.rs"), source).unwrap_or_else(|err| {
        panic!("UI 埋め込みファイルの書き込みに失敗: {err}");
    });
}

/// pnpm で UI をビルドする
///
/// 依存が未インストールならロックファイルどおりにインストールしてからビルドする
///
/// # Arguments
/// * `dist` - 出力ディレクトリ
fn build_ui(dist: &Path) {
    if !Path::new(UI_DIR).join("node_modules").is_dir() {
        run_pnpm(&["install", "--frozen-lockfile"]);
    }
    let dist = dist.to_string_lossy();
    run_pnpm(&["run", "build", "--outDir", &dist, "--emptyOutDir"]);
}

/// `ui/` で pnpm を実行し、失敗したらビルドを止める
///
/// Windows の pnpm はバッチファイル (`pnpm.cmd`) のため `cmd /C` から起動する
///
/// # Arguments
/// * `args` - pnpm の引数
fn run_pnpm(args: &[&str]) {
    let status = Command::new("cmd")
        .args(["/C", "pnpm"])
        .args(args)
        .current_dir(UI_DIR)
        .status()
        .unwrap_or_else(|err| {
            panic!("pnpm を起動できません (Node.js と pnpm が必要です): {err}");
        });
    assert!(
        status.success(),
        "pnpm {} が失敗しました ({status})",
        args.join(" ")
    );
}

/// ディレクトリ配下のファイルを再帰的に集める
///
/// # Arguments
/// * `dir` - 探すディレクトリ
/// * `files` - 見つけたファイルの追加先
fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("UI の出力を読めません: {}: {err}", dir.display()));
    for entry in entries {
        let path = entry.expect("UI の出力の列挙").path();
        if path.is_dir() {
            collect_files(&path, files);
        } else {
            files.push(path);
        }
    }
}

/// 拡張子から MIME タイプを決める
///
/// # Arguments
/// * `path` - ファイルのパス
///
/// # Returns
/// * `&str` - MIME タイプ
fn mime_type(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or_default();
    match ext {
        "html" => "text/html",
        "js" => "text/javascript",
        "css" => "text/css",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "json" => "application/json",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}
