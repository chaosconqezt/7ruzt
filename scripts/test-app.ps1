<#
.SYNOPSIS
    Быстрый раннер для тестирования и отладки archiver-app без эмуляции мыши и скриншотов.

.DESCRIPTION
    Запускает приложение с тестовыми флагами, выполняет внутренние действия окна
    (выделение, нажатия клавиш, открытие диалогов) и возвращает машиночитаемое состояние
    (target/app-state.json) прямо в консоль.

.EXAMPLE
    .\scripts\test-app.ps1 -Select "0,2" -Press "f5"
    .\scripts\test-app.ps1 -Arguments @("C:\tmp") -Press "ctrl+n"
#>
[CmdletBinding()]
param(
    # Индексы для выделения при старте
    [string]$Select = "",
    # Клавиши для нажатия (через запятую, например "f5,escape" или "ctrl+a")
    [string]$Press = "",
    # Открыть диалог создания архива
    [switch]$Add,
    # Создать архив сразу
    [switch]$CreateNow,
    # Открыть диалог распаковки
    [switch]$Extract,
    # Проверить архив сразу
    [switch]$TestNow,
    # Открыть две панели (F9)
    [switch]$TwoPanel,
    # Путь для второй панели
    [string]$Right = "",
    # Задержка перед завершением (мс)
    [int]$WaitMs = 250,
    # Путь для дампа состояния
    [string]$DumpFile = "target\app-state.json",
    # Дополнительные аргументы (путь к папке/архиву)
    [string[]]$Arguments = @(),
    # Пересобрать бинарник перед запуском
    [switch]$Build
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

if ($Build) {
    Stop-Process -Name archiver-app -ErrorAction SilentlyContinue
    cmd /c "cd /d `"$root`" && cargo build -p archiver-app 2>&1"
    if ($LASTEXITCODE -ne 0) { throw "сборка завершилась ошибкой $LASTEXITCODE" }
}

$exe = Join-Path $root "target\debug\archiver-app.exe"
if (-not (Test-Path $exe)) {
    throw "нет $exe. Сначала соберите: cargo build -p archiver-app"
}

# Подкладываем 7zip рядом с exe, если сборка его ещё не положила
$vendor = Join-Path $root "vendor\7zip\win-x64"
$runtime = Split-Path -Parent $exe
if (Test-Path $vendor) {
    Copy-Item (Join-Path $vendor "7z.exe") $runtime -Force -ErrorAction SilentlyContinue
    Copy-Item (Join-Path $vendor "7z.dll") $runtime -Force -ErrorAction SilentlyContinue
}

$dumpTarget = Join-Path $root $DumpFile
if (Test-Path $dumpTarget) { Remove-Item $dumpTarget -Force }

$cmdArgs = @()
if ($Select) { $cmdArgs += "--select=$Select" }
if ($Press) { $cmdArgs += "--press=$Press" }
if ($Add) { $cmdArgs += "--add" }
if ($CreateNow) { $cmdArgs += "--create-now" }
if ($Extract) { $cmdArgs += "--extract" }
if ($TestNow) { $cmdArgs += "--test-now" }
if ($TwoPanel) { $cmdArgs += "--two-panel" }
if ($Right) { $cmdArgs += "--right=$Right" }
$cmdArgs += "--dump-state=$dumpTarget"
$cmdArgs += "--quit-after=$WaitMs"

if ($Arguments) {
    $cmdArgs += $Arguments
}

$env:ARCHIVER_TEST_FLAGS = "1"
$process = Start-Process -FilePath $exe -ArgumentList $cmdArgs -PassThru

$stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
while (-not $process.HasExited -and $stopwatch.ElapsedMilliseconds -lt 6000) {
    Start-Sleep -Milliseconds 50
    $process.Refresh()
}

if (-not $process.HasExited) {
    Stop-Process -Id $process.Id -Force
}

if (Test-Path $dumpTarget) {
    $raw = Get-Content $dumpTarget -Raw
    Write-Host "State Dump:" -ForegroundColor Cyan
    try {
        $parsed = $raw | ConvertFrom-Json
        $parsed | Format-List
    } catch {
        Write-Host $raw
    }
} else {
    Write-Warning "Файл дампа состояния $dumpTarget не был создан"
}

