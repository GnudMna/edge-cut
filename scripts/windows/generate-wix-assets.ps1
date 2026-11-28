# ========================================================================
# Script Name : generate-wix-assets.ps1
# Description : WiX インストーラー用バナー・ダイアログ画像 (BMP) を生成する
# Usage       : ./generate-wix-assets.ps1
# Requires    : PowerShell 7.0+, Windows (System.Drawing)
#
# 出力 (WiX 必須サイズ):
#   wix/Banner.bmp  - 493 x 58  (上部バナー。左白 + 右装飾)
#   wix/Dialog.bmp  - 493 x 312 (ようこそ画面。左装飾 + 右白背景)
# ========================================================================

#requires -Version 7.0

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

. (Join-Path $PSScriptRoot '..\common\wait-if-double-clicked.ps1')

# アプリアイコン (icon.svg) と揃えたブランドカラー
$ColorPrimary = [System.Drawing.Color]::FromArgb(23, 23, 31)
$ColorPrimaryDark = [System.Drawing.Color]::FromArgb(11, 11, 15)
$ColorAccent = [System.Drawing.Color]::FromArgb(56, 189, 248)

function New-LinearGradientBrush {
    param(
        [System.Drawing.Rectangle]$Rect,
        [System.Drawing.Color]$ColorStart,
        [System.Drawing.Color]$ColorEnd,
        [float]$Angle = 0
    )

    return New-Object System.Drawing.Drawing2D.LinearGradientBrush @(
        $Rect,
        $ColorStart,
        $ColorEnd,
        $Angle
    )
}

function Set-GraphicsQuality {
    param([System.Drawing.Graphics]$Graphics)

    $Graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $Graphics.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::ClearTypeGridFit
}

function Draw-AppIcon {
    param(
        [System.Drawing.Graphics]$Graphics,
        [System.Drawing.Image]$Icon,
        [int]$X,
        [int]$Y,
        [int]$Size
    )

    $Graphics.DrawImage($Icon, $X, $Y, $Size, $Size)
}

function Save-WixBitmap {
    param(
        [int]$Width,
        [int]$Height,
        [string]$OutputPath,
        [scriptblock]$Draw
    )

    $bitmap = New-Object System.Drawing.Bitmap $Width, $Height
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    Set-GraphicsQuality -Graphics $graphics

    & $Draw $graphics $Width $Height

    $graphics.Dispose()
    $bitmap.Save($OutputPath, [System.Drawing.Imaging.ImageFormat]::Bmp)
    $bitmap.Dispose()
}

function New-BannerBitmap {
    param(
        [string]$OutputPath,
        [System.Drawing.Image]$Icon
    )

    # WiX 各ダイアログ: タイトル/説明はバナー左側 (X=15 付近) に重なる
    $textOverlayEnd = 300

    Save-WixBitmap -Width 493 -Height 58 -OutputPath $OutputPath -Draw {
        param($Graphics, $Width, $Height)

        $leftRect = [System.Drawing.Rectangle]::new(0, 0, $textOverlayEnd, $Height)
        $leftBrush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 255, 255))
        $Graphics.FillRectangle($leftBrush, $leftRect)
        $leftBrush.Dispose()

        $rightRect = [System.Drawing.Rectangle]::new($textOverlayEnd, 0, $Width - $textOverlayEnd, $Height)
        $background = New-LinearGradientBrush -Rect $rightRect `
            -ColorStart $ColorPrimary `
            -ColorEnd $ColorPrimaryDark `
            -Angle 0
        $Graphics.FillRectangle($background, $rightRect)
        $background.Dispose()

        $accentBrush = New-Object System.Drawing.SolidBrush $ColorAccent
        $Graphics.FillRectangle($accentBrush, $textOverlayEnd, 0, 3, $Height)
        $accentBrush.Dispose()

        $iconSize = 40
        $iconX = $Width - $iconSize - 24
        $iconY = [int](($Height - $iconSize) / 2)
        Draw-AppIcon -Graphics $Graphics -Icon $Icon -X $iconX -Y $iconY -Size $iconSize
    }
}

function New-DialogBitmap {
    param(
        [string]$OutputPath,
        [System.Drawing.Image]$Icon
    )

    # WiX WelcomeDlg: ビットマップ X=11、テキスト X=135 のため左 124px が装飾領域
    $textOverlayStart = 124

    Save-WixBitmap -Width 493 -Height 312 -OutputPath $OutputPath -Draw {
        param($Graphics, $Width, $Height)

        $rightRect = [System.Drawing.Rectangle]::new(
            $textOverlayStart,
            0,
            $Width - $textOverlayStart,
            $Height
        )
        $rightBrush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 255, 255))
        $Graphics.FillRectangle($rightBrush, $rightRect)
        $rightBrush.Dispose()

        $leftRect = [System.Drawing.Rectangle]::new(0, 0, $textOverlayStart, $Height)
        $background = New-LinearGradientBrush -Rect $leftRect `
            -ColorStart $ColorPrimaryDark `
            -ColorEnd $ColorPrimary `
            -Angle 90
        $Graphics.FillRectangle($background, $leftRect)
        $background.Dispose()

        $accentBrush = New-Object System.Drawing.SolidBrush $ColorAccent
        $Graphics.FillRectangle($accentBrush, ($textOverlayStart - 4), 0, 4, $Height)
        $accentBrush.Dispose()

        $iconSize = 88
        $iconX = [int](($textOverlayStart - $iconSize) / 2)
        $iconY = [int](($Height - $iconSize) / 2)
        Draw-AppIcon -Graphics $Graphics -Icon $Icon -X $iconX -Y $iconY -Size $iconSize
    }
}

Invoke-ScriptMain {
    . (Join-Path $PSScriptRoot '..\common\cd-project-root.ps1')

    Add-Type -AssemblyName System.Drawing

    $wixDir = Join-Path $ProjectRoot 'wix'
    if (-not (Test-Path -LiteralPath $wixDir)) {
        New-Item -ItemType Directory -Path $wixDir | Out-Null
    }

    $bannerPath = Join-Path $wixDir 'Banner.bmp'
    $dialogPath = Join-Path $wixDir 'Dialog.bmp'
    $iconPath = Join-Path $ProjectRoot 'assets\icons\icon.png'
    if (-not (Test-Path -LiteralPath $iconPath)) {
        throw "アイコンが見つかりません: $iconPath"
    }

    $icon = [System.Drawing.Image]::FromFile($iconPath)

    try {
        Write-Host 'WiX インストーラー用アセットを生成しています...'
        New-BannerBitmap -OutputPath $bannerPath -Icon $icon
        New-DialogBitmap -OutputPath $dialogPath -Icon $icon

        Write-Host "  $bannerPath"
        Write-Host "  $dialogPath"
        Write-Host '完了'
    }
    finally {
        $icon.Dispose()
    }
}
