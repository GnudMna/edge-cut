# ========================================================================
# Script Name : build-portable.ps1
# Description : Windows 向けポータブル配布物 (ZIP) の作成
# Usage       : ./build-portable.ps1 [-SkipBuild]
# Requires    : PowerShell 7.0+, Rust / Cargo, cargo-about, mdBook
#
# 前提:
#   - 実行ファイルの隣の `portable` マーカーでポータブル版として動くこと
#     (設定などは実行フォルダの data\ に保存する。src/install_layout.rs)
#
# 出力:
#   target/portable/edge-cut-{version}-x86_64-portable/
#   target/portable/edge-cut-{version}-x86_64-portable.zip
# ========================================================================

#requires -Version 7.0

[CmdletBinding()]
param(
    # リリースビルドをスキップし、既存の target/release/*.exe を使う
    [switch]$SkipBuild
)

# シェルオプションを設定(エラー時に即終了)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# ダブルクリック実行時の Enter 待ちヘルパーを読み込み
. (Join-Path $PSScriptRoot '..\common\wait-if-double-clicked.ps1')

Invoke-ScriptMain {
    # プロジェクトルートへ移動
    . (Join-Path $PSScriptRoot '..\common\cd-project-root.ps1')
    . (Join-Path $PSScriptRoot '..\common\release-version.ps1')

    # アセット名の CPU 名は更新確認 (std::env::consts::ARCH) と揃える
    $Arch = 'x86_64'
    $ExeNames = @('EdgeCut.exe', 'EdgeCutUpdater.exe')
    $MarkerName = 'portable'
    $ReadmeName = 'PORTABLE.txt'

    if (-not $SkipBuild) {
        Write-Host 'リリースビルドを実行中...'
        cargo build --locked --release --bins
        if ($LASTEXITCODE -ne 0) {
            throw "エラー: cargo build --locked --release --bins が終了コード $LASTEXITCODE で失敗しました"
        }
        Write-Host ''
    }

    $releaseDir = Join-Path $ProjectRoot 'target\release'
    foreach ($exe in $ExeNames) {
        if (-not (Test-Path -LiteralPath (Join-Path $releaseDir $exe))) {
            throw "エラー: $exe が見つかりません: $releaseDir (先にリリースビルドしてください)"
        }
    }

    . (Join-Path $PSScriptRoot '..\common\generate-third-party-notices.ps1') -Target 'x86_64-pc-windows-msvc'
    Write-Host ''

    . (Join-Path $PSScriptRoot '..\common\build-user-docs.ps1')
    $userDocsDir = Invoke-UserDocsBuild -ProjectRoot $ProjectRoot
    Write-Host ''

    $version = Get-PackageVersion -ProjectRoot $ProjectRoot
    $packageName = "edge-cut-$version-$Arch-portable"
    $portableRoot = Join-Path $ProjectRoot 'target\portable'
    $stagingDir = Join-Path $portableRoot $packageName
    $zipPath = Join-Path $portableRoot "$packageName.zip"

    foreach ($path in @($stagingDir, $zipPath)) {
        if (Test-Path -LiteralPath $path) {
            Remove-Item -LiteralPath $path -Recurse -Force
        }
    }
    $null = New-Item -ItemType Directory -Path $stagingDir -Force

    Write-Host "配布フォルダを作成しています: $stagingDir"
    foreach ($exe in $ExeNames) {
        Copy-Item -LiteralPath (Join-Path $releaseDir $exe) -Destination $stagingDir
    }
    foreach ($name in @('LICENSE', 'THIRD_PARTY_NOTICES')) {
        Copy-Item -LiteralPath (Join-Path $ProjectRoot $name) -Destination $stagingDir
    }
    Copy-Item -LiteralPath $userDocsDir -Destination (Join-Path $stagingDir 'docs') -Recurse
    Set-Content -LiteralPath (Join-Path $stagingDir $MarkerName) -Value '' -NoNewline -Encoding utf8
    $readme = @"
EdgeCut $version (ポータブル版)

EdgeCut.exe を起動すると使えます。インストールは不要です。
設定・WebView2 のデータ・ログは、このフォルダの data フォルダに保存します。
portable ファイルを削除すると、MSI 版と同じく利用者のプロファイルに保存します。

更新はメニューの「更新を確認…」から行えます。更新プログラム (EdgeCutUpdater.exe) が
新しい版をダウンロードして検証し、このフォルダのファイルを差し替えます (data フォルダは残します)。

使い方: docs\index.html
"@
    Set-Content -LiteralPath (Join-Path $stagingDir $ReadmeName) -Value $readme -Encoding utf8

    Write-Host "ZIP を作成しています: $zipPath"
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    # 展開後に単一のフォルダ ($packageName) へまとまるよう、親フォルダごと格納する
    [System.IO.Compression.ZipFile]::CreateFromDirectory(
        $stagingDir,
        $zipPath,
        [System.IO.Compression.CompressionLevel]::Optimal,
        $true
    )

    Write-Host ''
    Write-Host 'ポータブル配布物の作成が完了しました:'
    Write-Host "  $zipPath"
}
