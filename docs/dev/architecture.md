# アーキテクチャ

Rust 2024 edition、`eframe` / `egui` によるネイティブ GUI アプリ。lib クレート `edge_cut` (`src/lib.rs`) にコアロジックと GUI を置き、bin クレート `EdgeCut` (`src/main.rs`) は `edge_cut::app::run()` を呼ぶだけのエントリポイントとする。

## モジュール構成

| パス | 責務 |
| :--- | :--- |
| `src/trim.rs` | 背景色検出 (四辺を量子化して近傍で集計し、最多クラスタ内の最頻 RGBA)・余白除去のコアロジック |
| `src/image_load.rs` | サイズ上限付きの安全な画像読み込み (EXIF Orientation の反映・ICC プロファイルの取得を含む) |
| `src/image_output.rs` | 出力形式 (`OutputFormat`) の決定・画像保存 (ICC プロファイルの再埋め込み)・出力パス生成 (衝突時は連番サフィックス) |
| `src/batch_trim.rs` | 複数ファイルの一括トリミング (進捗通知・キャンセル) |
| `src/panic_hook.rs` | panic フック。panic の内容を `%LOCALAPPDATA%\EdgeCut\panic.log` (`LOCALAPPDATA` 未設定時は一時ディレクトリ配下) へ追記し、UI スレッドの panic ではダイアログも表示 |
| `src/panic_message.rs` | `catch_panic` で処理中の panic を捕捉してメッセージ化 (ワーカーの panic を失敗として扱う) |
| `src/consts.rs` | 上限値・対応拡張子・ウィンドウサイズなどの定数 |
| `src/app/` | egui GUI (下表) |
| `src/update/` | gh-release-bridge 経由の更新確認・minisign 署名検証・MSI 適用 |
| `src/test_helpers.rs` | `#[cfg(test)]` 限定。`tests/common/images.rs` を `include!` してユニットテストと画像生成ヘルパーを共有 |
| `tests/` | lib クレートの公開 API に対する統合テスト (`tests/common/` に画像生成ヘルパー) |
| `docs/user/` | 利用者向けドキュメント (mdBook の原稿)。`scripts/common/build-user-docs.ps1` が HTML 化して MSI の `docs\` に同梱する |

`src/app/` の構成:

| パス | 責務 |
| :--- | :--- |
| `state` | アプリの状態 |
| `bundled` | 配布物に同梱した文書 (利用者向けドキュメント・LICENSE・THIRD_PARTY_NOTICES) の探索と、Windows の既定のアプリでの起動。同梱先は実行ファイルの位置から MSI の配布レイアウトに従って求める |
| `view/` | 描画。`toolbar` / `settings` / `preview` / `input` / `batch_errors` / `update_dialog` に分割 |
| `actions/` | 操作。`dialog` / `load` / `io_result` / `trim` / `save` / `clipboard` / `batch` / `drop` / `app_update` に責務ごとに分割 |
| `trim_worker` / `clipboard_worker` / `load_worker` | プレビュー用・コピーと貼り付け用・画像読み込み用の常駐ワーカースレッド |
| `save_dir` | 自動保存先の既定パスの決定 (Windows の既知のフォルダ) |
| `shortcuts` | キーボードショートカットの判定 |
| `image_convert` | プレビュー用テクスチャ変換 |
| `fonts/` / `icon` | フォント (`loader` / `registry`)・アイコンの読み込み |
| `title_bar` | タイトルバーの色をテーマに合わせる (`unsafe_code` を許可する唯一のモジュール) |

## 設計方針

- コアロジック (`trim` / `image_load` / `image_output` / `batch_trim`) は egui に依存させない。egui 依存は `src/app/` に閉じ込める
- 重い処理は UI スレッドで行わず、ワーカースレッドへ渡す。プレビュー・クリップボード・読み込みは常駐ワーカー、ファイルダイアログ・保存・一括トリミング・ドロップされたフォルダの展開・更新確認はその都度起こすスレッドで行い、結果はチャネル経由で UI スレッドへ返す。ワーカーの panic は `catch_panic` で捕捉して失敗として通知する
- ウィンドウを閉じる要求があったフレームでウィンドウを非表示にし (`view` の `hide_window_on_close`)、保存スレッドと一括トリミングは `IoState` / `BatchState` の `Drop` で完了を待ってからプロセスを終える (一括トリミングは未着手のファイルをキャンセルする)
- 公開 API を追加・変更した場合は `src/lib.rs` の再エクスポートとモジュール表を更新する。ライブラリとしての使い方は `src/lib.rs` の doc コメント (`cargo doc --open`) を参照
- 対応 OS は Windows のみ。`cfg(windows)` などの OS 分岐は置かず、Windows 専用 API を直接使う

背景色検出・余白判定のアルゴリズム (利用者向けの説明) は [動作の仕組み](../user/how-it-works.md) を参照。
