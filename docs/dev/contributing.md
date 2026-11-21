# 開発への参加

ブランチ・コミットの規約、コーディング規約、ドキュメントの配置ルールをまとめる。

## Git 運用

- git-flow 形式。`develop` から `feature/*` / `fix/*` / `docs/*` などを切り、`main` へは `release/vX.Y.Z` 経由でマージする
- コミットメッセージは `<種別>: <日本語の要約>`。種別は `Feat` / `Update` / `Fix` / `Refactor` / `Perf` / `Test` / `Docs` / `Style` / `Chore` / `Release` / `Add`
- 利用者に影響する変更は [CHANGELOG.md](../../CHANGELOG.md) の `[Unreleased]` に同じ変更で追記する。書き方は CHANGELOG.md 冒頭の規約に従う (1 項目 1 文・体言止め・症状と変更後の挙動を書く。テスト・リファクタリング・ドキュメント整理など利用者に見えない変更は書かない)
- Pull Request 前に [品質チェック](testing.md#品質チェック) を通す

## コーディング規約

規約は `.claude/rules/` に置く (Claude Code では対象ファイルを扱うときに自動で読み込まれる)。

| ファイル | 対象 | 内容 |
| :--- | :--- | :--- |
| `comments.md` | `*.rs` | コメントの文体 (常体・句点・半角括弧)、セクション区切り、`///` の構成 (`pub fn` は必須)、テストの記述 (`〜すること`) |
| `imports.md` | `*.rs` | `use` 文のグループ順 (`std` → 外部クレート → `super` / `crate`)・空行・アルファベット順 |
| `docs.md` | ドキュメント (`docs/`・`README.md`・`CHANGELOG.md` など) | 文体・文書ごとの書き分け |

Claude Code 向けには、品質チェック (`/check`) と CHANGELOG 追記 (`/changelog`) のスキルを `.claude/skills/` に、共有の権限設定を `.claude/settings.json` に置く。個人用の上書きは `.claude/settings.local.json` (Git 管理外) に置く。

## ドキュメントの配置

ドキュメントは読み手ごとに分け、同じ説明は 1 か所にだけ書く。ほかの文書からはリンクで参照する。

| 置き場所 | 読み手 | 書くこと |
| :--- | :--- | :--- |
| `README.md` | 初めて使う人 | 概要・主な機能・動作要件とインストール・クイックスタート・ドキュメント索引 |
| `docs/user/introduction.md` | 利用者 | 同梱ドキュメントの表紙 (概要・はじめて使う手順・ページ一覧) |
| `docs/user/usage.md` | 利用者 | 操作方法 (背景色・自動保存・一括トリミング・出力形式・ショートカット・更新) |
| `docs/user/how-it-works.md` | 利用者 | 背景色の検出・余白の判定・トリミングの仕組み |
| `docs/user/formats.md` | 利用者 | 対応形式・保存時の変換・ICC / EXIF の扱い・読み込み上限 |
| `docs/user/troubleshooting.md` | 利用者 | 症状別の確認事項 |
| `docs/dev/` | 開発者 | 環境構築・テスト・アーキテクチャ・リリース・開発規約 |
| `CHANGELOG.md` | 利用者・開発者 | バージョンごとの変更 (`release.ps1` が `[Unreleased]` 節を読むため移動しない) |
| `CLAUDE.md` | Claude Code | コマンド・構成の要約・間違えやすい点・規約への参照 |

- 操作・表示が変わる変更では、該当する `docs/user/` のページも同じ変更で更新する
- `docs/user/` は配布物に HTML で同梱するため、開発者向けの記述 (内部名・実装詳細・ビルド手順) を書かない。相対リンクは `docs/user/` 内のページに限る (外を指す相対リンクはリンク検証で失敗する)
