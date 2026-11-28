<div align="center">
  <br>
  <img src="assets/icons/icon.png" alt="EdgeCut" width="120" />
  <h1>EdgeCut</h1>
  <p><b>画像の背景余白を自動検出・トリミングするデスクトップ GUI ツール</b></p>
  <p>
    <a href="https://github.com/GnudMna/edge-cut/releases/latest"><img src="https://img.shields.io/github/v/release/GnudMna/edge-cut?label=release&color=2563EB&style=flat-square" alt="最新リリース"></a>
    <img src="https://img.shields.io/badge/platform-Windows_10%2B-0078D4?logo=windows&logoColor=white&style=flat-square" alt="platform: Windows 10+">
    <img src="https://img.shields.io/badge/runtime-WebView2-0F766E?logo=microsoftedge&logoColor=white&style=flat-square" alt="runtime: WebView2">
    <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Rust-1.100-CE422B?logo=rust&logoColor=white&style=flat-square" alt="Rust 1.100"></a>
    <a href="./LICENSE"><img src="https://img.shields.io/badge/license-MIT-F59E0B?style=flat-square" alt="license: MIT"></a>
  </p>
</div>

---

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
      <h3>💾</h3>
      <b>設定の保持</b><br><br>
      <sub>終了時の設定を<br>次回起動時に<br>自動復元</sub>
    </td>
  </tr>
</table>

<br>

---

## 主な機能

上の 4 点のほかに、次の機能がある。

| 機能 | 概要 |
| :--- | :--- |
| **自動余白除去** | 四辺から背景色を検出し、上下左右の余白を除去 ([動作の仕組み](docs/user/how-it-works.md)) |
| **トリミングの調整** | 余白内の小さなゴミの無視 (ノイズ除外)・除去する辺の選択・辺ごとの余白・アスペクト比の固定 |
| **プレビューの拡大** | 元画像とトリミング後を個別にズーム・パン |
| **入出力** | ファイル・ドラッグ & ドロップ・クリップボードから読み込み、保存・コピー。PNG / JPEG / WebP / BMP / TIFF / GIF に対応 |
| **自動保存** | 画像を開いた直後に指定フォルダへ保存 |
| **色と向きの保持** | ICC プロファイルと解像度 (DPI) を引き継ぎ、EXIF の回転情報を反映 |
| **アプリ更新** | 最新版を確認し、署名とハッシュを検証してから適用 |

操作の詳細は [基本操作](docs/user/usage.md) を参照。

---

## 動作要件とインストール

### 動作要件

| 項目 | 要件 |
| :--- | :--- |
| OS | Windows 10 以降 (Linux / macOS は非対応) |
| ランタイム | Microsoft Edge WebView2 ランタイム (Windows 11 には標準で入っている。Windows 10 で起動できない場合は Microsoft のサイトから導入する) |

### インストール

[最新のリリース](https://github.com/GnudMna/edge-cut/releases/latest) から、次のどちらかを入手する。

| 配布形態 | ファイル | 使い方 |
| :--- | :--- | :--- |
| MSI インストーラー | `edge-cut-{バージョン}-x86_64.msi` | 実行すると `%LOCALAPPDATA%\edge-cut` へユーザー単位でインストールされる (管理者権限は不要)。スタートメニューに **EdgeCut** と **EdgeCut ドキュメント** が追加され、ユーザー `PATH` への追加も選べる |
| ポータブル版 | `edge-cut-{バージョン}-x86_64-portable.zip` | インストール不要。展開したフォルダの `EdgeCut.exe` を起動する。設定などはそのフォルダの `data` に保存する |

どちらもメニューの「更新を確認…」で新しい版に更新できる ([アプリ更新](docs/user/usage.md#アプリ更新))。ソースからビルドする場合は [開発環境とビルド](docs/dev/setup.md) を参照。

---

## クイックスタート

1. EdgeCut を起動し、「開く」で画像を選ぶか、画像をウィンドウへドラッグ & ドロップする
2. 背景色が自動で検出され、元画像にトリミング範囲が重ねて表示される (「結果」「並べて」で表示を切り替えられる)
3. 右側の設定パネルにある「許容度」「余白」のスライダーで仕上がりを調整する
4. 「保存」(<kbd>Ctrl</kbd> + <kbd>S</kbd>) でファイルへ書き出すか、「コピー」(<kbd>Ctrl</kbd> + <kbd>C</kbd>) でクリップボードへコピーする

トリミングと背景色の細かい調整は [仕上がりの調整](docs/user/adjust.md)、自動保存・一括トリミングは [保存と一括処理](docs/user/output.md)、キーボードショートカットは [基本操作](docs/user/usage.md#キーボードショートカット) を参照。

---

## ドキュメント

### 利用者向け

| ドキュメント | 内容 |
| :--- | :--- |
| [基本操作](docs/user/usage.md) | 画面構成・基本的な流れ・プレビューの拡大・メニュー・キーボードショートカット・アプリ更新 |
| [仕上がりの調整](docs/user/adjust.md) | ノイズ除外・除去する辺・辺ごとの余白・アスペクト比・背景色の調整 |
| [保存と一括処理](docs/user/output.md) | 出力形式・出力ファイル名・自動保存・一括トリミング |
| [動作の仕組み](docs/user/how-it-works.md) | 背景色の検出・余白の判定・トリミングの流れ |
| [対応フォーマットと上限](docs/user/formats.md) | 読み込み・保存できる形式、ICC プロファイル・EXIF の扱い、読み込みサイズの上限 |
| [トラブルシューティング](docs/user/troubleshooting.md) | 症状別の確認事項 |
| [CHANGELOG.md](CHANGELOG.md) | バージョンごとの変更履歴 |

利用者向けドキュメントは、MSI とポータブル版に HTML で同梱している。

### 開発者向け

| ドキュメント | 内容 |
| :--- | :--- |
| [開発環境とビルド](docs/dev/setup.md) | 前提条件・ビルド・スクリプト・配布パッケージ作成・第三者ライセンス表示・利用者向けドキュメントの同梱 |
| [テストと品質チェック](docs/dev/testing.md) | `check` スクリプト・テスト構成・依存関係の監査 |
| [アーキテクチャ](docs/dev/architecture.md) | モジュールの責務・設計方針 |
| [リリース](docs/dev/release.md) | `release.ps1`・更新署名鍵 |
| [開発への参加](docs/dev/contributing.md) | Git 運用・コーディング規約・ドキュメントの配置 |

---

## ライセンス

[MIT License](LICENSE)
