<#
.SYNOPSIS
    Обновляет vendor/7zip/<платформа> до указанной версии 7-Zip и пересчитывает хеши.

.DESCRIPTION
    Скачивает официальный архив с github.com/ip7z/7zip, распаковывает его во
    временную папку (в систему ничего не ставится), копирует нужные файлы,
    сверяет SHA-256 и обновляет manifest.json + VERSION.

    windows-x64: NSIS-установщик открывается как архив, берём 7z.exe + 7z.dll.
    linux-x64:   tar.xz, берём единственный статический бинарь 7zz.
    macos-arm64: tar.xz с 7zz для Apple Silicon.
    macos-x64:   tar.xz с 7zz для Intel.

    Работает и в Windows PowerShell 5.1, и в PowerShell 7.

.EXAMPLE
    .\scripts\update-7zip.ps1
    Переустановить запиненную версию и проверить, что файлы не подменены.

.EXAMPLE
    .\scripts\update-7zip.ps1 -Version 26.08 -Force
    Обновиться на новую версию и переписать манифест.

.EXAMPLE
    .\scripts\update-7zip.ps1 -Platform linux-x64
    Переустановить линуксовый 7zz и сверить хеши.
#>
[CmdletBinding()]
param(
    [string]$Version,
    [string]$Platform = "windows-x64",
    [switch]$Force
)

$ErrorActionPreference = "Stop"

# Set-Content -Encoding utf8 в PowerShell 5.1 пишет BOM, из-за которого ломается JSON.
function Write-TextNoBom {
    param([string]$Path, [string]$Text)
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($Path, $Text, $utf8)
}

$root = Split-Path -Parent $PSScriptRoot
$manifestPath = Join-Path $root "vendor\7zip\manifest.json"
$manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
$entry = $manifest.platforms.$Platform

if (-not $entry) { throw "В манифесте нет платформы '$Platform'." }

if (-not $Version) { $Version = $entry.version }
$isSameVersion = ($Version -eq $entry.version)

if (-not $isSameVersion -and -not $Force) {
    throw "Манифест пинует $($entry.version), запрошено $Version. Для обновления добавь -Force."
}

$digits = $Version -replace '\.', ''

switch ($Platform) {
    "windows-x64" {
        $assetName = "7z$digits-x64.exe"
        $targetDir = Join-Path $root "vendor\7zip\win-x64"
    }
    "linux-x64" {
        $assetName = "7z$digits-linux-x64.tar.xz"
        $targetDir = Join-Path $root "vendor\7zip\linux-x64"
    }
    "macos-arm64" {
        $assetName = "7z$digits-mac.tar.xz"
        $targetDir = Join-Path $root "vendor\7zip\macos-arm64"
    }
    "macos-x64" {
        $assetName = "7z$digits-mac-x64.tar.xz"
        $targetDir = Join-Path $root "vendor\7zip\macos-x64"
    }
    default { throw "Неизвестная платформа '$Platform' (ожидали windows-x64, linux-x64, macos-arm64 или macos-x64)." }
}

$url = "https://github.com/ip7z/7zip/releases/download/$Version/$assetName"
$work = Join-Path $env:TEMP ("7zip-vendor-" + [guid]::NewGuid().ToString("N").Substring(0, 8))
$asset = Join-Path $work $assetName
$extracted = Join-Path $work "x"

New-Item -ItemType Directory -Force $work | Out-Null

try {
    Write-Host "Скачиваю $url"
    Invoke-WebRequest -Uri $url -OutFile $asset -UseBasicParsing

    $installerHash = (Get-FileHash $asset -Algorithm SHA256).Hash.ToLower()
    if ($isSameVersion -and $entry.installer_sha256) {
        if ($installerHash -ne $entry.installer_sha256) {
            throw "sha256 архива не совпал с манифестом:`n  ожидали $($entry.installer_sha256)`n  получили $installerHash"
        }
        Write-Host "sha256 архива совпал с манифестом"
    }

    New-Item -ItemType Directory -Force $extracted | Out-Null

    if ($Platform -eq "linux-x64" -or $Platform -like "macos-*") {
        Write-Host "Распаковываю tar.xz (в систему не ставим)"
        tar -xJf $asset -C $extracted
        if ($LASTEXITCODE -ne 0) { throw "tar не смог распаковать $asset (exit $LASTEXITCODE)" }
    } else {
        # Нужен любой рабочий 7z, чтобы распаковать установщик без установки в систему.
        $sevenZip = Join-Path $targetDir "7z.exe"
        if (-not (Test-Path $sevenZip)) {
            $sevenZip = (Get-Command 7z.exe -ErrorAction SilentlyContinue | Select-Object -First 1).Source
        }
        if (-not $sevenZip) {
            throw "Нужен любой 7z.exe (наш vendor/7zip/win-x64 или из PATH), чтобы распаковать установщик."
        }

        Write-Host "Распаковываю установщик (в систему не ставим)"
        & $sevenZip x $asset "-o$extracted" -y | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "7z не смог распаковать установщик (exit $LASTEXITCODE)" }
    }

    $files = @($entry.runtime.files.PSObject.Properties.Name)
    if ($files.Count -eq 0) {
        if ($Platform -eq "windows-x64") { $files = @("7z.exe", "7z.dll", "License.txt") }
        else { $files = @("7zz", "License.txt") }
    }

    # Сначала staging: проверяем хеши ДО замены vendor. При несовпадении
    # запиненные файлы остаются целыми, откат не нужен.
    $staged = Join-Path $work "staged"
    New-Item -ItemType Directory -Force $staged | Out-Null
    foreach ($file in $files) {
        $from = Join-Path $extracted $file
        if (-not (Test-Path $from)) { throw "в распакованном архиве нет '$file'" }
        Copy-Item -LiteralPath $from -Destination $staged -Force
    }

    $hashes = [ordered]@{}
    foreach ($file in $files) {
        $hashes[$file] = (Get-FileHash (Join-Path $staged $file) -Algorithm SHA256).Hash.ToLower()
    }

    if ($isSameVersion) {
        foreach ($file in $hashes.Keys) {
            $pinned = $entry.runtime.files.$file
            if ($pinned -and $pinned -ne $hashes[$file]) {
                throw "sha256 '$file' не совпал с манифестом:`n  ожидали $pinned`n  получили $($hashes[$file])"
            }
        }
    }

    # Верификация пройдена — заменяем vendor.
    New-Item -ItemType Directory -Force $targetDir | Out-Null
    foreach ($file in $files) {
        Copy-Item -LiteralPath (Join-Path $staged $file) -Destination $targetDir -Force
    }
    Write-TextNoBom (Join-Path $targetDir "VERSION") $Version

    # Снимаем Mark of the Web, если он появился: Windows может отказаться запускать
    # помеченный exe. У нас такие потоки не наблюдались, но защита дешёвая.
    if ($Platform -eq "windows-x64") {
        Get-ChildItem $targetDir -File | ForEach-Object {
            Unblock-File -LiteralPath $_.FullName -ErrorAction SilentlyContinue
        }
    }

    $entry.version = $Version
    $entry.build = $assetName
    $entry.source_url = $url
    $entry.installer_sha256 = $installerHash
    $entry.runtime.files = $hashes
    Write-TextNoBom $manifestPath ($manifest | ConvertTo-Json -Depth 10)

    Write-Host "Готово: vendor/7zip/$Platform -> $Version"
    foreach ($file in $hashes.Keys) { Write-Host ("  {0,-12} {1}" -f $file, $hashes[$file]) }
    Write-Host "Не забудь закоммитить vendor/7zip и manifest.json."
}
finally {
    Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
}
