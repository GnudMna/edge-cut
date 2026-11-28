# ========================================================================
# Script Name : verify-msi.ps1
# Description : ビルド済み MSI の内容・メタデータを検証する (ドットソース用)
# Usage       : . (Join-Path $PSScriptRoot '..\common\verify-msi.ps1')
#               Invoke-MsiPackageVerification -ProjectRoot $ProjectRoot [-MsiPath <path>] [-SkipExtract]
# Requires    : PowerShell 7.0+, Windows (Windows Installer COM)
# ========================================================================

#requires -Version 7.0

Set-StrictMode -Version Latest

# ======================================================================
# 定数
# ======================================================================

$script:MsiExpectedProductName = 'EdgeCut'
$script:MsiExpectedManufacturer = 'Yuto Hirata'
$script:MsiExpectedLanguageId = '1041'
# MSI の Platform (Summary Information の Template) と実行ファイルの PE Machine (AMD64)
$script:MsiExpectedPlatform = 'x64'
$script:PeMachineAmd64 = 0x8664
$script:MsiRequiredFileNames = @(
    'EdgeCut.exe',
    'EdgeCutUpdater.exe',
    'LICENSE',
    'THIRD_PARTY_NOTICES',
    'index.html'
)
$script:MsiRequiredFeatureIds = @(
    'Binaries',
    'Environment'
)
$script:MsiExecutableNames = @(
    'EdgeCut.exe',
    'EdgeCutUpdater.exe'
)
$script:MsiThirdPartyNoticesHeader = '第三者ソフトウェアに関する表示'
$script:MsiMinExecutableBytes = 512 * 1024
$script:MsiMinThirdPartyNoticesBytes = 1024

# ======================================================================
# リポジトリから期待値を読み取る
# ======================================================================

# Cargo.toml の [package] version を読み取る
function Get-MsiExpectedVersion {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$ProjectRoot
    )

    $content = Get-Content -LiteralPath (Join-Path $ProjectRoot 'Cargo.toml') -Raw -Encoding utf8
    if ($content -match '(?m)^version\s*=\s*"([^"]+)"') {
        return $Matches[1]
    }

    throw 'エラー: Cargo.toml から version を読み取れませんでした'
}

# Cargo.toml の [package.metadata.wix] upgrade-guid を読み取る
function Get-CargoUpgradeGuid {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$ProjectRoot
    )

    $content = Get-Content -LiteralPath (Join-Path $ProjectRoot 'Cargo.toml') -Raw -Encoding utf8
    if ($content -match '(?m)^upgrade-guid\s*=\s*"([^"]+)"') {
        return $Matches[1]
    }

    throw 'エラー: Cargo.toml から upgrade-guid を読み取れませんでした'
}

# wix/main.wxs の UpgradeCode を読み取る
function Get-WixUpgradeCode {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$ProjectRoot
    )

    $content = Get-Content -LiteralPath (Join-Path $ProjectRoot 'wix\main.wxs') -Raw -Encoding utf8
    if ($content -match "UpgradeCode='([^']+)'") {
        return $Matches[1]
    }

    throw 'エラー: wix/main.wxs から UpgradeCode を読み取れませんでした'
}

