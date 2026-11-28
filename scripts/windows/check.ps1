# ========================================================================
# Script Name : check.ps1
# Description : Windows用の品質チェックスクリプト(UI の型・静的解析・テスト・ライセンス・監査 / fmt / clippy / doc / 許可リスト同期 / deny / test / doctest)
# Usage       : ./check.ps1
# Requires    : PowerShell 7.0+
# ========================================================================

#requires -Version 7.0

# シェルオプションを設定(エラー時に即終了)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# ダブルクリック実行時の Enter 待ちヘルパーを読み込み
. (Join-Path $PSScriptRoot '..\common\wait-if-double-clicked.ps1')

# ネイティブコマンドの終了コード検査ヘルパーを読み込み
. (Join-Path $PSScriptRoot '..\common\invoke-native.ps1')

Invoke-ScriptMain {
    # プロジェクトルートへ移動
    . (Join-Path $PSScriptRoot '..\common\cd-project-root.ps1')

    # 品質チェックを開始(--locked で Cargo.lock の更新漏れもエラーにする)
    Write-Host '品質チェックを実行しています...'
    Write-Host ''

    # UI (TypeScript) の依存をロックファイルどおりに揃える (build.rs のビルドもこれを使う)
    Write-Host 'UI の依存をインストール中...'
    Invoke-Native pnpm --dir ui install --frozen-lockfile
    Write-Host ''

    # UI の型検査・静的解析・テスト
    Write-Host 'UI の型検査・静的解析・テストを実行中...'
    Invoke-Native pnpm --dir ui run typecheck
    Invoke-Native pnpm --dir ui run lint
    Invoke-Native pnpm --dir ui test
    Write-Host ''

    # UI に同梱する npm パッケージのライセンスと脆弱性を検証
    Write-Host 'UI の依存のライセンスと脆弱性を検証中...'
    Invoke-Native pnpm --dir ui run licenses:check
    Invoke-Native pnpm --dir ui audit --prod
    Write-Host ''

    # コード整形を検証
    Write-Host 'コード整形を検証中...'
    Invoke-Native cargo fmt --check
    Write-Host ''

    # Clippy を実行
    Write-Host 'Clippy を実行中...'
    Invoke-Native cargo clippy --all-targets --locked '--' -D warnings
    Write-Host ''

    # ドキュメントを生成 (リンク切れなどの rustdoc 警告はエラー扱い)
    Write-Host 'ドキュメントを検証中...'
    $previousRustDocFlags = $env:RUSTDOCFLAGS
    $env:RUSTDOCFLAGS = '-D warnings'
    try {
        Invoke-Native cargo doc --no-deps --document-private-items --locked
    } finally {
        if ($null -eq $previousRustDocFlags) {
            Remove-Item Env:RUSTDOCFLAGS -ErrorAction SilentlyContinue
        } else {
            $env:RUSTDOCFLAGS = $previousRustDocFlags
        }
    }
    Write-Host ''

    # deny.toml と about.toml の許可リストの一致を検証
    Write-Host '許可ライセンスの同期を検証中...'
    . (Join-Path $PSScriptRoot '..\common\license-allowlist-check.ps1')
    Write-Host ''

    # サプライチェーン監査を実行
    Write-Host 'サプライチェーン監査を実行中...'
    . (Join-Path $PSScriptRoot '..\common\cargo-deny-check.ps1')
    Write-Host ''

    # テストを実行
    Write-Host 'テストを実行中...'
    Invoke-Native cargo test --all-targets --locked

    Invoke-Native cargo test --doc --locked
    Write-Host ''

    # 完了メッセージを表示
    Write-Host 'すべてのチェックが完了しました'
}
