# ========================================================================
# Script Name : verify-msi.ps1
# Description : ビルド済み MSI の内容・メタデータを検証する
# Usage       : ./verify-msi.ps1
#               ./verify-msi.ps1 -MsiPath target\wix\edge-cut-2.0.0-x86_64.msi
#               ./verify-msi.ps1 -SkipExtract
# Requires    : PowerShell 7.0+, Windows (Windows Installer COM)
#
# 検証内容 (build-msi.ps1 も MSI 作成後に同じ検証を行う):
#   - Cargo.toml の upgrade-guid と wix/main.wxs の UpgradeCode の一致
#   - ProductName / ProductVersion / Manufacturer / 言語 / UpgradeCode / Platform (x64)
#   - per-user インストール (ALLUSERS)
#   - 必須ファイル (EdgeCut.exe, EdgeCutUpdater.exe, LICENSE, THIRD_PARTY_NOTICES, docs\index.html)
#   - 必須 Feature (Binaries, Environment)
#   - ライセンス同意画面が無く、完了画面に起動のチェックボックスがあること
#   - 管理インストールで展開したファイルの内容 (既定。-SkipExtract で省略)
# ========================================================================

#requires -Version 7.0

[CmdletBinding()]
param(
    # 検証する MSI (省略時は target/wix から Cargo.toml の version に一致する MSI を選ぶ)
    [string]$MsiPath,

    # 期待する ProductVersion (省略時は Cargo.toml の version)
    [string]$Version,

    # 管理インストールによる展開と、展開したファイルの検証をスキップする
    [switch]$SkipExtract
)

# シェルオプションを設定(エラー時に即終了)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# ダブルクリック実行時の Enter 待ちヘルパーを読み込み
. (Join-Path $PSScriptRoot '..\common\wait-if-double-clicked.ps1')

Invoke-ScriptMain {
    # 相対パスで指定された MSI は、呼び出し元のカレントディレクトリを基準に解決する
    if ($MsiPath) {
        $MsiPath = [System.IO.Path]::GetFullPath($MsiPath, (Get-Location).Path)
    }

    # プロジェクトルートへ移動
    . (Join-Path $PSScriptRoot '..\common\cd-project-root.ps1')
    . (Join-Path $PSScriptRoot '..\common\verify-msi.ps1')

    Invoke-MsiPackageVerification `
        -ProjectRoot $ProjectRoot `
        -MsiPath $MsiPath `
        -Version $Version `
        -SkipExtract:$SkipExtract
}
