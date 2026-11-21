# テストと品質チェック

## 品質チェック

`scripts/windows/check.ps1` で fmt / clippy / deny / test / doctest を一括実行する。変更後はこれが通ることを確認する。

個別に実行する場合:

```bash
cargo fmt --all                           # 整形
cargo clippy --all-targets -- -D warnings # 静的解析 (警告はエラー扱い)
cargo deny check                          # 依存の脆弱性・ライセンス監査
cargo test --all-targets                  # ユニット・統合テスト
cargo test --doc                          # doctest
cargo test <テスト名>                      # 単一テスト
```

| スクリプト | 説明 |
| :--- | :--- |
| `windows/check.ps1` | 一括チェック (fmt / clippy / deny / test / doc) |
| `windows/test.ps1` | テスト実行 |
| `windows/clippy.ps1` | Clippy による静的解析 |
| `windows/format.ps1` | コードフォーマット |

- `clippy::pedantic` を warn で有効化し、`unsafe_code` は deny (許可するのは `src/app/title_bar.rs` のみ)
- rustfmt の `max_width` は 100

## テスト構成

| 場所 | 内容 |
| :--- | :--- |
| 各モジュールの `#[cfg(test)]` | ユニットテスト |
| `tests/` | lib クレートの公開 API に対する統合テスト |
| `tests/common/` | 画像生成ヘルパー。`src/test_helpers.rs` が `include!` してユニットテストでも共有する |

## 依存関係の監査

[`deny.toml`](../../deny.toml) で依存関係の脆弱性・ライセンス・取得元を検証する。Windows (`x86_64-pc-windows-msvc`) のみを対象にする。

依存を追加する場合は `deny.toml` の許可ライセンス (パーミッシブのみ) に収まるか確認し、`about.toml` の許可リストも同じ内容に揃える。
