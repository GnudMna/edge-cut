<div align="center">
  <br>
  <img src="assets/icons/icon.png" alt="EdgeCut" width="120" />
  <h1>✂️ EdgeCut</h1>
  <p><b>画像の背景余白を自動検出・トリミングするデスクトップ GUI ツール</b></p>
  <p>
    <sub>Rust 製 &nbsp;·&nbsp; egui によるリアルタイムプレビュー &nbsp;·&nbsp; Windows / Linux / macOS</sub>
  </p>
  <br>
  <a href="https://www.rust-lang.org/">
    <img src="https://img.shields.io/badge/Rust-2024_edition-CE422B?logo=rust&logoColor=white&style=for-the-badge" alt="Rust">
  </a>
  <a href="./LICENSE">
    <img src="https://img.shields.io/badge/License-MIT-4A90D9?style=for-the-badge" alt="MIT License">
  </a>
  <img src="https://img.shields.io/badge/Windows-0078D4?logo=windows&logoColor=white&style=for-the-badge" alt="Windows">
  <img src="https://img.shields.io/badge/Linux-FCC624?logo=linux&logoColor=black&style=for-the-badge" alt="Linux">
  <img src="https://img.shields.io/badge/macOS-000000?logo=apple&logoColor=white&style=for-the-badge" alt="macOS">
  <a href="https://github.com/emilk/egui">
    <img src="https://img.shields.io/badge/GUI-egui%20%2F%20eframe-7C3AED?style=for-the-badge" alt="egui">
  </a>
  <br><br>
</div>

---

<br>

<table align="center">
  <tr>
    <td align="center" width="210">
      <h3>⚡</h3>
      <b>リアルタイムプレビュー</b><br><br>
      <sub>スライダー操作に即時追従。<br>元画像とトリミング後を<br>左右に並べて比較</sub>
    </td>
    <td align="center" width="210">
      <h3>🎨</h3>
      <b>柔軟な背景色指定</b><br><br>
      <sub>自動検出・カラーピッカー<br>スポイト・手動 RGBA 編集</sub>
    </td>
    <td align="center" width="210">
      <h3>📦</h3>
      <b>一括処理</b><br><br>
      <sub>複数ファイルをバックグラウンドで<br>まとめてトリミング。<br>UI をブロックしない</sub>
    </td>
    <td align="center" width="210">
      <h3>🌍</h3>
      <b>クロスプラットフォーム</b><br><br>
      <sub>Windows / Linux / macOS。<br>設定は次回起動時に<br>自動復元</sub>
    </td>
  </tr>
</table>

<br>

---

## ドキュメント

### 利用者向け

| ドキュメント | 内容 |
| :--- | :--- |
| [使い方](docs/user/usage.md) | 基本操作・背景色の調整・自動保存・一括トリミング・出力形式・キーボードショートカット・アプリ更新 |
| [動作の仕組み](docs/user/how-it-works.md) | 背景色の検出・余白の判定・トリミングの流れ |
| [対応フォーマットと上限](docs/user/formats.md) | 読み込み・保存できる形式、ICC プロファイル・EXIF の扱い、読み込みサイズの上限 |
| [トラブルシューティング](docs/user/troubleshooting.md) | 症状別の確認事項 |
| [CHANGELOG.md](CHANGELOG.md) | バージョンごとの変更履歴 |

利用者向けドキュメントは、配布物 (MSI / tar.gz / .app・DMG) に HTML で同梱している。

### 開発者向け

| ドキュメント | 内容 |
| :--- | :--- |
| [開発環境とビルド](docs/dev/setup.md) | 前提条件・ビルド・スクリプト・配布パッケージ作成・第三者ライセンス表示・利用者向けドキュメントの同梱 |
| [テストと品質チェック](docs/dev/testing.md) | `check` スクリプト・テスト構成・依存関係の監査 |
| [アーキテクチャ](docs/dev/architecture.md) | モジュールの責務・設計方針 |
| [リリース](docs/dev/release.md) | `release.ps1`・更新署名鍵 |
| [開発への参加](docs/dev/contributing.md) | Git 運用・コーディング規約・ドキュメントの配置 |

---

## 主な機能

| 機能 | 概要 |
| :--- | :--- |
| **自動余白除去** | 四辺から背景色を検出し、上下左右の余白を除去 ([動作の仕組み](docs/user/how-it-works.md)) |
| **許容度・余白の調整** | スライダー操作に即時追従するプレビューで、背景とみなす範囲と追加する余白を調整 |
| **背景色の指定** | 自動検出 / カラーピッカー / スポイト / 手動 RGBA 編集 |
| **入出力** | ファイル・ドラッグ & ドロップ・クリップボードから読み込み、保存・コピー。PNG / JPEG / WebP / BMP / TIFF / GIF に対応 |
| **自動保存** | 画像を開いた直後に指定フォルダへ保存 |
| **一括トリミング** | 複数ファイル・フォルダをバックグラウンドで並列処理 |
| **色と向きの保持** | ICC プロファイルを引き継ぎ、EXIF の回転情報を反映 |
| **アプリ更新 (Windows)** | 最新版を確認し、署名とハッシュを検証してから適用 |

操作の詳細は [使い方](docs/user/usage.md) を参照。

---

## 動作要件とインストール

- **対応 OS:** Windows 10 以降 / Linux (X11 または Wayland) / macOS
- **Linux の日本語表示:** 日本語フォントが必要 ([トラブルシューティング](docs/user/troubleshooting.md#日本語が表示されない-linux))

| 形態 | 入手・配置 |
| :--- | :--- |
| **Windows MSI** | ユーザー単位で `%LOCALAPPDATA%\edge-cut` へインストール。管理者権限は不要。スタートメニューに **EdgeCut** と **EdgeCut ドキュメント** が追加される。インストーラーではユーザー `PATH` への追加も選択できる |
| **Linux tar.gz** | 展開して `./install.sh` を実行すると `~/.local` にインストールされ、アプリケーション一覧と `edge-cut` コマンドから起動できる。ドキュメントは `~/.local/share/edge-cut/docs/index.html` |
| **macOS DMG** | `EdgeCut.app` をアプリケーションフォルダへコピーする。ドキュメントは DMG 直下の `docs/index.html` |

ソースからビルドする場合は [開発環境とビルド](docs/dev/setup.md) を参照。

---

## クイックスタート

1. EdgeCut を起動し、「📂 開く」で画像を選ぶか、画像をウィンドウへドラッグ & ドロップする
2. 背景色が自動で検出され、元画像とトリミング後が左右に並んで表示される
3. 「許容度」「余白」のスライダーで仕上がりを調整する
4. 「💾 保存」(<kbd>Ctrl</kbd> + <kbd>S</kbd>) でファイルへ書き出すか、「📋 コピー」(<kbd>Ctrl</kbd> + <kbd>C</kbd>) でクリップボードへコピーする

背景色の調整・自動保存・一括トリミング・キーボードショートカットは [使い方](docs/user/usage.md) を参照。

---

## ライセンス

[MIT](LICENSE)
