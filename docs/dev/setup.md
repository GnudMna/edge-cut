# 開発環境とビルド

前提条件・ビルド・配布パッケージ (MSI・ポータブル版) の作成をまとめる。

## 前提条件

対応 OS は Windows のみ。Linux / macOS ではビルドできない。

| 用途 | 必要なもの |
| :--- | :--- |
| ビルド | [Rust ツールチェーン](https://rustup.rs/) 1.100 (`rust-toolchain.toml` で固定し、rustup が自動で導入する)、Visual Studio Build Tools など MSVC ツールチェーン (C++ ワークロード)、[Node.js](https://nodejs.org/) と [pnpm](https://pnpm.io/) (UI のビルド) |
| 実行 | Microsoft Edge WebView2 ランタイム (Windows 11 は標準で導入済み) |
| 品質チェック | [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) |
| 配布パッケージ作成 | MSI・ポータブル版の共通: [cargo-about](https://github.com/EmbarkStudios/cargo-about)、[mdBook](https://rust-lang.github.io/mdBook/) ([利用者向けドキュメントの同梱](#利用者向けドキュメントの同梱) の固定版)。MSI のみ: [cargo-wix](https://github.com/volks73/cargo-wix)、[WiX Toolset v3.14.1](https://github.com/wixtoolset/wix3/releases) 以上 |

`cargo build` だけなら `cargo-deny`・配布パッケージ作成用のツールは不要だが、Node.js と pnpm は必要 (UI のビルドを `build.rs` が行うため)。mdBook は固定版が必要で、未導入や版違いのときはパッケージ作成スクリプトが導入コマンドを表示して止まる。

```powershell
cargo install cargo-deny --locked
cargo install cargo-about --locked --features cli
cargo install cargo-wix --locked
winget install WiXToolset.WiXToolset
```

## ビルド

```bash
cargo run                 # デバッグビルド & 実行
cargo build --release     # リリースビルド
```

ビルド成果物は `target/release/EdgeCut.exe`。

### UI (TypeScript)

UI は `ui/` の React + TypeScript で、Vite でビルドする。`build.rs` が `ui/` で `pnpm run build` を実行し、出力を `OUT_DIR` に置いて実行ファイルへ埋め込む (`ui/node_modules` が無ければ先に `pnpm install --frozen-lockfile` を実行する)。`ui/src` などを変更すると `cargo build` で UI も作り直す。

| 環境変数 | 説明 |
| :--- | :--- |
| `EDGECUT_UI_DIST` | ビルド済みの UI のディレクトリ。指定すると pnpm でのビルドを省き、そのディレクトリを埋め込む |

```bash
pnpm --dir ui install      # UI の依存を導入する (ロックファイルを更新する場合)
pnpm --dir ui run build    # UI だけをビルドする (出力は ui/dist/)
```

デバッグビルドでは `WebView2` の開発者ツールと右クリックメニューを有効にする (リリースビルドでは無効)。

CRT (C ランタイム) は静的リンクする (`.cargo/config.toml` の `+crt-static`)。配布先に VC++ ランタイムを導入しなくても起動できる。環境変数 `RUSTFLAGS` などを設定すると `.cargo/config.toml` の rustflags は使われず動的リンクになるため、`build.rs` が警告を出す。`RUSTFLAGS` を使う場合は `-C target-feature=+crt-static` も加える。

デバッグビルドでも依存クレートだけは最適化する (`Cargo.toml` の `[profile.dev.package."*"]`)。画像のデコード・エンコードが遅くなるのを避けるための設定で、自作コードは最適化しない。初回のビルドは依存の再コンパイルで時間がかかる。

## スクリプト

ラッパースクリプトを `scripts/windows/` に置く。PowerShell 7 向けで、エクスプローラーからのダブルクリック実行にも対応する。共通処理は `scripts/common/` に置く。

品質チェック用のスクリプト (`check.ps1` / `test.ps1` / `clippy.ps1` / `format.ps1`) は [テストと品質チェック](testing.md#品質チェック) を参照。それ以外は次のとおり。

| スクリプト | 説明 |
| :--- | :--- |
| `windows/build.ps1` | デバッグ / リリースビルド (`-Release` / `--release` / `-r`。それ以外の引数はエラー) |
| `windows/build-msi.ps1` | MSI インストーラー作成と、作成後の内容検証 (`-SkipBuild` でビルド、`-SkipVerify` で検証を省略可) |
| `windows/build-portable.ps1` | ポータブル版の ZIP 作成 (`-SkipBuild` でビルドを省略可。[配布パッケージ (ポータブル版)](#配布パッケージ-ポータブル版)) |
| `windows/verify-msi.ps1` | ビルド済み MSI の内容検証 (`-MsiPath` で対象を指定、`-SkipExtract` で展開を省略可。[MSI の検証](#msi-の検証)) |
| `windows/generate-wix-assets.ps1` | MSI 用のバナー・ダイアログ画像 (`wix/*.bmp`) を再生成 |
| `windows/release.ps1` | リリース一括実行 ([リリース](release.md)) |

## デバッグ (VS Code)

`.vscode/launch.json` に CodeLLDB (`vadimcn.vscode-lldb`。推奨拡張機能に含む) 向けの構成を置く。アプリ本体・ユニットテスト (名前で絞り込み可)・統合テスト (`integration_test`) の起動と、実行中プロセスへのアタッチを選べる。rust-analyzer が標準ライブラリを解析できるよう、`rust-toolchain.toml` で `rust-src` を導入する。

## 配布パッケージ (MSI)

`build-msi.ps1` は、`THIRD_PARTY_NOTICES` と利用者向けドキュメントを生成してから同梱する。

| 項目 | 内容 |
| :--- | :--- |
| 出力 | `target/wix/{パッケージ名}-{バージョン}-{アーキテクチャ}.msi` |
| インストール先 | `%LOCALAPPDATA%\edge-cut` (ユーザー単位) |
| 同梱内容 | `bin\EdgeCut.exe` / `bin\EdgeCutUpdater.exe` / `LICENSE` / `THIRD_PARTY_NOTICES` / `docs\` |
| スタートメニュー | **EdgeCut** と **EdgeCut ドキュメント** を追加 |
| 画面 | `WixUI_FeatureTree` からライセンス同意画面を除いた独自 UI (`wix/main.wxs`)。完了画面の「EdgeCut を起動する」チェックボックスは既定でオン。更新プログラムの無人インストール (`msiexec /qn`) では画面を出さない |

### MSI の検証

`build-msi.ps1` は MSI の作成後に `scripts/common/verify-msi.ps1` で次を検証し、満たさなければ失敗する。単体では `verify-msi.ps1` で実行できる。

| 対象 | 内容 |
| :--- | :--- |
| メタデータ | `Cargo.toml` の `upgrade-guid` と `wix/main.wxs` の `UpgradeCode` の一致、ProductName・ProductVersion (`Cargo.toml` の version)・Manufacturer・言語・Platform (x64)・per-user (`ALLUSERS`) |
| 収録内容 | 必須ファイル (`EdgeCut.exe`・`EdgeCutUpdater.exe`・`LICENSE`・`THIRD_PARTY_NOTICES`・`docs\index.html`) と Feature (`Binaries`・`Environment`) |
| 画面 | ライセンス同意画面 (`LicenseAgreementDlg`) が無く、完了画面に起動のチェックボックスと起動処理がある |
| 展開した内容 | 管理インストール (`msiexec /a`) で一時フォルダへ展開し、実行ファイルのサイズと PE アーキテクチャ (x64)、`THIRD_PARTY_NOTICES` のヘッダーとビルド時の生成物との一致、`docs/user/SUMMARY.md` の各ページの HTML を確かめる |

## 配布パッケージ (ポータブル版)

`build-portable.ps1` は、MSI と同じく `THIRD_PARTY_NOTICES` と利用者向けドキュメントを生成してから ZIP にまとめる。

| 項目 | 内容 |
| :--- | :--- |
| 出力 | `target/portable/edge-cut-{バージョン}-x86_64-portable.zip` (展開すると同名のフォルダ 1 つにまとまる。ZIP にする前のフォルダも同じ場所に残る) |
| 同梱内容 | `EdgeCut.exe` / `EdgeCutUpdater.exe` / `LICENSE` / `THIRD_PARTY_NOTICES` / `docs\` / `portable` (空ファイル) / `PORTABLE.txt` (利用者向けの説明) |
| 判定 | 実行ファイルの隣に `portable` があるとポータブル版として動き、設定・`WebView2` のデータ・ログを実行フォルダの `data\` に保存する (`src/install_layout.rs`)。`portable` を消すと MSI 版と同じ保存先を使う |
| 更新 | 更新プログラムが ZIP を展開し、実行フォルダのファイルを差し替える (`data\` は残す) |

## 第三者ライセンス表示 (`THIRD_PARTY_NOTICES`)

`LICENSE` は EdgeCut 本体の利用条件を示す。配布バイナリに含まれる OSS 依存の帰属・ライセンス全文は `THIRD_PARTY_NOTICES` にまとめる。

| 項目 | 内容 |
| :--- | :--- |
| 生成ツール | cargo-about (Rust の依存)、`ui/scripts/third-party-notices.mjs` (UI に同梱する npm パッケージ) |
| 設定 | [`about.toml`](../../about.toml)、テンプレート [`scripts/templates/third-party-notices.hbs`](../../scripts/templates/third-party-notices.hbs) |
| 手動生成 | `scripts/common/generate-third-party-notices.ps1` |
| 出力 | リポジトリルートの `THIRD_PARTY_NOTICES` (`.gitignore` 対象。パッケージ作成時に自動生成) |
| ターゲット | `x86_64-pc-windows-msvc` のみ (`deny.toml` と同じ) |

## 利用者向けドキュメントの同梱

[`docs/user/`](../user/) を原稿とする mdBook の本を HTML にし、MSI とポータブル版の `docs\` に同梱する。

| 項目 | 内容 |
| :--- | :--- |
| 原稿 | `docs/user/` (設定 [`book.toml`](../user/book.toml)、目次 [`SUMMARY.md`](../user/SUMMARY.md)、表紙 `introduction.md`) |
| 生成 | `scripts/common/build-user-docs.ps1`。パッケージ作成スクリプトが呼ぶ。出力は `target/user-docs/` |
| mdBook の版 | `build-user-docs.ps1` の `$UserDocsMdBookVersion` に固定し、異なる版では失敗する (0.5 以降は静的ファイル名にハッシュが付き、版で出力が変わるため)。版の指定はこの定数だけにあり、他の文書には書かない |
| 変更履歴 | 原稿を `target/user-docs-src/` へ複製し、`changelog.md` を `CHANGELOG.md` の内容で置き換えてからビルドする。先頭の記載規約 (開発者向け) は除く。`docs/user/...` へのリンクは本のページへ書き換え、本に含めない相対リンクはリンクを外す。箇条書きは「。」の位置で折り返す |
| リンク検証 | 出力 HTML の相対リンクについて、リンク先ファイルとアンカーが存在しなければ失敗する |
| MSI への収録 | ファイル名が版で変わるため、出力から WiX 断片 `target/wix/user-docs.wxs` (ComponentGroup `UserDocs`) を生成し、`cargo wix -I` で `wix/main.wxs` に加えて渡す。per-user 構成のため、ディレクトリごとに HKCU の値を KeyPath とする Component を作る |

- 執筆中のプレビューは `mdbook serve docs/user --open` で行える。このとき変更履歴ページは `CHANGELOG.md` をそのまま取り込むため、ページ内の相対リンクは辿れない (配布物では書き換わる)

### 同梱する第三者素材とライセンス

mdBook の出力には mdBook 自身と他のプロジェクトの素材が含まれる。cargo-about は Rust の依存だけを扱うため、これらの表示は [`scripts/templates/user-docs-notices.txt`](../../scripts/templates/user-docs-notices.txt) に書き、`generate-third-party-notices.ps1` が `THIRD_PARTY_NOTICES` の末尾に追記する。

| 素材 | ライセンス | 扱い |
| :--- | :--- | :--- |
| mdBook のテーマ (ページの雛形・`book.js`・`toc.js`・CSS・favicon) | MPL-2.0 | 改変せずに同梱し、ライセンス全文とソースの入手先を表示する |
| highlight.js | BSD-3-Clause | ライセンス全文を表示する |
| clipboard.js | MIT | ライセンス全文を表示する |
| Font Awesome Free のアイコン (ページ内の SVG) | CC BY 4.0 | 帰属を表示する (SVG 内のコメントにもある) |
| Open Sans / Source Code Pro (mdBook 既定の Web フォント) | Apache-2.0 / OFL-1.1 | 同梱しない。[`docs/user/theme/fonts/fonts.css`](../user/theme/fonts/fonts.css) を空にして外している (本文は日本語が中心で、閲覧環境のフォントで表示されるため) |

- **MPL-2.0 の扱い:** `deny.toml` の方針では MPL を許可しないが、これは Rust の依存クレートに対するもの。mdBook のテーマはリンクしない別ファイルで、改変せずにソースのまま配るため、ファイル単位の義務 (ライセンス全文とソースの入手先の表示) を満たせば本体のライセンスに影響しない。この例外は `deny.toml` の冒頭にも記す。テーマのファイルを改変 (`docs/user/theme/` で上書き) する場合は、改変したファイルも MPL-2.0 で提供する必要があるため、フォントの無効化 (空の `fonts.css`) 以外の上書きは避ける
- **検証:** 生成スクリプトは、出力にフォントが含まれないこと、出力に含まれる mdBook・highlight.js・clipboard.js・Font Awesome の版が `user-docs-notices.txt` に書かれていることを確かめ、合わなければ失敗する。mdBook の版を上げたら、同梱素材とライセンスを確認して `user-docs-notices.txt` を更新する
