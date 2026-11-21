# アーキテクチャ

Rust 2024 edition、`eframe` / `egui` によるネイティブ GUI アプリ。lib クレート `edge_cut` (`src/lib.rs`) にコアロジックと GUI を置き、bin クレート `EdgeCut` (`src/main.rs`) は `edge_cut::app::run()` を呼ぶだけのエントリポイントとする。

## モジュール構成

| パス | 責務 |
| :--- | :--- |
| `src/trim.rs` | 背景色検出 (四辺を量子化して近傍で集計し、最多クラスタ内の最頻 RGBA)・余白除去のコアロジック |
| `src/image_load.rs` | サイズ上限付きの安全な画像読み込み (EXIF Orientation の反映・ICC プロファイルの取得を含む) |
| `src/image_output.rs` | 出力形式 (`OutputFormat`) の決定・画像保存 (ICC プロファイルの再埋め込み)・出力パス生成 (衝突時は連番サフィックス) |
| `src/batch_trim.rs` | 複数ファイルの一括トリミング (進捗通知・キャンセル) |
| `src/panic_hook.rs` | panic フック。panic の内容を `%LOCALAPPDATA%\EdgeCut\panic.log` (Windows 以外は一時ディレクトリ配下) へ追記し、UI スレッドの panic ではダイアログも表示 |
| `src/panic_message.rs` | `catch_panic` で処理中の panic を捕捉してメッセージ化 (ワーカーの panic を失敗として扱う) |
| `src/consts.rs` | 上限値・対応拡張子・ウィンドウサイズなどの定数 |
| `src/app/` | egui GUI (下表) |
| `src/update/` | gh-release-bridge 経由の更新確認・minisign 署名検証・MSI 適用 (`#[cfg(windows)]` 限定) |
| `src/test_helpers.rs` | `#[cfg(test)]` 限定。`tests/common/images.rs` を `include!` してユニットテストと画像生成ヘルパーを共有 |
| `tests/` | lib クレートの公開 API に対する統合テスト (`tests/common/` に画像生成ヘルパー) |
| `docs/user/` | 利用者向けドキュメント (mdBook の原稿)。`scripts/common/build-user-docs.{ps1,sh}` が HTML 化して配布物の `docs/` に同梱する |

`src/app/` の構成:

| パス | 責務 |
| :--- | :--- |
| `state` | アプリの状態 |
| `view/` | 描画。`toolbar` / `settings` / `preview` / `input` / `batch_errors` / `update_dialog` に分割 |
| `actions/` | 操作。`dialog` / `load` / `io_result` / `trim` / `save` / `clipboard` / `batch` / `drop` / `app_update` に責務ごとに分割 |
| `trim_worker` / `clipboard_worker` / `load_worker` | プレビュー用・コピーと貼り付け用・画像読み込み用のワーカースレッド |
| `save_dir` | 自動保存先の既定パスの決定 (Windows の既知のフォルダ・Linux の XDG ユーザーディレクトリ) |
| `shortcuts` | キーボードショートカットの判定 |
| `image_convert` | プレビュー用テクスチャ変換 |
| `fonts/` / `icon` | フォント (`loader` / `registry`)・アイコンの読み込み |
| `title_bar` | タイトルバーの色をテーマに合わせる (`#[cfg(windows)]` 限定。`unsafe_code` を許可する唯一のモジュール) |

## 設計方針

- コアロジック (`trim` / `image_load` / `image_output` / `batch_trim`) は egui に依存させない。egui 依存は `src/app/` に閉じ込める
- 重い処理は UI スレッドで行わず、ワーカースレッドへ渡す
- 公開 API を追加・変更した場合は `src/lib.rs` の再エクスポートとモジュール表を更新する。ライブラリとしての使い方は `src/lib.rs` の doc コメント (`cargo doc --open`) を参照
- Windows 専用依存は `cfg(windows)` の中でのみ使う

背景色検出・余白判定のアルゴリズム (利用者向けの説明) は [動作の仕組み](../user/how-it-works.md) を参照。
