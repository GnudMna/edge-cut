---
paths:
  - "docs/**/*.md"
  - "README.md"
  - "CHANGELOG.md"
  - "CLAUDE.md"
  - ".claude/rules/*.md"
  - ".claude/skills/**/*.md"
---

# ドキュメント規約

どの文書に何を書くかは [docs/dev/contributing.md](../../docs/dev/contributing.md#ドキュメントの配置) に従う。同じ説明は 1 か所にだけ書き、ほかの文書からはリンクで参照する。

## 文体

- 常体 (だ・である調)。です・ます調にしない
- 括弧・コロンは半角を使う
- 表の区切りは `| :--- | :--- |` の形式にそろえる

## 文書ごとの規約

- `docs/user/` (利用者向け・mdBook 原稿): 配布物に HTML で同梱するため、開発者向けの記述 (内部名・実装詳細・ビルド手順) を書かず、`docs/user/` の外へ相対リンクしない。ページを足したら `docs/user/SUMMARY.md` にも追加する
- `docs/dev/architecture.md`: モジュールの責務と設計方針を書く。モジュールを追加・移動・分割したら同じ変更で更新する
- `CHANGELOG.md`: 書き方は冒頭の規約に従う。`release.ps1` が `[Unreleased]` 節を読むため、見出し構造を崩さず、既存のバージョン節は編集しない
- `CLAUDE.md`: 毎回読み込まれるため、作業時に間違えやすい点と参照先だけを置き、詳細は `docs/dev/` へのリンクにする
