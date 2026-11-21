# Changelog

このプロジェクトの注目すべき変更はすべてこのファイルに記録する。

形式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に準拠し、[Semantic Versioning](https://semver.org/lang/ja/) に従う。

## [Unreleased]

## [0.1.1] - 2026-07-04

### Added

- `cargo-deny` によるサプライチェーン監査 (`deny.toml` と check スクリプト)
- `cargo-about` で生成した `THIRD_PARTY_NOTICES` の各配布物への同梱 (Windows MSI / Linux tar.gz / macOS .app・DMG)
- コア処理 (`open_image` / `trim` / 保存 / `batch_trim_output_path`) の統合テスト
- `edge_cut` ライブラリクレートとしての公開 API (`TrimResult`, `LoadError`, `SaveError`, `BatchFileError` など)

### Changed

- トリム・画像入出力などのコアロジックを `edge_cut` ライブラリへ分離し、egui 依存を GUI 層に限定
- 対応画像拡張子定数を `consts` に集約
- 一括トリミング処理を `batch_trim` モジュールへ切り出し、逐次処理でキャンセルを確実化
- `trim` の戻り値を `TrimResult` に変更し、エラー型に `std::error::Error` を実装

### Fixed

- 一括トリミングで既存出力ファイルと衝突する場合、連番サフィックスで回避
- 一括トリミングのキャンセル処理とエラー詳細表示 (ステータスバーのホバー) を改善

### Performance

- 大画像プレビューの縮小表示、RGBA 直接参照、逐次/並列の切替、画像読み込みの単一ファイルオープンによりピークメモリと I/O を削減

## [0.1.0] - 2026-06-24

### Added

- Linux / macOS 対応
- 背景色の手動指定 (カラーピッカー・スポイト・RGBA 編集) と設定の永続化
- 大容量画像の読み込み上限
- 設定値のバリデーション
- プレビュートリミングのバックグラウンド化 (専用ワーカースレッド) による UI フリーズ防止
- スライダードラッグ中のリアルタイムプレビュー更新

### Changed

- 一括トリミングで元画像のフォーマットを維持して保存
- `App` 構造体を責務別の部分構造体へ分割
- アプリケーションアイコンを更新
- MSI インストーラー用の実行時画像を追加

## [0.0.2] - 2026-04-12

### Added

- 一括トリミング (バックグラウンド並列処理、進捗表示、キャンセル)
- クリップボードへの画像コピー
- 設定の保存・次回起動時の自動復元
- キーボードによるスライダー操作
- ユニットテストモジュール
- `cargo` コマンド実行用スクリプト

### Changed

- エッジ走査・一括トリミングの並列化と走査最適化
- 全ピクセルが背景色の場合の早期リターン
- スライダー操作中はトリミング処理を抑制
- テクスチャ再生成コストの削減
- デフォルトの自動保存先ディレクトリを変更
- ディレクトリ環境変数の優先順位を変更

### Fixed

- 背景色判定アルゴリズムの不具合
- ステータスメッセージが意図せず上書きされる問題

## [0.0.1] - 2026-04-10

### Added

- 画像の背景余白を自動検出・トリミングする GUI アプリケーション (Windows)
- リアルタイムプレビュー (元画像とトリミング後の左右比較)
- 許容度・余白ピクセルのスライダー調整
- コンパクト UI モード
- Windows MSI インストーラー

### Fixed

- アンダーフローによるパニックが発生しうる不具合
- 既存フォントを上書きしてしまう問題
- ツールチップに使用できないショートカットキーが記載されていた問題

[Unreleased]: https://github.com/GnudMna/edge-cut/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/GnudMna/edge-cut/compare/v0.0.2...v0.1.0
[0.0.2]: https://github.com/GnudMna/edge-cut/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/GnudMna/edge-cut/releases/tag/v0.0.1