# 利用者向けドキュメントの目次 (docs/user/SUMMARY.md) から、出力される HTML のページ名を求める
function Get-UserDocPageNames {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$ProjectRoot
    )

    $summary = Get-Content -LiteralPath (Join-Path $ProjectRoot 'docs\user\SUMMARY.md') -Raw -Encoding utf8
    $pages = [System.Collections.Generic.List[string]]::new()
    $pages.Add('index.html')
    foreach ($match in [regex]::Matches($summary, '\]\(([^)#]+)\.md\)')) {
        $pages.Add(($match.Groups[1].Value -replace '/', '\') + '.html')
    }

    if ($pages.Count -le 1) {
        throw 'エラー: docs/user/SUMMARY.md からページを読み取れませんでした'
    }

    return [string[]]$pages
}

# 検証する MSI のパスを決める (省略時は target/wix から指定バージョンの MSI を 1 つ選ぶ)
function Resolve-BuiltMsiPath {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$ProjectRoot,

        [Parameter(Mandatory = $true)]
        [string]$Version,

        [string]$MsiPath
    )

    if ($MsiPath) {
        if (-not (Test-Path -LiteralPath $MsiPath -PathType Leaf)) {
            throw "エラー: 指定した MSI が見つかりません: $MsiPath"
        }

        return (Resolve-Path -LiteralPath $MsiPath).Path
    }

    $wixDir = Join-Path $ProjectRoot 'target\wix'
    if (-not (Test-Path -LiteralPath $wixDir)) {
        throw 'エラー: target\wix が見つかりません。先に build-msi.ps1 を実行してください'
    }

    # cargo-wix の既定名は `{パッケージ名}-{バージョン}-{アーキテクチャ}.msi`
    $escapedVersion = [regex]::Escape($Version)
    $candidates = @(Get-ChildItem -LiteralPath $wixDir -Filter 'edge-cut-*.msi' -File |
        Where-Object { $_.Name -match "^edge-cut-$escapedVersion-[^-]+\.msi$" })

    if ($candidates.Count -eq 0) {
        throw "エラー: バージョン $Version の MSI が見つかりません ($wixDir)"
    }
    if ($candidates.Count -gt 1) {
        $names = ($candidates | ForEach-Object { $_.Name }) -join ', '
        throw "エラー: バージョン $Version に一致する MSI が複数あります (-MsiPath で指定してください): $names"
    }

    return $candidates[0].FullName
}

# ======================================================================
# Windows Installer データベース
# ======================================================================

# MSI データベースを読み取り専用で開く
function Open-MsiDatabase {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    $installer = New-Object -ComObject WindowsInstaller.Installer
    # 0 = msiOpenDatabaseModeReadOnly
    return $installer.OpenDatabase($Path, 0)
}

# MSI の文字列から NUL と前後の空白を除く
function Get-NormalizedMsiString {
    [CmdletBinding()]
    param(
        [AllowNull()]
        [AllowEmptyString()]
        [string]$Value
    )

    if ([string]::IsNullOrEmpty($Value)) {
        return [string]::Empty
    }

    return $Value.Replace([string][char]0, [string]::Empty).Trim()
}

# SQL を実行し、各レコードの 1 列目を文字列の配列で返す
function Get-MsiQueryColumn {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        $Database,

        [Parameter(Mandatory = $true)]
        [string]$Sql
    )

    $values = [System.Collections.Generic.List[string]]::new()
    $view = $Database.OpenView($Sql)
    try {
        [void]$view.Execute()
        while ($true) {
            $record = $view.Fetch()
            if ($null -eq $record) {
                break
            }
            $values.Add((Get-NormalizedMsiString -Value ([string]$record.StringData(1))))
        }
    } finally {
        [void]$view.Close()
    }

    return [string[]]$values
}

# Property テーブルの値を返す (無ければ空文字列)
function Get-MsiPropertyValue {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        $Database,

        [Parameter(Mandatory = $true)]
        [string]$PropertyName
    )

    $escapedName = $PropertyName.Replace("'", "''")
    $values = @(Get-MsiQueryColumn -Database $Database -Sql "SELECT Value FROM Property WHERE Property = '$escapedName'")
    if ($values.Count -eq 0) {
        return [string]::Empty
    }

    return $values[0]
}

# 指定テーブルの主キーに値があるかを返す (テーブル自体が無い場合も偽)
function Test-MsiTableRow {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        $Database,

        [Parameter(Mandatory = $true)]
        [string]$Table,

        [Parameter(Mandatory = $true)]
        [string]$Column,

        [Parameter(Mandatory = $true)]
        [string]$Value
    )

    $tables = @(Get-MsiQueryColumn -Database $Database -Sql "SELECT Name FROM _Tables WHERE Name = '$Table'")
    if ($tables.Count -eq 0) {
        return $false
    }

    $escapedValue = $Value.Replace("'", "''")
    $rows = @(Get-MsiQueryColumn -Database $Database -Sql "SELECT $Column FROM $Table WHERE $Column = '$escapedValue'")
    return $rows.Count -gt 0
}

# File テーブルのファイル名一覧を返す (短い名前|長い名前 の形式は長い名前にする)
function Get-MsiFileNames {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        $Database
    )

    $names = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
    foreach ($fileName in Get-MsiQueryColumn -Database $Database -Sql 'SELECT FileName FROM File') {
        $null = $names.Add($fileName.Split('|')[-1])
    }

    return , $names
}

# GUID 文字列を比較用に正規化する
function Get-NormalizedGuid {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$Value
    )

    return $Value.Trim().Trim('{', '}').ToUpperInvariant()
}

