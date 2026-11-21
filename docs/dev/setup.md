# 開発環境とビルド

前提条件・ビルド・配布パッケージ (MSI / tar.gz / .app・DMG) の作成をまとめる。

## 前提条件

| 用途 | 必要なもの |
| :--- | :--- |
| ビルド | [Rust ツールチェーン](https://rustup.rs/) 1.96 以上 (`rust-toolchain.toml` で固定) |
| 品質チェック | [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) (`cargo install cargo-deny --locked`) |
| 配布パッケージ作成 (共通) | [cargo-about](https://github.com/EmbarkStudios/cargo-about) (`cargo install cargo-about --locked --features cli`)、[mdBook](https://rust-lang.github.io/mdBook/) ([利用者向けドキュメントの同梱](#利用者向けドキュメントの同梱) の固定版)。Linux / macOS では perl も使う |
| MSI 作成 | [cargo-wix](https://github.com/volks73/cargo-wix) と [WiX Toolset v3.14.1](https://github.com/wixtoolset/wix3/releases) 以上 |

`cargo build` だけなら `cargo-deny`・配布用ツールは不要。mdBook は固定版が必要で、未導入や版違いのときはパッケージ作成スクリプトが導入コマンドを表示して止まる。

### Windows

- Visual Studio Build Tools など MSVC ツールチェーン (C++ ワークロード)

```powershell
cargo install cargo-wix --locked
winget install WiXToolset.WiXToolset
```

### Linux

GUI 表示には X11 または Wayland 向けの開発ライブラリが必要。Ubuntu / Debian の例:

```bash
sudo apt install libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
  libspeechd-dev libxkbcommon-dev libgtk-3-dev libssl-dev
```

日本語表示には日本語フォントが必要 (利用者向けの説明は [トラブルシューティング](../user/troubleshooting.md#日本語が表示されない-linux))。

### macOS

- Xcode Command Line Tools (`xcode-select --install`)

## ビルド

```bash
cargo run                 # デバッグビルド & 実行
cargo build --release     # リリースビルド
```

ビルド成果物は `target/release/EdgeCut` (Windows では `EdgeCut.exe`)。

## スクリプト

各プラットフォーム向けのラッパースクリプトを `scripts/` 以下に置く。

| プラットフォーム | ディレクトリ | 備考 |
| :--------------: | ------------ | ---- |
| Windows | `scripts/windows/` | PowerShell 7 向け (エクスプローラーからのダブルクリック実行にも対応) |
| Linux | `scripts/linux/` | Bash 向け |
| macOS | `scripts/macos/` | Bash 向け |

共通のスクリプト (`build.*` / `test.*` / `clippy.*` / `format.*` / `check.*`) は [テストと品質チェック](testing.md) を参照。

| スクリプト | 説明 |
| ---------- | ---- |
| `windows/build.ps1`, `{linux,macos}/build.sh` | デバッグ / リリースビルド (`--release`。sh は `-r` も可) |
| `windows/build-msi.ps1` | MSI インストーラー作成 (`-SkipBuild` でビルド省略可) |
| `windows/generate-wix-assets.ps1` | MSI 用のバナー・ダイアログ画像 (`wix/*.bmp`) と使用許諾 (`wix/Eula.rtf`) を再生成 |
| `windows/release.ps1` | リリース一括実行 ([リリース](release.md)) |
| `linux/build-package.sh` | 配布用 tar.gz 作成 (`--skip-build` でビルド省略可) |
| `macos/build-app.sh` | `.app` バンドル作成 (`--skip-build` でビルド省略可) |
| `macos/build-dmg.sh` | DMG 配布イメージ作成 (`--skip-app` で `.app` 生成を省略可) |

## 配布パッケージ

パッケージ作成スクリプトは、`THIRD_PARTY_NOTICES` と利用者向けドキュメントを生成してから同梱する。

| 形態 | 出力 | 同梱内容 |
| :--- | :--- | :--- |
| **Windows MSI** | `target/wix/{パッケージ名}-{バージョン}-{アーキテクチャ}.msi` | `%LOCALAPPDATA%\edge-cut` へインストール。`bin\EdgeCut.exe` / `LICENSE` / `THIRD_PARTY_NOTICES` / `docs\`。スタートメニューに **EdgeCut** と **EdgeCut ドキュメント** を追加 |
| **Linux tar.gz** | `target/dist/{パッケージ名}-{バージョン}-{ターゲット}.tar.gz` | `EdgeCut` / `LICENSE` / `THIRD_PARTY_NOTICES` / `docs/` / デスクトップエントリ / アイコン / `install.sh`。`install.sh` は `~/.local` にインストールする |
| **macOS .app** | `target/dist/EdgeCut.app` | `Contents/Resources/` に `LICENSE` / `THIRD_PARTY_NOTICES` / `docs/` |
| **macOS DMG** | `target/dist/{パッケージ名}-{バージョン}-{ターゲット}.dmg` | `EdgeCut.app`・`Applications` へのリンク・`docs/` |

## 第三者ライセンス表示 (`THIRD_PARTY_NOTICES`)

`LICENSE` は EdgeCut 本体の利用条件を示す。配布バイナリに含まれる OSS 依存の帰属・ライセンス全文は `THIRD_PARTY_NOTICES` にまとめる。

| 項目 | 内容 |
| :--- | :--- |
| 生成ツール | cargo-about |
| 設定 | [`about.toml`](../../about.toml)、テンプレート [`scripts/templates/third-party-notices.hbs`](../../scripts/templates/third-party-notices.hbs) |
| 手動生成 | `scripts/common/generate-third-party-notices.ps1` (Windows) / `generate-third-party-notices.sh [TARGET]` (Linux / macOS) |
| 出力 | リポジトリルートの `THIRD_PARTY_NOTICES` (`.gitignore` 対象。パッケージ作成時に自動生成) |
| ターゲット | 配布スクリプトが OS 別 triple を指定 (`deny.toml` と同じ 3 ターゲット) |

## 利用者向けドキュメントの同梱

[`docs/user/`](../user/) を原稿とする mdBook の本を HTML にし、各配布物の `docs/` に同梱する。

| 項目 | 内容 |
| :--- | :--- |
| 原稿 | `docs/user/` (設定 [`book.toml`](../user/book.toml)、目次 [`SUMMARY.md`](../user/SUMMARY.md)、表紙 `introduction.md`) |
| 生成 | `scripts/common/build-user-docs.ps1` (Windows) / `build-user-docs.sh` (Linux / macOS。perl を使用)。各パッケージ作成スクリプトが呼ぶ。出力は `target/user-docs/` |
| mdBook の版 | `build-user-docs.ps1` の `$UserDocsMdBookVersion` に固定し、異なる版では失敗する (0.5 以降は静的ファイル名にハッシュが付き、版で出力が変わるため)。`build-user-docs.sh` もこの定数を読む。版の指定はこの定数だけにあり、他の文書には書かない |
| 変更履歴 | 原稿を `target/user-docs-src/` へ複製し、`changelog.md` を `CHANGELOG.md` の内容で置き換えてからビルドする。先頭の記載規約 (開発者向け) は除く。`docs/user/...` へのリンクは本のページへ書き換え、本に含めない相対リンクはリンクを外す。箇条書きは「。」の位置で折り返す |
| リンク検証 | 出力 HTML の相対リンクについて、リンク先ファイルとアンカーが存在しなければ失敗する |
| MSI への収録 | ファイル名が版で変わるため、出力から WiX 断片 `target/wix/user-docs.wxs` (ComponentGroup `UserDocs`) を生成し、`cargo wix -I` で `wix/main.wxs` に加えて渡す。per-user 構成のため、ディレクトリごとに HKCU の値を KeyPath とする Component を作る |

### 同梱する第三者素材とライセンス

mdBook の出力には mdBook 自身と他のプロジェクトの素材が含まれる。cargo-about は Rust の依存だけを扱うため、これらの表示は [`scripts/templates/user-docs-notices.txt`](../../scripts/templates/user-docs-notices.txt) に書き、`generate-third-party-notices.*` が `THIRD_PARTY_NOTICES` の末尾に追記する。

| 素材 | ライセンス | 扱い |
| :--- | :--- | :--- |
| mdBook のテーマ (ページの雛形・`book.js`・`toc.js`・CSS・favicon) | MPL-2.0 | 改変せずに同梱し、ライセンス全文とソースの入手先を表示する |
| highlight.js | BSD-3-Clause | ライセンス全文を表示する |
| clipboard.js | MIT | ライセンス全文を表示する |
| Font Awesome Free のアイコン (ページ内の SVG) | CC BY 4.0 | 帰属を表示する (SVG 内のコメントにもある) |
| Open Sans / Source Code Pro (mdBook 既定の Web フォント) | Apache-2.0 / OFL-1.1 | 同梱しない。[`docs/user/theme/fonts/fonts.css`](../user/theme/fonts/fonts.css) を空にして外している (本文は日本語が中心で、閲覧環境のフォントで表示されるため) |

- **MPL-2.0 の扱い:** `deny.toml` の方針では MPL を許可しないが、これは Rust の依存クレートに対するもの。mdBook のテーマはリンクしない別ファイルで、改変せずにソースのまま配るため、ファイル単位の義務 (ライセンス全文とソースの入手先の表示) を満たせば本体のライセンスに影響しない。この例外は `deny.toml` の冒頭にも記す。テーマのファイルを改変 (`docs/user/theme/` で上書き) する場合は、改変したファイルも MPL-2.0 で提供する必要があるため、フォントの無効化 (空の `fonts.css`) 以外の上書きは避ける
- **検証:** 生成スクリプトは、出力にフォントが含まれないこと、出力に含まれる mdBook・highlight.js・clipboard.js・Font Awesome の版が `user-docs-notices.txt` に書かれていることを確かめ、合わなければ失敗する。mdBook の版を上げたら、同梱素材とライセンスを確認して `user-docs-notices.txt` を更新する

- 執筆中のプレビューは `mdbook serve docs/user --open` で行える。このとき変更履歴ページは `CHANGELOG.md` をそのまま取り込むため、ページ内の相対リンクは辿れない (配布物では書き換わる)
- Windows と Linux / macOS の生成スクリプトは同じ規則で同じ出力になるよう揃えてある。変換規則やリンク検証を変える場合は両方を直す
