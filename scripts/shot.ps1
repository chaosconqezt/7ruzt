<#
.SYNOPSIS
    Запускает приложение, снимает скрин его окна и закрывает процесс.

.DESCRIPTION
    Нужен, чтобы проверять, как реально выглядит окно, не глядя в монитор:
    сборка идёт в песочнице, глаз у человека один, а итераций будет много.

    Три вещи, без которых скрин выходит не тем:
      * DPI-awareness — иначе GetWindowRect отдаёт виртуальные координаты,
        а CopyFromScreen снимает физические пиксели, и кроп уезжает;
      * редиректы stdout/stderr — иначе GUI-процесс наследует pipe обёртки
        и она не завершается;
      * окно должно быть GUI-подсистемы (см. windows_subsystem в app/src/main.rs),
        иначе MainWindowHandle цепляет консоль, а не окно приложения.

.EXAMPLE
    .\scripts\shot.ps1
    .\scripts\shot.ps1 -Out preview-linux.png -Wait 1500
#>
[CmdletBinding()]
param(
    [string]$Out = "preview.png",
    [int]$Wait = 800,
    [string]$Exe = "",
    [string[]]$Arguments = @(),
    [int]$ClickX = -1,
    [int]$ClickY = -1,
    [switch]$DoubleClick,
    # Клавиши после запуска (формат WScript.Shell.SendKeys: "{ENTER}", "^a", "{TAB}").
    [string]$Keys = "",
    # Пауза после нажатия клавиш: 7z на листинге отвечает за 20–40 мс, но кадру
    # нужно ещё отрисоваться, поэтому по умолчанию 600 мс.
    [int]$KeyWait = 600,
    # Пересобрать приложение перед запуском: одна команда вместо двух.
    [switch]$Build,
    # Протяжка мышью внутри окна: "x1,y1,x2,y2" (окно-координаты).
    [string]$Drag = "",
    # Навести курсор без клика: "x,y" (окно-координаты).
    [string]$Hover = "",
    # Не отпускать кнопку после протяжки — снимок посреди жеста (видна рамка).
    [switch]$DragHold
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

# `-Build`: убить запущенный экземпляр (он держит exe и мешает линковке) и собрать.
# Вывод cargo отдаём через cmd: в PowerShell 5.1 `2>&1` на нативной команде рождает
# ErrorRecord, и при ErrorActionPreference=Stop успешная сборка считалась бы падением.
if ($Build) {
    Stop-Process -Name archiver-app -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 300
    $log = cmd /c "cd /d `"$root`" && cargo build -p archiver-app 2>&1"
    if ($LASTEXITCODE -ne 0) {
        $log | Write-Host
        throw "сборка не прошла (код $LASTEXITCODE)"
    }
    Write-Host "сборка: ok"
}

if (-not $Exe) { $Exe = Join-Path $root "target\debug\archiver-app.exe" }
if (-not (Test-Path $Exe)) {
    throw "нет '$Exe'. Сначала соберите: cargo build -p archiver-app"
}

# Раскладка как в поставке: 7z.exe лежит рядом с исполняемым файлом программы.
# (build.rs делает то же самое; копия — страховка для старых сборок.)
$vendor = Join-Path $root "vendor\7zip\win-x64"
$runtime = Split-Path -Parent $Exe
if (Test-Path $vendor) {
    Copy-Item (Join-Path $vendor "7z.exe") $runtime -Force
    Copy-Item (Join-Path $vendor "7z.dll") $runtime -Force
}

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class ShotWin32 {
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
}
'@
[ShotWin32]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null

# Клик по координатам внутри окна: нужен, чтобы проверять выделение и сортировку
# без человека у монитора. Координаты — в пикселях от левого верхнего угла окна.
function Invoke-Click([int]$x, [int]$y) {
    [ShotWin32]::SetCursorPos($x, $y) | Out-Null
    Start-Sleep -Milliseconds 120
    [ShotWin32]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
    [ShotWin32]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
}

# Протяжка мышью: рамка выделения проверяется только настоящим движением с
# зажатой кнопкой. Движемся шагами — приложение получает несколько mouse_move,
# как при живой руке, иначе рамка «прыгает» одним событием.
function Invoke-Drag([int]$x1, [int]$y1, [int]$x2, [int]$y2, [bool]$hold = $false) {
    $steps = 8
    [ShotWin32]::SetCursorPos($x1, $y1) | Out-Null
    Start-Sleep -Milliseconds 120
    [ShotWin32]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 80
    for ($i = 1; $i -le $steps; $i++) {
        $x = $x1 + [int](($x2 - $x1) * $i / $steps)
        $y = $y1 + [int](($y2 - $y1) * $i / $steps)
        [ShotWin32]::SetCursorPos($x, $y) | Out-Null
        Start-Sleep -Milliseconds 35
    }
    if ($hold) {
        # Кнопку не отпускаем: скрин снимается посреди протяжки, видна рамка.
        Start-Sleep -Milliseconds 200
        return
    }
    [ShotWin32]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 250
}

Add-Type -AssemblyName System.Drawing

$process = $null
try {
    # Флаги автоматизации приложения включаются только с этой переменной.
    $env:ARCHIVER_TEST_FLAGS = "1"

    $startArgs = @{
        FilePath               = $Exe
        PassThru               = $true
        RedirectStandardOutput = Join-Path $root "target\app-out.log"
        RedirectStandardError  = Join-Path $root "target\app-err.log"
    }
    if ($Arguments.Count -gt 0) { $startArgs.ArgumentList = $Arguments }
    $process = Start-Process @startArgs

    $handle = [IntPtr]::Zero
    for ($i = 0; $i -lt 60; $i++) {
        Start-Sleep -Milliseconds 200
        $process.Refresh()
        if ($process.HasExited) { throw "приложение упало, код $($process.ExitCode)" }
        if ($process.MainWindowHandle -ne [IntPtr]::Zero) { $handle = $process.MainWindowHandle; break }
    }
    if ($handle -eq [IntPtr]::Zero) { throw "окно не появилось за 12 с" }

    [ShotWin32]::SetForegroundWindow($handle) | Out-Null
    Start-Sleep -Milliseconds $Wait

    $rect = New-Object ShotWin32+RECT
    [ShotWin32]::GetWindowRect($handle, [ref]$rect) | Out-Null

    if ($ClickX -ge 0 -and $ClickY -ge 0) {
        Invoke-Click ($rect.Left + $ClickX) ($rect.Top + $ClickY)
        if ($DoubleClick) {
            Start-Sleep -Milliseconds 90
            Invoke-Click ($rect.Left + $ClickX) ($rect.Top + $ClickY)
        }
        Start-Sleep -Milliseconds 300
        # Клик мог увести фокус — возвращаем окно на передний план перед снимком.
        [ShotWin32]::SetForegroundWindow($handle) | Out-Null
        Start-Sleep -Milliseconds 150
    }

    # Протяжка мышью: "x1,y1,x2,y2" — координаты окна.
    if ($Drag) {
        $points = $Drag.Split(",") | ForEach-Object { [int]$_.Trim() }
        if ($points.Count -ne 4) { throw "Drag ожидает x1,y1,x2,y2, получено: $Drag" }
        Invoke-Drag ($rect.Left + $points[0]) ($rect.Top + $points[1]) `
            ($rect.Left + $points[2]) ($rect.Top + $points[3]) $DragHold.IsPresent
        [ShotWin32]::SetForegroundWindow($handle) | Out-Null
        Start-Sleep -Milliseconds 150
    }

    # Наведение без клика: нужно для проверки hover-переключения меню.
    if ($Hover) {
        $points = $Hover.Split(",") | ForEach-Object { [int]$_.Trim() }
        if ($points.Count -ne 2) { throw "Hover ожидает x,y, получено: $Hover" }
        [ShotWin32]::SetCursorPos(($rect.Left + $points[0]), ($rect.Top + $points[1])) | Out-Null
        Start-Sleep -Milliseconds 400
    }

    # Клавиши: настоящий ввод через SendKeys — тот же путь, что и у человека.
    # Так проверяются Enter/стрелки/набор, а не вызов внутренних функций.
    if ($Keys) {
        [ShotWin32]::SetForegroundWindow($handle) | Out-Null
        Start-Sleep -Milliseconds 150
        $shell = New-Object -ComObject WScript.Shell
        $shell.SendKeys($Keys)
        Start-Sleep -Milliseconds $KeyWait
    }

    $width = $rect.Right - $rect.Left
    $height = $rect.Bottom - $rect.Top
    if ($width -le 0 -or $height -le 0) { throw "нулевой прямоугольник окна" }

    $bitmap = New-Object System.Drawing.Bitmap($width, $height)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, (New-Object System.Drawing.Size($width, $height)))
    $target = Join-Path $root $Out
    $bitmap.Save($target, [System.Drawing.Imaging.ImageFormat]::Png)
    $graphics.Dispose()
    $bitmap.Dispose()

    Write-Host ("скрин: {0} ({1}x{2})" -f $target, $width, $height)
}
finally {
    if ($process -and -not $process.HasExited) { Stop-Process -Id $process.Id -Force }
}