# ======================================================================
# 検証
# ======================================================================

# Cargo.toml の upgrade-guid と wix/main.wxs の UpgradeCode の一致を検証する
function Assert-WixMetadataConsistency {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$ProjectRoot
    )

    $cargoGuid = Get-NormalizedGuid -Value (Get-CargoUpgradeGuid -ProjectRoot $ProjectRoot)
    $wixGuid = Get-NormalizedGuid -Value (Get-WixUpgradeCode -ProjectRoot $ProjectRoot)
    if ($cargoGuid -ne $wixGuid) {
        throw @(
            'エラー: UpgradeCode が一致しません',
            "  Cargo.toml upgrade-guid: $cargoGuid",
            "  wix/main.wxs UpgradeCode: $wixGuid"
        ) -join [Environment]::NewLine
    }
}

# MSI のプロパティ・ファイル・Feature・画面を検証する
function Assert-MsiDatabaseContents {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$MsiPath,

        [Parameter(Mandatory = $true)]
        [string]$ExpectedVersion,

        [Parameter(Mandatory = $true)]
        [string]$ExpectedUpgradeCode
    )

    $database = Open-MsiDatabase -Path $MsiPath
    try {
        $expectedProperties = [ordered]@{
            ProductName = $script:MsiExpectedProductName
            ProductVersion = $ExpectedVersion
            Manufacturer = $script:MsiExpectedManufacturer
            ProductLanguage = $script:MsiExpectedLanguageId
        }
        foreach ($name in $expectedProperties.Keys) {
            $actual = Get-MsiPropertyValue -Database $database -PropertyName $name
            if ($actual -cne $expectedProperties[$name]) {
                throw "エラー: MSI の $name が期待値と異なります (実際: '$actual', 期待: '$($expectedProperties[$name])')"
            }
        }

        $upgradeCode = Get-MsiPropertyValue -Database $database -PropertyName 'UpgradeCode'
        if ((Get-NormalizedGuid -Value $upgradeCode) -ne (Get-NormalizedGuid -Value $ExpectedUpgradeCode)) {
            throw "エラー: MSI の UpgradeCode が期待値と異なります (実際: '$upgradeCode', 期待: '$ExpectedUpgradeCode')"
        }

        # per-user インストール (InstallScope='perUser') では ALLUSERS を設定しない
        $allUsers = Get-MsiPropertyValue -Database $database -PropertyName 'ALLUSERS'
        if ($allUsers -and $allUsers -ne '2') {
            throw "エラー: per-user インストールを想定していますが ALLUSERS=$allUsers です (期待: 未設定または 2)"
        }

        # Summary Information の Template (PID_TEMPLATE = 7) は `{Platform};{LanguageId}`
        $summary = $database.SummaryInformation(0)
        $template = Get-NormalizedMsiString -Value ([string]$summary.Property(7))
        $expectedTemplate = "$($script:MsiExpectedPlatform);$($script:MsiExpectedLanguageId)"
        if ($template -ne $expectedTemplate) {
            throw "エラー: MSI の Platform / 言語 (Template) が期待値と異なります (実際: '$template', 期待: '$expectedTemplate')"
        }

        $fileNames = Get-MsiFileNames -Database $database
        foreach ($requiredName in $script:MsiRequiredFileNames) {
            if (-not $fileNames.Contains($requiredName)) {
                throw "エラー: MSI の File テーブルに $requiredName が含まれていません"
            }
        }

        foreach ($requiredFeature in $script:MsiRequiredFeatureIds) {
            if (-not (Test-MsiTableRow -Database $database -Table 'Feature' -Column 'Feature' -Value $requiredFeature)) {
                throw "エラー: MSI の Feature テーブルに $requiredFeature が含まれていません"
            }
        }

        # MIT License は同意を要しないため、ライセンス同意画面を出さない (wix/main.wxs の独自 UI)
        if (Test-MsiTableRow -Database $database -Table 'Dialog' -Column 'Dialog' -Value 'LicenseAgreementDlg') {
            throw 'エラー: MSI にライセンス同意画面 (LicenseAgreementDlg) が含まれています'
        }

        # 完了画面の「EdgeCut を起動する」チェックボックスと、その起動処理
        if (-not (Test-MsiTableRow -Database $database -Table 'CustomAction' -Column 'Action' -Value 'LaunchApplication')) {
            throw 'エラー: MSI に完了画面からアプリを起動するカスタムアクション (LaunchApplication) がありません'
        }
        $checkboxText = Get-MsiPropertyValue -Database $database -PropertyName 'WIXUI_EXITDIALOGOPTIONALCHECKBOXTEXT'
        if (-not $checkboxText) {
            throw 'エラー: MSI の完了画面に起動のチェックボックス (WIXUI_EXITDIALOGOPTIONALCHECKBOXTEXT) がありません'
        }
    } finally {
        [void][System.Runtime.InteropServices.Marshal]::ReleaseComObject($database)
    }
}

