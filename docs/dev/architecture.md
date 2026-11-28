# アーキテクチャ

Rust 2024 edition のデスクトップ GUI アプリ。ウィンドウは `tao`、表示は `wry` (`WebView2`) で、UI は `ui/` の React + TypeScript で書く。状態と処理はすべて Rust 側が持ち、UI は Rust から届く画面状態を描画してコマンドを送り返すだけにする。lib クレート `edge_cut` (`src/lib.rs`) にコアロジックと GUI を置き、bin クレートは本体の `EdgeCut` (`src/main.rs`) と更新プログラムの `EdgeCutUpdater` (`src/bin/updater.rs`) の 2 つで、それぞれ `edge_cut::app::run()` / `run_updater()` を呼ぶだけのエントリポイントとする。

## モジュール構成

| パス | 責務 |
| :--- | :--- |
| `src/trim/` | 背景色検出・余白除去のコアロジック (下表) |
| `src/image_load.rs` | サイズ上限付きの安全な画像読み込み (EXIF Orientation の反映・ICC プロファイルと解像度の取得・複数フレームの検出を含む) |
| `src/dpi.rs` | 解像度 (DPI) の型。PNG `pHYs` / JPEG JFIF・EXIF / TIFF 解像度タグからの読み取りと、PNG / TIFF のエンコード済みバイト列への書き込み (JPEG は `image` のエンコーダ経由) |
| `src/image_output/` | 出力形式の決定・画像保存・出力パス生成 (下表) |
| `src/batch_trim/` | 複数ファイルの一括トリミング (下表) |
| `src/panic_hook.rs` | panic フック。panic の内容を `%LOCALAPPDATA%\EdgeCut\panic.log` (ポータブル版は `<実行フォルダ>\data\panic.log`。保存先を決められない場合は一時ディレクトリ配下) へ追記し、256 KiB を超える場合は `panic.log.1` へ退避 (1 世代のみ)、UI スレッドの panic ではダイアログも表示 |
| `src/panic_message.rs` | `catch_panic` で処理中の panic を捕捉してメッセージ化 (ワーカーの panic を失敗として扱う) |
| `src/consts.rs` | 上限値・対応拡張子・ウィンドウサイズなどの定数 |
| `src/app/` | GUI のホスト側 (ウィンドウ・`WebView`・状態・操作・ワーカー。下表) |
| `ui/` | UI 本体 (React + TypeScript、Vite でビルド。下表) |
| `src/update/` | GitHub Releases による更新確認・minisign 署名検証と、更新プログラムの進行・MSI の無人インストール・ポータブル版の差し替え (下表) |
| `src/install_layout.rs` | 配布形態 (MSI / ポータブル) ごとの実行ファイル名と、設定・`WebView2`・ログの保存先。実行ファイルの隣に `portable` があればポータブル版として実行フォルダの `data\` を使う |
| `src/bin/updater.rs` | 更新プログラム `EdgeCutUpdater.exe` のエントリポイント (`app::run_updater` を呼ぶだけ) |
| `src/test_helpers.rs` | `#[cfg(test)]` 限定。`tests/common/images.rs` を `include!` してユニットテストと画像生成ヘルパーを共有 |
| `tests/` | lib クレートの公開 API に対する統合テスト (`tests/common/` に画像生成ヘルパー) |
| `docs/user/` | 利用者向けドキュメント (mdBook の原稿)。`scripts/common/build-user-docs.ps1` が HTML 化して MSI とポータブル版の `docs\` に同梱する |

`src/trim/` の構成:

| パス | 責務 |
| :--- | :--- |
| `background` | 背景色検出。四辺を量子化して近傍で集計し、最多クラスタ内の最頻 RGBA を選ぶ |
| `detect` | 余白の矩形検出。色形式ごとに生バッファから画素を読み、背景以外の画素が `ignore_pixels` (ノイズ除外) を超える行・列から最小矩形を求め、辺ごとの余白・除去する辺・アスペクト比を適用して `TrimInfo` にする |
| `aspect` | アスペクト比の固定。範囲外を含む矩形 (`Rect`) を、中央を基準に (除去しない辺は動かさず) 指定の比率へ広げる・切り詰める。適用できない場合は `AspectSkip` で理由を返す |
| `options` | トリミングの設定 (`TrimOptions` / `Margins` / `TrimSides` / `AspectRatio` / `AspectFit`)。プレビューの再計算を省く判定のキーも兼ねる |
| `crop` | `TrimInfo` に従った切り出しと、範囲外へ拡張する余白の追加 |
| `types` | `TrimInfo` / `TrimResult` / `TrimBounds` / `AspectSkip` |
| `mod` | 公開 API の再エクスポートと `trim` / `trim_with_options` / `sample_pixel` |

`src/image_output/` の構成:

| パス | 責務 |
| :--- | :--- |
| `format` | 出力形式 (`OutputFormat`) |
| `save` | 保存オプション (`SaveOptions`)・上書き保存・連番保存 (衝突時は連番サフィックス) |
| `encode` | 形式に合わせた変換 (JPEG の透過合成など)・エンコード・ICC プロファイルの再埋め込み |
| `webp_lossy` | ロッシー WebP のエンコード (libwebp)・ICC プロファイルの `ICCP` チャンク挿入 |
| `temp_file` | 同じディレクトリへ書いてから置き換える一時ファイル |
| `path` | 出力ファイル名・パスの生成 |
| `error` | `SaveError` |

`src/batch_trim/` の構成:

| パス | 責務 |
| :--- | :--- |
| `runner` | 一括トリミングの実行 (並列処理・キャンセル・集計) |
| `memory_budget` | 並列処理するファイル間で共有するメモリ予算 |
| `result` | 設定 (`BatchTrimParams`)・進捗 (`BatchProgress`)・集計結果 (`BatchTrimResult`) |
| `error` | `BatchFileError` |

`src/app/` の構成:

| パス | 責務 |
| :--- | :--- |
| `state/` | アプリの状態。`App` 本体と、責務ごとの部分構造体 (`settings` / `image_state` / `trim_state` / `io_state` / `batch_state` / `update_state`) |
| `event_loop` | ウィンドウ (枠なし) と `WebView` の生成、イベントループ。UI からのコマンド・ドラッグ & ドロップ・ワーカーの完了通知を受けて `App` へ反映し、画面状態を UI へ送る。終了時のウィンドウ位置・設定の保存も行う |
| `ipc/` | UI とのやり取り。`command` (UI からのコマンドの定義と `App` への反映) / `snapshot` (UI へ送る画面状態) |
| `updater` | 更新プログラムの画面。tao の小さなウィンドウに同じ UI の `?view=updater` を表示し、`update::flow` を別スレッドで動かす (`bridge` でページと利用者の操作を受け渡し、`view` が UI との JSON) |
| `protocol` | カスタムプロトコル `edgecut://` (`WebView2` では `http://edgecut.localhost/`)。ビルド時に埋め込んだ UI のファイルと、プレビュー画像の RGBA を返す |
| `waker` | ワーカースレッドからイベントループを起こす通知 (テストでは何もしない `Waker::noop` を使う) |
| `single_instance` | 多重起動の防止 (`%TEMP%` のロックファイルを排他ロックして、本体を利用者ごとに 1 つに限り、2 つ目の起動では既存のウィンドウを前面に出す。`unsafe` を使う唯一のモジュール) |
| `settings_store` | 設定ファイル (`%APPDATA%\EdgeCut\settings.json`。ポータブル版は `<実行フォルダ>\data\settings.json`) の読み書きと、egui 版の保存ファイル (`data\app.ron`) からの引き継ぎ |
| `bundled` | 配布物に同梱した文書 (利用者向けドキュメント・LICENSE・THIRD_PARTY_NOTICES) の探索と、Windows の既定のアプリでの起動。同梱先は実行ファイルの位置から MSI の配布レイアウトに従って求める |
| `actions/` | 操作。`dialog` / `load` / `io_result` / `trim` / `save` / `clipboard` / `batch` / `drop` / `app_update` に責務ごとに分割 (`test_helpers` はテスト用の `App` の組み立て) |
| `trim_worker` / `clipboard_worker` / `load_worker` | プレビュー用・コピーと貼り付け用・画像読み込み用の常駐ワーカースレッド。`clipboard_worker` はコピー時に ICC プロファイルから sRGB へ変換する (クリップボードはプロファイルを運べないため) |
| `save_dir` | 自動保存先の既定パスの決定 (Windows の既知のフォルダ) |
| `image_convert` | プレビュー画像の変換。長辺が上限を超える画像の縮小と、RGB / GRAY の ICC プロファイルから sRGB への色変換 (`moxcms`。コピーでも共用) を行う |
| `icon` | ウィンドウのアイコンの読み込み |

