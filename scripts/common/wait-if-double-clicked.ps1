# ========================================================================
# Script Name : wait-if-double-clicked.ps1
# Description : エクスプローラーからのダブルクリック実行時にウィンドウを開いたままにする
# Usage       : . (Join-Path $PSScriptRoot '..\common\wait-if-double-clicked.ps1')
#               Invoke-ScriptMain { ... }
# Requires    : PowerShell 7.0+
# ========================================================================

#requires -Version 7.0

# エクスプローラー経由の起動かどうかを直接の親プロセスで判定する
# (祖先まで辿ると、スタートメニュー等から起動したターミナル配下でも常に true になる)
function Test-LaunchedFromExplorer {
    try {
        $self = Get-CimInstance Win32_Process -Filter "ProcessId=$PID" -ErrorAction Stop
        if (-not $self -or $self.ParentProcessId -eq 0) {
            return $false
        }

        $parent = Get-CimInstance Win32_Process -Filter "ProcessId=$($self.ParentProcessId)" -ErrorAction Stop
        if (-not $parent) {
            return $false
        }

        $name = $parent.Name.ToLowerInvariant()
        return ($name -eq 'explorer.exe' -or $name -eq 'openwith.exe')
    } catch {
        return $false
    }
}

# ダブルクリック実行時のみ Enter 待ちでウィンドウを開いたままにする
function Wait-IfDoubleClicked {
    # 入力がリダイレクトされた非対話の実行では待たない
    if ([Console]::IsInputRedirected) {
        return
    }

    if (-not (Test-LaunchedFromExplorer)) {
        return
    }

    Write-Host ''

    # -NonInteractive などで Read-Host が使えなくても、終了コードには影響させない
    try {
        Read-Host '終了するには Enter キーを押してください' | Out-Null
    } catch {
        return
    }
}

# スクリプト本体を実行し、エラー時も Enter 待ち後に静かに終了する
function Invoke-ScriptMain {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [scriptblock]$Body
    )

    if (-not (Get-Variable -Name EdgeCutScriptMainDepth -Scope Global -ErrorAction SilentlyContinue)) {
        $global:EdgeCutScriptMainDepth = 0
    }

    $global:EdgeCutScriptMainDepth++
    $isOuterInvocation = $global:EdgeCutScriptMainDepth -eq 1
    $exitCode = 0

    try {
        & $Body

        if ((Test-Path variable:LASTEXITCODE) -and $LASTEXITCODE -ne 0) {
            $exitCode = $LASTEXITCODE
        }
    } catch {
        $exitCode = 1
        Write-Host ''

        if ($_.Exception.Message) {
            Write-Host $_.Exception.Message
        } else {
            Write-Host $_
        }
    } finally {
        $global:EdgeCutScriptMainDepth--

        # 他スクリプトから呼ばれた場合は Enter 待ちしない (最外の直接実行時のみ)
        if ($isOuterInvocation) {
            Wait-IfDoubleClicked
        }
    }

    if ($exitCode -ne 0) {
        exit $exitCode
    }
}
