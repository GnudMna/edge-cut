<div align="center">
  <img src="assets/icons/icon.png" alt="EdgeCut Logo" width="128" />
  <h1>EdgeCut</h1>
  <p>
    画像の背景余白を自動検出・トリミングする GUI ツール
  </p>

  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Rust-2024_edition-orange?logo=rust" alt="Rust"></a>
  <a href="./LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue" alt="License"></a>
  <a href="https://www.microsoft.com/windows"><img src="https://img.shields.io/badge/platform-Windows-lightblue?logo=windows" alt="Platform"></a>
  <a href="https://github.com/emilk/egui"><img src="https://img.shields.io/badge/GUI-egui%20%2F%20eframe-purple" alt="GUI"></a>
</div>

---

## 概要

EdgeCut は、画像四隅の色を基に背景色を自動検出し、余白を一括でトリミングする Windows 向けデスクトップアプリケーションです。  
許容度・余白ピクセル调整に対応しており、PNG・JPEG などの主要フォーマットをサポートします。

## 機能

| 機能 | 説明 |
|---|---|
| 自動余白トリミング | 四隅の最頻色を背景として検出し、上下左右の余白を除去 |
| 許容度調整 | 0〜100 の範囲で背景色との類似度閾値を設定 |
| 余白追加 | トリミング後に任意のピクセル数を四辺に追加 |
| ドラッグ & ドロップ | ファイルをウィンドウにドロップして読み込み |
| クリップボード貼り付け | クリップボードの画像を直接読み込み |
| 自動保存 | 指定フォルダへ自動でトリミング結果を保存 |
| ダーク / ライトモード | テーマをワンクリックで切り替え |
| コンパクト UI | トリミング後の画像のみを表示するコンパクト表示 |

## 対応フォーマット

PNG / JPEG / BMP / GIF / TIFF / WebP

## ビルド

Rust ツールチェーン (`cargo`) が必要です。

```bash
# デバッグビルド & 実行
cargo run

# リリースビルド
cargo build --release
```

ビルド成果物は `target/release/EdgeCut.exe` に生成されます。

## 使い方

1. **画像を開く** — ツールバーの「📂 開く」ボタンでファイルを選択、またはウィンドウへドラッグ & ドロップ
2. **許容度を調整** — スライダーで背景色の許容範囲を設定（0: 完全一致、100: 最大範囲）
3. **余白を調整** — トリミング後に残したい余白ピクセル数を設定
4. **保存** — 「💾 保存」ボタンで出力先を指定して保存

## ライセンス

[MIT](LICENSE)
