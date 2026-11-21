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

## 目次

- [概要](#概要)
- [動作の仕組み](#動作の仕組み)
- [機能](#機能)
- [クイックスタート](#クイックスタート)
- [使い方](#使い方)
- [対応フォーマット](#対応フォーマット)
- [読み込み上限](#読み込み上限)
- [要件](#要件)
- [ビルド](#ビルド)
- [開発](#開発)
- [ライセンス](#ライセンス)

---

## 概要

EdgeCut は、画像の四辺から背景色を推定し、余白を除去するデスクトップアプリケーションです。  
スクリーンショットの周囲余白、単色背景のイラスト、透過 PNG の外側などを、許容度と余白ピクセルを調整しながらプレビューできます。

---

## 動作の仕組み

<table>
  <tr>
    <td align="center" valign="top" width="100">
      <br>
      <kbd>&nbsp;Step 1&nbsp;</kbd>
      <br><br>
    </td>
    <td valign="top">
      <br>
      <b>背景色の検出</b><br>
      画像の上・下・左・右の全辺ピクセルを走査し、量子化した色の近傍で最も票の多いクラスタから、最頻出の RGBA 値を背景色とする。JPEG ノイズなどで同系色の票が割れても安定して検出でき、角が丸い素材や隅に異なる色がある画像でも誤検出しにくい
      <br><br>
    </td>
  </tr>
  <tr>
    <td align="center" valign="top">
      <br>
      <kbd>&nbsp;Step 2&nbsp;</kbd>
      <br><br>
    </td>
    <td valign="top">
      <br>
      <b>余白の判定</b><br>
      各ピクセルと背景色の RGBA 距離が「許容度」以内なら背景とみなす。許容度 N のしきい値は RGB の最大距離 (黒〜白、約 441.7) の N% で、0 は完全一致、100 はすべてのピクセルを背景とする。アルファ差も距離に含めるため、不透明な背景上の半透明ピクセルは内容として残る。アルファ値が 16 未満のほぼ完全透明ピクセルは常に背景として扱う
      <br><br>
    </td>
  </tr>
  <tr>
    <td align="center" valign="top">
      <br>
      <kbd>&nbsp;Step 3&nbsp;</kbd>
      <br><br>
    </td>
    <td valign="top">
      <br>
      <b>トリミング</b><br>
      上下左右から背景と判定された領域を除去し、必要に応じて四辺へ指定ピクセル数の余白を追加する
      <br><br>
    </td>
  </tr>
</table>

> 💡 背景色は自動検出のほか、カラーピッカーでの手動指定や元画像のスポイトでも上書きできます。

---

## 機能

<details open>
<summary>✂️ &nbsp;<b>トリミング</b></summary>
<br>

| 機能 | 説明 |
| ---- | ---- |
| 自動余白除去 | 四辺の最頻色を背景として検出し、上下左右の余白を除去 |
| 許容度調整 | 0〜100 の範囲で背景色との類似度閾値を設定 |
| 余白追加 | トリミング後に 0〜100 px の余白を四辺に追加 (元画像の端を越える分は背景色で塗って追加) |
| 背景色の指定 | 自動検出 / カラーピッカー / スポイト / 手動 RGBA 編集 |

</details>

<details open>
<summary>📂 &nbsp;<b>入出力</b></summary>
<br>

| 機能 | 説明 |
| ---- | ---- |
| ファイルを開く | ダイアログ選択、ドラッグ & ドロップ |
| クリップボード | 画像の貼り付け読み込み、トリミング結果のコピー |
| 出力形式 | 「形式」で 元の形式 / PNG / JPEG / WebP / BMP / TIFF / GIF を選択。保存・自動保存・一括トリミングに共通で適用 |
| 保存 | ファイルダイアログから任意のパスへ出力 (既定名は `{名前}_trimmed.{拡張子}`) |
| 自動保存 | 画像を開いた直後に指定フォルダへ自動保存 |
| 一括トリミング | 複数ファイルをまとめて処理 (`{名前}_trimmed.{拡張子}`)。複数ファイルのドラッグ & ドロップでも開始 |
| EXIF 回転の反映 | スマートフォン写真などの Orientation (回転・反転) を読み込み時に適用 |
| ICC プロファイルの引き継ぎ | 元画像の ICC プロファイルを PNG / JPEG / TIFF / WebP の出力へ再埋め込みし、色の見え方を保つ |

</details>

<details open>
<summary>🖥️ &nbsp;<b>UI</b></summary>
<br>

| 機能 | 説明 |
| ---- | ---- |
| 並列プレビュー | 元画像とトリミング後を左右に並べて表示。元画像側は削られる領域を暗くし、トリミング範囲の枠線を重ねる |
| コンパクト UI | トリミング後の画像のみを表示する省スペースモード |
| ダーク / ライト | テーマをワンクリックで切り替え |
| 設定の永続化 | 許容度・余白・テーマ・自動保存先などを次回起動時に復元 |
| アプリ更新 (Windows) | 最新リリースを確認し、署名を検証した MSI でアップデート |

</details>

---

## クイックスタート

リリースバイナリをお持ちでない場合は、Rust ツールチェーンをインストールしたうえで次を実行します。

```bash
cargo run --release
```

> Windows で MSI インストーラーを作成する場合は [ビルド](#ビルド) の Windows セクションを参照してください。

---

## 使い方

<details open>
<summary>📋 &nbsp;<b>基本的な流れ</b></summary>
<br>

| ステップ | 操作 | 説明 |
| :------: | ---- | ---- |
| **1** | **画像を開く** | 「📂 開く」でファイルを選択、ウィンドウへドラッグ & ドロップ、または「📋 貼り付け」でクリップボードから読み込み |
| **2** | **許容度を調整** | スライダーで背景色の許容範囲を設定 (0: 完全一致に近い、100: 広い範囲を余白とみなす) |
| **3** | **余白を調整** | トリミング後に追加したい余白ピクセル数を設定 (0〜100 px)。内容から四辺までが常に指定幅になり、元画像の端を越える分は背景色で塗られる |
| **4** | **結果を出力** | 「💾 保存」でファイルへ書き出す、または「📋 コピー」でクリップボードへコピー |

スライダーはドラッグ中もプレビューが更新されます。ホバー中に <kbd>←</kbd> <kbd>→</kbd> キーでも値を微調整できます。

</details>

<details open>
<summary>🎨 &nbsp;<b>背景色の調整</b></summary>
<br>

自動検出で意図とずれる場合は、許容度・余白スライダーの右側にある背景色コントロール (画像の読み込み中のみ表示) で上書きできます。現在の背景色は色見本と `RGBA(r, g, b, a)` で表示され、手動指定中は「手動」と表示されます。

| 操作 | 説明 |
| ---- | ---- |
| **カラーピッカー** | 色見本をクリックして RGBA を直接編集する |
| **スポイト** | 「スポイト」を押してから元画像をクリックし、そのピクセルの色を背景色にする (元画像を表示する標準モードのみ) |
| **自動** | 四辺から背景色を再検出する (手動指定を解除。手動指定中のみ有効) |

> ⚠️ 手動で指定した背景色は「自動」を押すまで解除されません。以降に開く画像・一括トリミング・次回起動時にもそのまま適用されます。

</details>

<details open>
<summary>💾 &nbsp;<b>自動保存</b></summary>
<br>

「自動保存」を有効にすると、画像を開いた直後にトリミング結果を保存します。  
「📁 変更」で保存先フォルダを指定できます。

- 出力ファイル名: `{元ファイル名}_trimmed.{拡張子}` (クリップボードから貼り付けた画像は `clipboard_trimmed.{拡張子}`)
- 拡張子は「形式」の設定に従う (下記「出力形式」参照)
- 同名ファイルがある場合は上書きせず `_2`, `_3`, ... と連番を付ける

| プラットフォーム | デフォルトの保存先 |
| :--------------: | ------------------ |
| Windows | `%USERPROFILE%\Pictures\EdgeCut` |
| Linux / macOS | `~/Pictures/EdgeCut` |

</details>

<details open>
<summary>📦 &nbsp;<b>一括トリミング</b></summary>
<br>

「📂 一括トリミング」から複数の画像ファイルと出力フォルダを選択すると、現在の許容度・余白・背景色設定でまとめて処理します。

- 出力ファイル名: `{元ファイル名}_trimmed.{拡張子}`
- 拡張子は「形式」の設定に従う (下記「出力形式」参照)
- 同名ファイルがある場合は上書きせず `_2`, `_3`, ... と連番を付ける
- 複数の画像ファイルをウィンドウへドラッグ & ドロップした場合も、出力フォルダを選択して一括トリミングを開始 (未対応の拡張子は除外し、除外件数をステータスバーに表示)
- フォルダをドラッグ & ドロップした場合は、フォルダ直下の対応画像を一括トリミング (サブフォルダは対象外。対象が 1 件でも一括トリミングとして処理)
- 処理はバックグラウンドで 1 ファイルずつ実行され、進捗はステータスバーに表示
- 「⏹ キャンセル」で次のファイル処理前に中止可能
- 失敗したファイルがあっても残りの処理は続行し、エラーの詳細はステータスバーのホバーで確認できる

</details>

<details open>
<summary>🗂️ &nbsp;<b>出力形式</b></summary>
<br>

ツールバーの「形式」で、保存・自動保存・一括トリミングの出力形式をまとめて指定できます (設定は次回起動時も保持)。

| 選択 | 出力 |
| ---- | ---- |
| 元の形式 (既定) | 元画像の拡張子を維持 (小文字化)。拡張子がない・未対応の場合とクリップボードから貼り付けた画像は PNG |
| PNG / JPEG / WebP / BMP / TIFF / GIF | 指定形式 (`.png` / `.jpg` / `.webp` / `.bmp` / `.tiff` / `.gif`) |

- 「💾 保存」ではダイアログの既定のファイル名・種類に反映される。ダイアログで別の拡張子を入力した場合はその形式で保存し、拡張子を省略した場合は設定の形式で保存する
- JPEG は透過部分を白背景に合成して品質 95 で保存する。PNG / TIFF 以外の形式では 16bit 画像を 8bit に変換する

</details>

<details>
<summary>🔄 &nbsp;<b>アプリ更新 (Windows)</b></summary>
<br>

ツールバー右端の「⋯」メニューから現在のバージョンを確認し、更新を適用できます。

1. 「更新を確認…」で更新ダイアログを開き、最新リリースを問い合わせる
2. 新しいバージョンがあれば、現在・最新のバージョンとサイズが表示される。「⬇ ダウンロードしてインストール」で MSI をダウンロードする
3. サイズ・SHA-256・minisign 署名をすべて検証できた場合のみインストーラーを起動し、EdgeCut は終了する (未保存の画像は先に保存しておく)

確認・ダウンロードにはタイムアウトがあり、失敗時はダイアログの「再試行」でやり直せます。

検証に失敗した MSI は適用しません。仕組みは [更新署名鍵](#更新署名鍵) を参照してください。

</details>

<details open>
<summary>🛠️ &nbsp;<b>その他の操作</b></summary>
<br>

| ボタン | 説明 |
| ------ | ---- |
| 🗑 クリア | 読み込んだ画像をクリアして初期状態に戻す |
| ⊟ コンパクト / ⊞ 標準 | プレビュー表示モードの切り替え |
| 🌙 ダーク / ☀ ライト | テーマの切り替え |
| ⋯ | バージョン表示・更新確認 (Windows のみ) |

</details>

---

## 対応フォーマット

読み込み・保存ともに次のフォーマットに対応します。

<div align="center">

`PNG` &nbsp; `JPEG` (`.jpg` / `.jpeg`) &nbsp; `BMP` &nbsp; `GIF` &nbsp; `TIFF` (`.tiff` / `.tif`) &nbsp; `WebP`

</div>

JPEG で保存する場合、透過部分は白背景に合成され、品質 95 でエンコードされます。ICC プロファイルは PNG / JPEG / TIFF / WebP へ保存する場合のみ引き継ぎます (BMP / GIF は非対応。CMYK JPEG など出力と色空間が合わないプロファイルは埋め込まない)。

---

## 読み込み上限

大容量画像によるメモリ枯渇を防ぐため、次の上限があります。

| 項目 | 上限 |
| ---- | :--: |
| ファイルサイズ | 256 MB |
| 幅・高さ | 各 16,384 px |
| ピクセル数 | 約 6,400 万 (8,000×8,000 相当) |

上限を超えるファイルは読み込み時にエラーとなり、ステータスバーに理由が表示されます。クリップボードから貼り付けた画像にも同じ幅・高さ・ピクセル数の上限が適用されます。

プレビューでは長辺 4,096 px を超える画像を縮小して表示しますが、保存・コピーは元の解像度で行います。

---

## 要件

- Windows 10 以降 / Linux / macOS
- [Rust ツールチェーン](https://rustup.rs/) 1.96 以上 (`rust-toolchain.toml` で固定)

<details>
<summary>🪟 &nbsp;<b>Windows</b></summary>
<br>

- **リリースビルド / MSI**: Visual Studio Build Tools など MSVC ツールチェーン (C++ ワークロード)
- **MSI 作成**: [cargo-wix](https://github.com/volks73/cargo-wix) と [WiX Toolset v3.14.1](https://github.com/wixtoolset/wix3/releases)

```powershell
cargo install cargo-wix --locked
winget install WiXToolset.WiXToolset
```

</details>

<details>
<summary>🐧 &nbsp;<b>Linux</b></summary>
<br>

GUI 表示には X11 または Wayland 向けの開発ライブラリが必要です。Ubuntu / Debian の例:

```bash
sudo apt install libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
  libspeechd-dev libxkbcommon-dev libgtk-3-dev libssl-dev
```

日本語表示には Noto CJK などの日本語フォントパッケージのインストールを推奨します:

```bash
sudo apt install fonts-noto-cjk
```

</details>

<details>
<summary>🍎 &nbsp;<b>macOS</b></summary>
<br>

- Xcode Command Line Tools (`xcode-select --install`)

</details>

---

## ビルド

```bash
# デバッグビルド & 実行
cargo run

# リリースビルド
cargo build --release
```

ビルド成果物は `target/release/EdgeCut` (Windows では `EdgeCut.exe`) に生成されます。

### ビルドスクリプト

各プラットフォーム向けのラッパースクリプトを `scripts/` 以下に用意しています。

| プラットフォーム | ディレクトリ | 備考 |
| :--------------: | ------------ | ---- |
| Windows | `scripts/windows/` | PowerShell 7 向け (エクスプローラーからのダブルクリック実行にも対応) |
| Linux | `scripts/linux/` | Bash 向け |
| macOS | `scripts/macos/` | Bash 向け |

<details>
<summary>🪟 &nbsp;<b>Windows スクリプト一覧</b></summary>
<br>

| スクリプト | 説明 |
| ---------- | ---- |
| `build.ps1` | デバッグ / リリースビルド (`--release`) |
| `test.ps1` | テスト実行 |
| `clippy.ps1` | Clippy による静的解析 |
| `format.ps1` | コードフォーマット |
| `check.ps1` | CI 向け一括チェック (fmt / clippy / deny / test / doc) |
| `build-msi.ps1` | MSI インストーラー作成 (`-SkipBuild` でビルド省略可。`THIRD_PARTY_NOTICES` を自動生成して同梱) |
| `generate-wix-assets.ps1` | MSI 用のバナー・ダイアログ画像 (`wix/*.bmp`) と使用許諾 (`wix/Eula.rtf`) を再生成 |
| `release.ps1` | リリース一括実行 (バージョン更新 / CHANGELOG / 品質チェック / コミット / MSI 作成 / minisign 署名)。詳細は [リリース](#リリース) |

MSI は `target/wix/` に `{パッケージ名}-{バージョン}-{アーキテクチャ}.msi` として出力されます。インストール先は `%LOCALAPPDATA%\edge-cut` (`THIRD_PARTY_NOTICES` を同梱)。

</details>

<details>
<summary>🐧 &nbsp;<b>Linux スクリプト一覧</b></summary>
<br>

| スクリプト | 説明 |
| ---------- | ---- |
| `build.sh` | デバッグ / リリースビルド (`--release` / `-r`) |
| `build-package.sh` | 配布用 tar.gz 作成 (`--skip-build` でビルド省略可。`THIRD_PARTY_NOTICES` を自動生成して同梱) |
| `test.sh` | テスト実行 |
| `clippy.sh` | Clippy による静的解析 |
| `format.sh` | コードフォーマット |
| `check.sh` | CI 向け一括チェック (fmt / clippy / deny / test / doc) |

tar.gz は `target/dist/` に `{パッケージ名}-{バージョン}-{ターゲット}.tar.gz` として出力されます。展開後 `install.sh` で `~/.local` にインストールできます (`LICENSE` / `THIRD_PARTY_NOTICES` / デスクトップエントリを同梱)。

</details>

<details>
<summary>🍎 &nbsp;<b>macOS スクリプト一覧</b></summary>
<br>

| スクリプト | 説明 |
| ---------- | ---- |
| `build.sh` | デバッグ / リリースビルド (`--release` / `-r`) |
| `build-app.sh` | `.app` バンドル作成 (`--skip-build` でビルド省略可。`THIRD_PARTY_NOTICES` を `Contents/Resources/` に同梱) |
| `build-dmg.sh` | DMG 配布イメージ作成 (`--skip-app` で `.app` 生成を省略可) |
| `test.sh` | テスト実行 |
| `clippy.sh` | Clippy による静的解析 |
| `format.sh` | コードフォーマット |
| `check.sh` | CI 向け一括チェック (fmt / clippy / deny / test / doc) |

`.app` は `target/dist/EdgeCut.app`、DMG は `target/dist/{パッケージ名}-{バージョン}-{ターゲット}.dmg` に出力されます。

</details>

---

## 開発

プロジェクトは Rust 2024 edition、`eframe` / `egui` によるネイティブ GUI アプリです。lib クレート `edge_cut` (`src/lib.rs`) にコアロジックと GUI を置き、bin クレート `EdgeCut` (`src/main.rs`) は `edge_cut::app::run()` を呼ぶだけのエントリポイントです。

<details open>
<summary>📁 &nbsp;<b>主要モジュール構成</b></summary>
<br>

| パス | 責務 |
| ---- | ---- |
| `src/trim.rs` | 背景色検出 (量子化クラスタの最頻 RGBA)・余白除去のコアロジック |
| `src/image_load.rs` | サイズ上限付きの安全な画像読み込み (EXIF Orientation の反映・ICC プロファイルの取得を含む) |
| `src/image_output.rs` | 出力形式の決定・画像保存 (ICC プロファイルの再埋め込み)・出力パス生成 (衝突時は連番サフィックス) |
| `src/batch_trim.rs` | 複数ファイルの一括トリミング (進捗通知・キャンセル) |
| `src/consts.rs` | 上限値・対応拡張子・ウィンドウサイズなどの定数 |
| `src/app/` | egui GUI。状態管理・描画・操作・プレビュー用 / クリップボード用ワーカースレッド |
| `src/update/` | gh-release-bridge 経由の更新確認・署名検証・MSI 適用 (Windows のみ) |
| `tests/` | lib クレートの公開 API に対する統合テスト |

コアロジック (`trim` / `image_load` / `image_output` / `batch_trim`) は egui に依存しません。ライブラリとしての使い方は `src/lib.rs` の doc コメント (`cargo doc --open`) を参照してください。

</details>

品質チェックは `scripts/{platform}/check.*` で一括実行できます。個別に実行する場合:

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo deny check
cargo test --all-targets
cargo test --doc
```

コーディング規約 (コメント・`use` 文の並び) は [`.claude/rules/`](.claude/rules/) にあります。コミットメッセージは `<種別>: <日本語の要約>` 形式で、ユーザー向けの変更は [`CHANGELOG.md`](CHANGELOG.md) の `[Unreleased]` に追記します。

### ブランチ運用

git-flow 形式です。`develop` から `feature/*` / `fix/*` / `docs/*` などを切り、`main` へは `release/vX.Y.Z` 経由でマージします。

### 依存関係の監査

[`deny.toml`](deny.toml) で依存関係の脆弱性・ライセンス・取得元を検証する。Windows / Linux / macOS の 3 ターゲット (`x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`) を対象にする。`cargo-deny` は `cargo install cargo-deny --locked` で導入する。

### リリース

`release/v{version}` ブランチ上で `scripts/windows/release.ps1` を実行する。

```powershell
git checkout -b release/v0.3.0
./scripts/windows/release.ps1 -Version 0.3.0   # または -Bump Patch。-DryRun で変更内容のみ表示
```

1. `Cargo.toml` / `Cargo.lock` のバージョンを更新し、`CHANGELOG.md` の `[Unreleased]` 節をリリース見出しへ移す (空だと失敗する)
2. `check.ps1` で品質チェックを実行し (`-SkipCheck` で省略)、`Release: v{version}` でコミットする (`-NoCommit` で省略)
3. `build-msi.ps1` で MSI を作成し、`minisign` で `<MSI>.minisig` を作って `keys/update-signing/*.pub` で検証する (`-SkipMsi` で両方省略)

**前提:**

- 環境変数 `EDGE_CUT_KEY` (gh-release-bridge の Worker Secret と同値) と `EDGE_CUT_SIGNING_KEY` (minisign 秘密鍵のパス) を設定しておく。署名の準備はビルド前に確認する
- 既定では作業ツリーがクリーンであること (`-AllowDirty` で上書き可)

**手動で残る作業:** `release/v{version}` を push し、Gitea 上で `v{version}` タグを付与する。GitHub Release へ MSI と署名ファイル (`*.msi.minisig`) をアップロードしたあと、gh-release-bridge の version API が MSI の `digest` を返し、署名ファイルも同じ URL 形式 (`/{owner}/{repo}/{tag}/{asset}.minisig`) で取得できることを確認する。

### 更新署名鍵

アプリ更新 (Windows) は、version API の digest (SHA-256) に加えて MSI の minisign (Ed25519) 署名を検証してから `msiexec` を起動する。署名の信頼コメント (`edge-cut-update version={version} file={アセット名}`) も照合するため、配信経路や digest を差し替えられても、正規の秘密鍵で署名されていない MSI や別バージョン向けの署名済み MSI は適用しない。

公開鍵は `keys/update-signing/*.pub` に置き、`build.rs` がビルド時にすべて埋め込む。秘密鍵はリポジトリに置かない。

```powershell
# minisign の導入
winget install jedisct1.minisign

# 鍵ペアの生成 (秘密鍵はリポジトリ外。パスワード保護を推奨)
minisign -G -p keys/update-signing/edge-cut-release.pub -s "$env:USERPROFILE\.minisign\edge-cut-release.key"

# release.ps1 で使う秘密鍵のパス
$env:EDGE_CUT_SIGNING_KEY = "$env:USERPROFILE\.minisign\edge-cut-release.key"
```

- 公開鍵を追加・変更した版を配布してから、その鍵で署名した更新を出す。利用者の手元のアプリは自分に埋め込まれた公開鍵でしか検証できない
- 鍵の入れ替えは、新旧の公開鍵を両方置いた版を一度配布し、以降の版で旧鍵を外す。秘密鍵を紛失・漏洩した場合も同じ手順で、旧鍵を外した版は手動で配布する
- 公開鍵が 1 つも無いビルドは更新を適用しない (開発ビルドでの誤適用を防ぐ)。`release.ps1` は公開鍵が無いと失敗する

### 第三者ライセンス表示 (`THIRD_PARTY_NOTICES`)

`LICENSE` は EdgeCut 本体の利用条件を示す。配布バイナリに含まれる OSS 依存の帰属・ライセンス全文は **`THIRD_PARTY_NOTICES`** にまとめる。

| 項目 | 内容 |
| :--- | :--- |
| 生成ツール | [cargo-about](https://github.com/EmbarkStudios/cargo-about) (`cargo install cargo-about --locked --features cli`) |
| 設定 | [`about.toml`](about.toml)、テンプレート [`scripts/templates/third-party-notices.hbs`](scripts/templates/third-party-notices.hbs) |
| 手動生成 | `scripts/common/generate-third-party-notices.ps1` (Windows) / `generate-third-party-notices.sh [TARGET]` (Linux / macOS) |
| 出力 | リポジトリルートの `THIRD_PARTY_NOTICES` (`.gitignore` 対象。パッケージ作成時に自動生成) |
| ターゲット | 配布スクリプトが OS 別 triple を指定 (`deny.toml` と同じ 3 ターゲット) |

| プラットフォーム | 同梱先 |
| :--------------- | :----- |
| **Windows MSI** | `%LOCALAPPDATA%\edge-cut\THIRD_PARTY_NOTICES` (`build-msi.ps1` が生成後に同梱) |
| **Linux tar.gz** | 展開先 `edge-cut/THIRD_PARTY_NOTICES` (`build-package.sh` が生成後に同梱。`install.sh` で `~/.local/share/edge-cut/` へコピー) |
| **macOS .app / DMG** | `EdgeCut.app/Contents/Resources/THIRD_PARTY_NOTICES` (`build-app.sh` / `build-dmg.sh`) |

---

## ライセンス

[MIT](LICENSE)
