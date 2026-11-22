# テストと品質チェック

## 品質チェック

`scripts/windows/check.ps1` で fmt / clippy / doc / 許可リスト同期 / deny / test / doctest を一括実行する。変更後はこれが通ることを確認する。cargo のコマンドは `--locked` 付きで実行し、`Cargo.lock` の更新漏れもエラーにする。

個別に実行する場合:

```bash
cargo fmt --all                           # 整形
cargo clippy --all-targets -- -D warnings # 静的解析 (警告はエラー扱い)
cargo deny check                          # 依存の脆弱性・ライセンス監査
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items # rustdoc のリンク切れ検出
cargo test --all-targets                  # ユニット・統合テスト
cargo test --doc                          # doctest
cargo test <テスト名>                      # 単一テスト
```

| スクリプト | 説明 |
| :--- | :--- |
| `windows/check.ps1` | 一括チェック (fmt / clippy / doc / 許可リスト同期 / deny / test / doctest) |
| `windows/test.ps1` | テスト実行 |
| `windows/clippy.ps1` | Clippy による静的解析 |
| `windows/format.ps1` | コードフォーマット |
| `windows/install-hooks.ps1` | push 前に `check.ps1` を実行する Git フック (`.githooks/pre-push`) を有効にする (`core.hooksPath` を設定) |
| `windows/uninstall-hooks.ps1` | `install-hooks.ps1` で有効にした Git フックを無効にする |

Git フックは任意で有効にする。一時的に省略する場合は `git push --no-verify` を使う。フックは push するコミットではなく作業ツリーの内容を検査する。

- `clippy::pedantic` を warn で有効化し、`unsafe_code` は deny (許可するのは `src/app/title_bar.rs` のみ)。`clippy::undocumented_unsafe_blocks` も warn で有効化し、`unsafe` ブロックには `// SAFETY:` コメントを必須とする
- rustfmt の `max_width` は 100
- doctest は Clippy の対象外のため、`src/lib.rs` の `#![doc(test(attr(deny(warnings))))]` で警告をエラーにする

## テスト構成

| 場所 | 内容 |
| :--- | :--- |
| 各モジュールの `#[cfg(test)]` | ユニットテスト |
| `tests/` | lib クレートの公開 API に対する統合テスト |
| `tests/malformed_image_test.rs` | 壊れた画像 (切り詰め・ランダム破損・寸法偽装・不正な ICC) を `open_image_with_icc` に通し、panic せず上限を守ることを確認する |
| `src/update/tests.rs` | 更新フローの結合テスト。ローカルの HTTP サーバーを bridge の代わりに立て、version API から SHA-256・minisign 署名の検証までを通す (`msiexec` は起動しない) |
| `tests/common/` | 画像生成ヘルパー。`src/test_helpers.rs` が `include!` してユニットテストでも共有する |

## 依存関係の監査

[`deny.toml`](../../deny.toml) で依存関係の脆弱性・ライセンス・取得元を検証する。Windows (`x86_64-pc-windows-msvc`) のみを対象にする。

依存を追加する場合は `deny.toml` の許可ライセンス (パーミッシブのみ) に収まるか確認し、`about.toml` の許可リストも同じ内容に揃える。許可ライセンスとターゲットの差分は `scripts/common/license-allowlist-check.ps1` が検出し、`check.ps1` から呼び出される。
