# ========================================================================
# Script Name : generate-third-party-notices.ps1
# Description : THIRD_PARTY_NOTICES を cargo-about で生成し、利用者向けドキュメントの素材表示を追記する
# Usage       : . (Join-Path $PSScriptRoot 'generate-third-party-notices.ps1') [-Target TARGET_TRIPLE]
# Requires    : PowerShell 7.0+
# ========================================================================

#requires -Version 7.0

param(
    # cargo-about の依存グラフ解決用ターゲット。未指定時は about.toml の targets を使用
    [string]$Target = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (-not (Get-Command cargo-about -ErrorAction SilentlyContinue)) {
    throw @(
        'エラー: cargo-about が見つかりません',
        'インストール: cargo install cargo-about --locked --features cli'
    ) -join [Environment]::NewLine
}

. (Join-Path $PSScriptRoot 'cd-project-root.ps1')

$OutputFile = Join-Path $ProjectRoot 'THIRD_PARTY_NOTICES'
$Template = Join-Path $ProjectRoot 'scripts\templates\third-party-notices.hbs'
$Manifest = Join-Path $ProjectRoot 'Cargo.toml'
$Config = Join-Path $ProjectRoot 'about.toml'

if (-not (Test-Path -LiteralPath $Template)) {
    throw "エラー: テンプレートが見つかりません: $Template"
}

Write-Host 'THIRD_PARTY_NOTICES を生成しています...'

$aboutArgs = @(
    'about', 'generate', $Template,
    '--config', $Config,
    '--manifest-path', $Manifest,
    '--output-file', $OutputFile,
    '--all-features',
    '--locked'
)

if ($Target) {
    $aboutArgs += @('--target', $Target)
    Write-Host "  ターゲット: $Target"
}

cargo @aboutArgs

if ($LASTEXITCODE -ne 0) {
    throw "エラー: cargo about generate が終了コード $LASTEXITCODE で失敗しました"
}

# cargo-about は Rust の依存だけを扱うため、利用者向けドキュメント (mdBook の HTML) に含まれる素材の表示を追記する
$userDocsNotices = Join-Path $ProjectRoot 'scripts\templates\user-docs-notices.txt'
$notices = (Get-Content -LiteralPath $OutputFile -Raw -Encoding utf8).TrimEnd() + "`n`n" +
    (Get-Content -LiteralPath $userDocsNotices -Raw -Encoding utf8)
Set-Content -LiteralPath $OutputFile -Value $notices -Encoding utf8 -NoNewline

Write-Host "生成しました: $OutputFile"
