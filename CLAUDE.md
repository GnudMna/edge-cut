# EdgeCut

画像の背景余白を自動検出・トリミングする Rust 製デスクトップ GUI ツール。ウィンドウは tao、表示は wry (WebView2) で、UI は `ui/` の React + TypeScript。対応 OS は Windows のみ (Linux / macOS は非対応)。開発者向けドキュメントは [docs/dev/](docs/dev/) (環境構築・テスト・アーキテクチャ・リリース・開発規約)、利用者向けは [docs/user/](docs/user/) にある。どの文書に何を書くかは [docs/dev/contributing.md のドキュメントの配置](docs/dev/contributing.md#ドキュメントの配置) に従う。

## 構成

lib クレート `edge_cut` (`src/lib.rs`) と、bin の `EdgeCut` (`src/main.rs`、本体) と `EdgeCutUpdater` (`src/bin/updater.rs`、更新プログラム) の構成。bin は `edge_cut::app::run()` / `run_updater()` を呼ぶだけ。モジュールごとの責務は [docs/dev/architecture.md](docs/dev/architecture.md) を参照する。

- `src/trim/` / `image_load.rs` / `image_output/` / `batch_trim/` — コアロジック (背景色検出・読み込み・保存・一括処理)。GUI (tao / wry) に依存させない。800 行を超えるファイルはサブモジュールに分割し、テストは `tests.rs` か各モジュール末尾の `mod tests` に置く
- `src/app/` — GUI のホスト側 (`event_loop` / `ipc/` / `protocol` / `state/` / `actions/` / 各ワーカースレッド / 更新プログラムの画面 `updater/`)。tao / wry 依存はここに閉じ込める。状態と処理は Rust が持ち、UI へは画面状態 (`ipc/snapshot.rs`) を送り、UI からはコマンド (`ipc/command.rs`) を受ける
- `ui/` — UI 本体 (React + TypeScript、Vite)。`build.rs` が pnpm でビルドして実行ファイルに埋め込む。型は `ui/src/types.ts` を `src/app/ipc` と揃える
- `src/update/` — 更新確認・minisign 署名検証、更新プログラムの進行 (`flow`)・MSI の無人インストールとポータブル版の差し替え (`install`)
- `src/install_layout.rs` — MSI 版 / ポータブル版 (実行ファイルの隣の `portable`) ごとの設定・データの保存先
- `tests/` — 公開 API の統合テスト。画像生成ヘルパーは `tests/common/` に置き、`src/test_helpers.rs` が `include!` してユニットテストと共有する
- `scripts/windows/` — エントリポイント (PowerShell 7)。共通処理は `scripts/common/` に置く

## コマンド

```bash
cargo run                                 # デバッグ実行
cargo build --release                     # リリースビルド (target/release/EdgeCut.exe)
cargo fmt --all                           # 整形
cargo clippy --all-targets -- -D warnings # 静的解析 (警告はエラー扱い)
cargo test --all-targets                  # ユニット・統合テスト
cargo test --doc                          # doctest
cargo test <テスト名>                      # 単一テスト
pnpm --dir ui run typecheck               # UI の型検査
pnpm --dir ui run lint                    # UI の静的解析 (ESLint)
pnpm --dir ui test                        # UI のテスト (Vitest)
cargo deny check                          # 依存の脆弱性・ライセンス監査
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items # rustdoc のリンク切れ検出

# 完了前の品質チェック一式 (UI の型・静的解析・テスト・ライセンス・監査 / fmt / clippy / doc / 許可リスト同期 / deny / test / doctest)
./scripts/windows/check.ps1
```

- ツールチェーンは `rust-toolchain.toml` で 1.100 固定、edition 2024
- `clippy::pedantic` を warn で有効化し、`unsafe_code` は deny (理由付きの `#[expect(unsafe_code, reason = ...)]` で許可するのは `src/app/single_instance.rs` の `win32` モジュールだけ)。`clippy::undocumented_unsafe_blocks` も warn で有効化し、`unsafe` ブロックには `// SAFETY:` コメントを必須とする。`clippy::unwrap_used` / `clippy::expect_used` も warn で有効化し、本体コードでは `unwrap` / `expect` を使わない (テストは `clippy.toml` で対象外。`build.rs` は理由付きの `#![expect]` で許可)
- rustfmt の `max_width` は 100

## 実装時の注意

- 重い処理は UI スレッドで行わず、ワーカースレッドへ渡す。ワーカーの panic は `catch_panic` で捕捉して失敗として通知する
- 公開 API を追加・変更した場合は `src/lib.rs` の再エクスポートとモジュール表を更新する
- 対応 OS は Windows のみ。`cfg(windows)` などの OS 分岐は置かない
- 依存を追加する場合は `deny.toml` の許可ライセンス (パーミッシブのみ) に収まるか確認し、`about.toml` の許可リストも同じ内容に揃える (差分は `check.ps1` が検出する)。UI の `dependencies` (npm) も同じ許可リストで `check.ps1` が検証する
- UI と Rust の間でコマンド・画面状態の項目を変えたら、`src/app/ipc/` と `ui/src/types.ts` (更新プログラムは `src/app/updater/view.rs` と `ui/src/updater/types.ts`) の両方を直す (JSON の項目名は camelCase、enum の値は Rust の名前のまま)
- `build.rs` は Windows のアイコン・バージョン情報、`keys/update-signing/*.pub` (minisign 公開鍵)、UI のビルド結果を埋め込む (Node.js と pnpm が必要)
- `src/update/signature.rs` の信頼コメント形式は `scripts/common/update-signing.ps1` と揃える
- リリースは `scripts/windows/release.ps1` で行う ([docs/dev/release.md](docs/dev/release.md))。指示がない限り実行しない

## ドキュメント同期

- 利用者に影響する変更は `CHANGELOG.md` の `[Unreleased]` に同じ変更で追記する (スキル `/changelog`)
- 操作・表示が変わる変更では `docs/user/` の該当ページも更新する
- モジュールを追加・移動・分割したら `docs/dev/architecture.md` と上の「構成」を更新する

## Git

ブランチ・コミットメッセージ・CHANGELOG の規約は [docs/dev/contributing.md の Git 運用](docs/dev/contributing.md#git-運用) に従う。ここには作業時に間違えやすい点だけを置く。

- ブランチは `develop` を checkout / pull せず、`git fetch origin develop` のうえ `origin/develop` から切る (`git switch -c <prefix>/<kebab-case> origin/develop`)
- worktree の自動命名ブランチは、最初のコミットの前に `git branch -m` で規約の名前へ改める。新機能の prefix は `feat` ではなく `feature`
- コミットメッセージの種別は先頭大文字 (`Feat:` / `Fix:` / `Docs:` など)

## コーディング規約

規約は `.claude/rules/` (`comments.md` / `imports.md` / `docs.md`) に置き、対象ファイルを扱うときに自動で読み込まれる。品質チェックと CHANGELOG 追記はスキル `/check`・`/changelog` (`.claude/skills/`) で行う。共有の権限設定は `.claude/settings.json`、個人用の上書きは `.claude/settings.local.json` (Git 管理外) に置く。