`ui/` の構成:

| パス | 責務 |
| :--- | :--- |
| `src/main.tsx` | エントリポイント。`?view=updater` なら更新プログラムの画面、それ以外は本体の画面を表示する |
| `src/types.ts` / `src/bridge.ts` | Rust とやり取りする型 (`src/app/ipc` と揃える)、コマンドの送信と画面状態の購読 |
| `src/App.tsx` | 画面全体の組み立て・テーマ・キーボードショートカット・ウィンドウ上端の大きさ変更 |
| `src/components/` | `TitleBar` (タイトルバーとツールバー) / `Preview` と `Pane` (canvas によるプレビュー・ズーム・パン・スポイト) / `Inspector` (設定パネル) / `StatusBar` / `Dialogs` と、共通部品 (`controls` / `overlays` / `Icon`) |
| `src/lib/` | 画面に依存しない計算 (`viewport`: ズームとパン、`format`: 表示用の文字列、`shortcuts`: ショートカットの判定)。テストは同じ場所の `*.test.ts` |
| `src/updater/` | 更新プログラムの画面 (`?view=updater` のときに `main.tsx` が表示する)。`UpdaterApp` が画面、`bridge` がページの購読と操作の送信、`describe` がページを見出し・本文・ボタンにする。型 (`types.ts`) は `src/app/updater/view.rs` と揃える |
| `scripts/` | UI に同梱する npm パッケージのライセンス検証 (`check-licenses.mjs`) と表記の生成 (`third-party-notices.mjs`)。ライセンス情報の取得と判定は両者で共有する `licenses.mjs` に置く |