# 実行ファイルの PE ヘッダーの Machine が x64 (AMD64) かを検証する
function Assert-PeMachineAmd64 {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    $stream = [System.IO.File]::OpenRead($Path)
    try {
        $reader = [System.IO.BinaryReader]::new($stream)
        if ($reader.ReadUInt16() -ne 0x5A4D) {
            throw "エラー: $Path は PE ファイルではありません (MZ ヘッダーがありません)"
        }

        # e_lfanew (0x3C) が PE ヘッダーの位置。署名 `PE\0\0` の直後が Machine
        $stream.Position = 0x3C
        $stream.Position = $reader.ReadInt32()
        if ($reader.ReadUInt32() -ne 0x00004550) {
            throw "エラー: $Path は PE ファイルではありません (PE 署名がありません)"
        }

        $machine = $reader.ReadUInt16()
        if ($machine -ne $script:PeMachineAmd64) {
            throw ('エラー: {0} の PE Machine が x64 ではありません (実際: 0x{1:X4}, 期待: 0x{2:X4})' -f $Path, $machine, $script:PeMachineAmd64)
        }
    } finally {
        $stream.Dispose()
    }
}

# MSI を管理インストール (msiexec /a) で展開する
function Expand-MsiAdministrative {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$MsiPath,

        [Parameter(Mandatory = $true)]
        [string]$TargetDir
    )

    $null = New-Item -ItemType Directory -Path $TargetDir -Force

    $arguments = @('/a', "`"$MsiPath`"", '/qn', "TARGETDIR=`"$TargetDir`"")
    $process = Start-Process -FilePath 'msiexec.exe' -ArgumentList $arguments -Wait -PassThru -NoNewWindow
    if ($process.ExitCode -ne 0) {
        throw "エラー: msiexec /a が終了コード $($process.ExitCode) で失敗しました"
    }
}

# 展開したファイル (実行ファイル・第三者ライセンス表示・利用者向けドキュメント) を検証する
function Assert-ExtractedMsiContents {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$ExtractDir,

        [Parameter(Mandatory = $true)]
        [string]$ProjectRoot
    )

    # 配置は wix/main.wxs の %LOCALAPPDATA%\edge-cut 以下と同じ (bin\ に実行ファイル、直下に文書)
    $appDirs = @(Get-ChildItem -LiteralPath $ExtractDir -Recurse -Directory -Filter 'edge-cut')
    if ($appDirs.Count -ne 1) {
        throw "エラー: 展開結果にインストール先 (edge-cut) が見つかりません ($ExtractDir)"
    }
    $appDir = $appDirs[0].FullName

    foreach ($executableName in $script:MsiExecutableNames) {
        $executablePath = Join-Path $appDir "bin\$executableName"
        if (-not (Test-Path -LiteralPath $executablePath -PathType Leaf)) {
            throw "エラー: 展開結果に bin\$executableName がありません ($appDir)"
        }

        $length = (Get-Item -LiteralPath $executablePath).Length
        if ($length -lt $script:MsiMinExecutableBytes) {
            throw "エラー: $executableName のサイズが小さすぎます ($length bytes, 最低 $($script:MsiMinExecutableBytes) bytes)"
        }

        Assert-PeMachineAmd64 -Path $executablePath
    }

    if (-not (Test-Path -LiteralPath (Join-Path $appDir 'LICENSE') -PathType Leaf)) {
        throw "エラー: 展開結果に LICENSE がありません ($appDir)"
    }

    $noticesPath = Join-Path $appDir 'THIRD_PARTY_NOTICES'
    if (-not (Test-Path -LiteralPath $noticesPath -PathType Leaf)) {
        throw "エラー: 展開結果に THIRD_PARTY_NOTICES がありません ($appDir)"
    }
    $notices = Get-Item -LiteralPath $noticesPath
    if ($notices.Length -lt $script:MsiMinThirdPartyNoticesBytes) {
        throw "エラー: THIRD_PARTY_NOTICES のサイズが小さすぎます ($($notices.Length) bytes)"
    }
    $noticesContent = Get-Content -LiteralPath $noticesPath -Raw -Encoding utf8
    if (-not $noticesContent.Contains($script:MsiThirdPartyNoticesHeader)) {
        throw 'エラー: THIRD_PARTY_NOTICES に期待するヘッダーが含まれていません'
    }

    # ビルド時に生成した THIRD_PARTY_NOTICES と同じものが入っているか (古い生成物の取り違えを検出)
    $sourceNoticesPath = Join-Path $ProjectRoot 'THIRD_PARTY_NOTICES'
    if (Test-Path -LiteralPath $sourceNoticesPath -PathType Leaf) {
        $sourceHash = (Get-FileHash -LiteralPath $sourceNoticesPath -Algorithm SHA256).Hash
        $msiHash = (Get-FileHash -LiteralPath $noticesPath -Algorithm SHA256).Hash
        if ($sourceHash -ne $msiHash) {
            throw 'エラー: MSI 内の THIRD_PARTY_NOTICES がビルド時の生成物と一致しません'
        }
    }

    $docsDir = Join-Path $appDir 'docs'
    foreach ($page in Get-UserDocPageNames -ProjectRoot $ProjectRoot) {
        if (-not (Test-Path -LiteralPath (Join-Path $docsDir $page) -PathType Leaf)) {
            throw "エラー: 利用者向けドキュメントに $page がありません ($docsDir)"
        }
    }
}

