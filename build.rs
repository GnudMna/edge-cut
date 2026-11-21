const LEGAL_COPYRIGHT: &str = "Copyright (c) 2026 Yuto Hirata";

const APP_NAME: &str = "EdgeCut";

fn main() {
    // 更新ダウンロードキー (gh-release-bridge) の埋め込み対象
    println!("cargo:rerun-if-env-changed=EDGE_CUT_KEY");

    // Windows の場合は exe にリソース情報を埋め込む
    #[cfg(windows)]
    {
        // アイコンパス
        const ICON_PATH: &str = "assets/icons/icon.ico";

        // アイコンパスを再実行条件に追加
        println!("cargo:rerun-if-changed={ICON_PATH}");

        // Windows リソースを設定
        let mut res = winres::WindowsResource::new();

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
}