`src/update/` の構成:

| パス | 責務 |
| :--- | :--- |
| `check` | version API の取得・SemVer の比較・配布形態 (`UpdatePackageKind`) に合うアセットの選択 |
| `session` | 本体側。更新プログラムを `%TEMP%\edge-cut-update\updater-*` へコピーして起動し、標準出力の `exit` 行 (終了の依頼) を受ける。標準入力のパイプは本体の終了まで保持し、更新プログラムはその終端で本体の終了を知る |
| `flow/` | 更新プログラムの進行 (確認 → ダウンロード → 本体の終了待ち → インストール → 起動)。画面と副作用をトレイト (`Ui` / `Ops`) で受け、本体の終了を依頼するまでは中止できる |
| `ops` | 更新プログラムの引数と `Ops` の実装 (本体の終了は標準入力の終端と実行ファイルの解放で判定) |
| `prepare` | アセットのダウンロードと検証・ZIP の展開・過去の版と終了した更新プログラムの作業フォルダの削除 |
| `extract` | ポータブル ZIP の展開 (展開先の外を指すエントリは飛ばす) |
| `install/` | 新しい版のインストールと本体の起動。`msi` が MSI の無人インストール (`msiexec /qn`。導入先は `bin` の親を `APPLICATIONFOLDER` として維持)、`portable` がポータブル版のファイル差し替え (退避フォルダを使い、失敗時は巻き戻す。`data` は対象外) |
| `verify` | アセット名・宣言サイズ・digest の検証と、サイズ上限付きのダウンロード・SHA-256 の照合 |
| `signature` | minisign 署名の検証 (ビルド時に埋め込んだ公開鍵・信頼コメントの照合) |
| `http` | 全リクエストにタイムアウトを設定した HTTP クライアント |
| `types` | GitHub API の最新リリース・`AvailableUpdate`・確認の結果・配布形態と検証済みの配布物 |
| `error` | `UpdateError` (利用者向けの文言は `Display` に集約) |
| `mod` | GitHub の URL・リリース元の定義と再エクスポート |
| `tests/` | ローカルの HTTP サーバー (`server`) を使う更新フローの結合テスト |

## 設計方針

- コアロジック (`trim` / `image_load` / `dpi` / `image_output` / `batch_trim`) は GUI (`tao` / `wry`) に依存させない。GUI への依存は `src/app/` に閉じ込める
- UI (TypeScript) は状態を持たない。Rust がイベントの処理後に画面状態 (`ipc::snapshot`) を作り、前回と変わったときだけ JSON で送る。UI はそれを描画し、操作は `window.ipc.postMessage` のコマンド (`ipc::command`) で返す。例外は表示だけに関わるもの (プレビューのズームとパン、スライダーのドラッグ中の値、詳細設定の開閉) で、UI 側に持つ
- プレビュー画像はカスタムプロトコルの `/preview/{id}` から RGBA のまま取得し、UI が canvas に描く。トリミング後の表示も同じ画像から切り出し、切り抜き画像は作らない
- `WebView` には UI のオリジン以外への遷移・新しいウィンドウを許さず、コマンドも UI のオリジンからのものだけ受け付ける
- 設定は UI のコマンドで変わるたびに保存する (スライダーのドラッグ中は終わるまで保存しない)
- 重い処理は UI スレッドで行わず、ワーカースレッドへ渡す。プレビュー・クリップボード・読み込みは常駐ワーカー、ファイルダイアログ・保存・一括トリミング・ドロップされたフォルダの展開・更新確認はその都度起こすスレッドで行い、結果はチャネル経由で UI スレッドへ返す。ワーカーの panic は `catch_panic` で捕捉して失敗として通知する
- ウィンドウを閉じる要求があったらウィンドウを非表示にして設定を保存し (`event_loop` の `request_close`)、イベントループを抜けた後、保存スレッドと一括トリミングは `IoState` / `BatchState` の `Drop` で完了を待ってからプロセスを終える。一括トリミングは未着手のファイルをキャンセルする。待機の期限は最初に破棄された側が 1 つだけ決めて共有するため、待つのは合計で最大 10 秒で、超えたら切り離して終了する (`App` に `Drop` を実装すると構造体更新構文が使えなくなるため、フィールド側で待つ)
- 一括トリミングの実行中は閉じる要求を取り消し、終了確認ダイアログ (UI の `Dialogs`) を出す。「中止して終了」で改めて閉じる要求を送り、上記の手順で終了する
- 公開 API を追加・変更した場合は `src/lib.rs` の再エクスポートとモジュール表を更新する。ライブラリとしての使い方は `src/lib.rs` の doc コメント (`cargo doc --open`) を参照
- 対応 OS は Windows のみ。`cfg(windows)` などの OS 分岐は置かず、Windows 専用 API を直接使う

背景色検出・余白判定のアルゴリズム (利用者向けの説明) は [動作の仕組み](../user/how-it-works.md) を参照。