# ビルド済み MSI を一括検証する
function Invoke-MsiPackageVerification {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$ProjectRoot,

        [string]$MsiPath,

        [string]$Version,

        [switch]$SkipExtract
    )

    if (-not $Version) {
        $Version = Get-MsiExpectedVersion -ProjectRoot $ProjectRoot
    }

    Write-Host 'MSI パッケージを検証しています...'
    Write-Host "  対象バージョン: $Version"

    Assert-WixMetadataConsistency -ProjectRoot $ProjectRoot
    Write-Host '  Cargo.toml と wix/main.wxs の UpgradeCode が一致'

    $resolvedMsiPath = Resolve-BuiltMsiPath -ProjectRoot $ProjectRoot -Version $Version -MsiPath $MsiPath
    Write-Host "  検証対象: $resolvedMsiPath"

    Assert-MsiDatabaseContents `
        -MsiPath $resolvedMsiPath `
        -ExpectedVersion $Version `
        -ExpectedUpgradeCode (Get-CargoUpgradeGuid -ProjectRoot $ProjectRoot)
    Write-Host '  ProductName / ProductVersion / Manufacturer / 言語 / UpgradeCode / Platform (x64)'
    Write-Host '  per-user インストール (ALLUSERS)'
    Write-Host "  必須ファイル ($($script:MsiRequiredFileNames -join ', '))"
    Write-Host "  必須 Feature ($($script:MsiRequiredFeatureIds -join ', '))"
    Write-Host '  ライセンス同意画面が無く、完了画面に起動のチェックボックスがあること'

    if ($SkipExtract) {
        Write-Host '  展開による検証はスキップしました (-SkipExtract)'
        Write-Host 'MSI の検証が完了しました'
        return
    }

    $extractDir = Join-Path ([System.IO.Path]::GetTempPath()) ('edge-cut-msi-verify-{0}' -f [Guid]::NewGuid())
    try {
        Expand-MsiAdministrative -MsiPath $resolvedMsiPath -TargetDir $extractDir
        Assert-ExtractedMsiContents -ExtractDir $extractDir -ProjectRoot $ProjectRoot
        Write-Host '  展開した実行ファイルのサイズと PE アーキテクチャ (x64)'
        Write-Host '  LICENSE / THIRD_PARTY_NOTICES (ヘッダーとビルド時の生成物との一致)'
        Write-Host '  利用者向けドキュメント (docs/user/SUMMARY.md の各ページ)'
    } finally {
        if (Test-Path -LiteralPath $extractDir) {
            Remove-Item -LiteralPath $extractDir -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    Write-Host 'MSI の検証が完了しました'
}
