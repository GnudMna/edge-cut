# ========================================================================
# Script Name : license-allowlist-check.ps1
# Description : deny.toml と about.toml の許可ライセンス・ターゲットの差分を検出
# Usage       : . (Join-Path $PSScriptRoot 'license-allowlist-check.ps1')
# Requires    : PowerShell 7.0+
# ========================================================================

#requires -Version 7.0

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# 配列ブロック内のコメント行を除いた、ダブルクォート文字列の一覧を返す
function Get-TomlQuotedValues {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$Text,

        [Parameter(Mandatory = $true)]
        [string]$Pattern,

        [Parameter(Mandatory = $true)]
        [string]$Label
    )

    $match = [regex]::Match($Text, $Pattern, 'Multiline, Singleline')
    if (-not $match.Success) {
        throw "エラー: $Label が見つかりません"
    }

    $values = foreach ($line in $match.Groups[1].Value -split '\r?\n') {
        if ($line.TrimStart().StartsWith('#')) {
            continue
        }
        foreach ($quoted in [regex]::Matches($line, '"([^"]+)"')) {
            $quoted.Groups[1].Value
        }
    }
    return @($values | Sort-Object -Unique)
}

# 2 つの一覧を比較し、差分があれば例外を投げる
function Assert-SameSet {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name,

        [AllowEmptyCollection()]
        [string[]]$Deny,

        [AllowEmptyCollection()]
        [string[]]$About
    )

    $onlyDeny = @($Deny | Where-Object { $_ -notin $About })
    $onlyAbout = @($About | Where-Object { $_ -notin $Deny })
    if ($onlyDeny.Count -eq 0 -and $onlyAbout.Count -eq 0) {
        return
    }

    $lines = @("エラー: deny.toml と about.toml の $Name が一致しません")
    if ($onlyDeny.Count -gt 0) {
        $lines += "  deny.toml のみ: $($onlyDeny -join ', ')"
    }
    if ($onlyAbout.Count -gt 0) {
        $lines += "  about.toml のみ: $($onlyAbout -join ', ')"
    }
    throw $lines -join [Environment]::NewLine
}

$denyText = Get-Content -LiteralPath (Join-Path $PWD 'deny.toml') -Raw
$aboutText = Get-Content -LiteralPath (Join-Path $PWD 'about.toml') -Raw

$denyLicenses = Get-TomlQuotedValues -Text $denyText -Pattern '^allow\s*=\s*\[(.*?)^\]' -Label 'deny.toml の licenses.allow'
$aboutLicenses = Get-TomlQuotedValues -Text $aboutText -Pattern '^accepted\s*=\s*\[(.*?)^\]' -Label 'about.toml の accepted'
Assert-SameSet -Name '許可ライセンス' -Deny $denyLicenses -About $aboutLicenses

$denyTargets = Get-TomlQuotedValues -Text $denyText -Pattern '^targets\s*=\s*\[(.*?)^\]' -Label 'deny.toml の graph.targets'
$aboutTargets = Get-TomlQuotedValues -Text $aboutText -Pattern '^targets\s*=\s*\[(.*?)^\]' -Label 'about.toml の targets'
Assert-SameSet -Name 'ターゲット' -Deny $denyTargets -About $aboutTargets
