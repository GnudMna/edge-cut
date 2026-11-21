# CLAUDE.md

画像の背景余白を自動検出・トリミングする Rust 製デスクトップ GUI ツール (EdgeCut)。eframe / egui ベースで Windows / Linux / macOS に対応する。

## コマンド

```bash
cargo run                                 # デバッグ実行
cargo build --release                     # リリースビルド (target/release/EdgeCut)
cargo fmt --all                           # 整形
cargo clippy --all-targets -- -D warnings # 静的解析 (警告はエラー扱い)
cargo test --all-targets                  # ユニット・統合テスト
cargo test --doc                          # doctest
cargo test <テスト名>                      # 単一テスト
cargo deny check                          # 依存の脆弱性・ライセンス監査
```

一括チェックは `scripts/windows/check.ps1` (Linux / macOS は `scripts/{linux,macos}/check.sh`)。変更後はこれと同等の fmt / clippy / test が通ることを確認する。

- ツールチェーンは `rust-toolchain.toml` で 1.96 固定、edition 2024
- `clippy::pedantic` を warn で有効化し、`unsafe_code` は forbid
- rustfmt の `max_width` は 100

## アーキテクチャ

lib クレート `edge_cut` (`src/lib.rs`) と bin クレート `EdgeCut` (`src/main.rs`) の二層構成。`main.rs` はエントリポイントのみで、`edge_cut::app::run()` を呼ぶだけ。

| パス | 責務 |
| ---- | ---- |
| `src/trim.rs` | 背景色検出 (四辺を量子化して近傍で集計し、最多クラスタ内の最頻 RGBA)・余白除去のコアロジック |
| `src/image_load.rs` | サイズ上限付きの安全な画像読み込み (EXIF Orientation を反映、ICC プロファイルを取得) |
| `src/image_output.rs` | 出力形式 (`OutputFormat`) の決定・画像保存 (ICC プロファイルを再埋め込み)・出力パス生成 (衝突時は連番サフィックス) |
| `src/batch_trim.rs` | 複数ファイルの一括トリミング (進捗通知・キャンセル) |
| `src/panic_message.rs` | `catch_panic` で処理中の panic を捕捉してメッセージ化 (ワーカーの panic を失敗として扱うため) |
| `src/consts.rs` | 上限値・対応拡張子・ウィンドウサイズなどの定数 |
| `src/app/` | egui GUI。`state` (状態)、`view/` (描画。`toolbar` / `settings` / `preview` / `input` / `update_dialog` に分割)、`actions/` (操作。`dialog` / `load` / `io_result` / `trim` / `save` / `clipboard` / `batch` / `drop` / `app_update` に責務ごとに分割)、`trim_worker` (プレビュー用ワーカースレッド)、`clipboard_worker` (コピー・貼り付け用ワーカースレッド)、`load_worker` (画像読み込み用ワーカースレッド)、`shortcuts` (キーボードショートカットの判定)、`image_convert` (プレビュー用テクスチャ変換)、`fonts` / `icon` |
| `src/update/` | gh-release-bridge 経由の更新確認・minisign 署名検証・MSI 適用。`#[cfg(windows)]` 限定 |
| `src/test_helpers.rs` | `#[cfg(test)]` 限定。`tests/common/images.rs` を `include!` してユニットテストと画像生成ヘルパーを共有 |
| `tests/` | lib クレートの公開 API に対する統合テスト (`tests/common/` に画像生成ヘルパー) |
| `docs/user/` | 利用者向けドキュメント (mdBook の原稿)。`scripts/common/build-user-docs.{ps1,sh}` が HTML 化して配布物の `docs/` に同梱する |
| `docs/dev/` | 開発者向けドキュメント (環境構築・テスト・アーキテクチャ・リリース・開発規約) |

- コアロジック (`trim` / `image_load` / `image_output` / `batch_trim`) は egui に依存させない。egui 依存は `src/app/` に閉じ込める
- 重い処理は UI スレッドで行わず、ワーカースレッドへ渡す
- 公開 API を追加・変更した場合は `src/lib.rs` の再エクスポートとモジュール表を更新する
- Windows 専用依存 (`ureq`, `sha2` など) は `cfg(windows)` の中でのみ使う
- `build.rs` は Windows のアイコン・バージョン情報を埋め込む。更新ダウンロードキーはビルド時環境変数 `EDGE_CUT_KEY` から埋め込む
- 更新署名の公開鍵 (minisign) は `keys/update-signing/*.pub` を `build.rs` が埋め込む。`src/update/signature.rs` の信頼コメント形式は `scripts/common/update-signing.ps1` と揃える
- リリースは `scripts/windows/release.ps1` で行う (バージョン更新・CHANGELOG・チェック・コミット・MSI 作成・署名)

## コーディング規約

- コメント: [.claude/rules/comments.md](.claude/rules/comments.md)
- use 文: [.claude/rules/imports.md](.claude/rules/imports.md)

依存を追加する場合は `deny.toml` の許可ライセンス (パーミッシブのみ) に収まるか確認する。`about.toml` の許可リストも同じ内容に揃える。

## Git 運用

- git-flow 形式。`develop` から `feature/*` `fix/*` `docs/*` などを切り、`main` へは `release/vX.Y.Z` 経由でマージする
- コミットメッセージは `<種別>: <日本語の要約>`。種別は `Feat` / `Update` / `Fix` / `Refactor` / `Perf` / `Test` / `Docs` / `Style` / `Chore` / `Release` / `Add`
- 利用者に影響する変更は `CHANGELOG.md` の `[Unreleased]` に追記する。書き方は `CHANGELOG.md` 冒頭の規約に従う (1 項目 1 文・体言止め・補足は入れ子。利用者に見えない変更は書かない)
- 操作・表示が変わる変更では `docs/user/` の該当ページも更新する。ドキュメントの置き場所は [docs/dev/contributing.md](docs/dev/contributing.md#ドキュメントの配置) に従い、同じ説明は 1 か所にだけ書く
