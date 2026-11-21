# アーキテクチャ

Rust 2024 edition、`eframe` / `egui` によるネイティブ GUI アプリ。lib クレート `edge_cut` (`src/lib.rs`) にコアロジックと GUI を置き、bin クレート `EdgeCut` (`src/main.rs`) は `edge_cut::app::run()` を呼ぶだけのエントリポイントとする。

## モジュール構成

| パス | 責務 |
| ---- | ---- |
| `src/trim.rs` | 背景色検出 (量子化クラスタの最頻 RGBA)・余白除去のコアロジック |
| `src/image_load.rs` | サイズ上限付きの安全な画像読み込み (EXIF Orientation の反映・ICC プロファイルの取得を含む) |
| `src/image_output.rs` | 出力形式の決定・画像保存 (ICC プロファイルの再埋め込み)・出力パス生成 (衝突時は連番サフィックス) |
| `src/batch_trim.rs` | 複数ファイルの一括トリミング (進捗通知・キャンセル) |
| `src/panic_message.rs` | ワーカーで捕捉した panic のメッセージ化 |
| `src/consts.rs` | 上限値・対応拡張子・ウィンドウサイズなどの定数 |
| `src/app/` | egui GUI。状態管理・描画・操作・キーボードショートカット・プレビュー用 / クリップボード用ワーカースレッド |
| `src/update/` | gh-release-bridge 経由の更新確認・署名検証・MSI 適用 (Windows のみ) |

## 設計方針

- コアロジック (`trim` / `image_load` / `image_output` / `batch_trim`) は egui に依存させない。egui 依存は `src/app/` に閉じ込める
- 重い処理は UI スレッドで行わず、ワーカースレッドへ渡す
- 公開 API を追加・変更した場合は `src/lib.rs` の再エクスポートとモジュール表を更新する。ライブラリとしての使い方は `src/lib.rs` の doc コメント (`cargo doc --open`) を参照
- Windows 専用依存は `cfg(windows)` の中でのみ使う

背景色検出・余白判定のアルゴリズム (利用者向けの説明) は [動作の仕組み](../user/how-it-works.md) を参照。
