---
name: check
description: 品質チェック一式 (scripts/windows/check.ps1) を実行し、失敗した項目を直して通るまで繰り返す。作業の完了前・コミットや PR の前に品質チェックを通すとき、または「check して」「品質チェックを通して」と頼まれたときに使う
---

# 品質チェック

`./scripts/windows/check.ps1` を実行し、失敗を直して全項目を通す。チェック内容は [docs/dev/testing.md の品質チェック](../../../docs/dev/testing.md#品質チェック) に従う。

## 実行

- PowerShell ツール (PowerShell 7) で、リポジトリのルートから実行する。数分かかるため `timeout` は 600000 にする

```powershell
./scripts/windows/check.ps1
```

## 失敗したとき

`check.ps1` は最初の失敗で止まる。出力から失敗した項目を特定し、次のとおり直してから、その項目だけを再実行して確認する。通ったら `check.ps1` 全体を再実行する。

| 失敗した項目 | 直し方 | 項目だけの確認 |
| :--- | :--- | :--- |
| コード整形 | `cargo fmt --all` | `cargo fmt --check` |
| Clippy | 警告の原因をコードで直す | `cargo clippy --all-targets -- -D warnings` |
| サプライチェーン監査 | 直さずにユーザーへ報告する (依存・`deny.toml` の変更は判断が要る) | `cargo deny check` |
| テスト | 失敗の原因を直す | `cargo test <テスト名>` / `cargo test --test <名前>` で絞る |
| doctest | 失敗の原因を直す | `cargo test --doc` |

次はしない。

- `#[allow(...)]`・`#[ignore]` などで警告やテストを黙らせる。必要だと判断したら、作業を止めてユーザーに確認する
- テストの期待値を、原因を確かめないまま実装に合わせて書き換える
- 同じ失敗を 3 回直しても通らないまま続ける。その時点で状況を報告して判断を仰ぐ

## 報告

最後に次を簡潔に報告する。

- 結果 (全項目成功 / 失敗した項目とエラー)
- 直した内容 (ファイルと理由)
- 未解決の項目と理由
