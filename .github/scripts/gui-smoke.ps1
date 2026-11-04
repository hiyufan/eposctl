# GUI smoke test: start the debug build of eposctl-gui once per page and save a
# screenshot of each (EPOSCTL_PAGE / EPOSCTL_SHOT, see src/bin/eposctl-gui/debug.rs).
# Fails if the app does not start, crashes, or does not produce a screenshot.
#
# The runner has no GPU, so Mesa's software OpenGL (llvmpipe) is put next to the
# exe; Windows loads opengl32.dll from the exe's folder first.
#
# There is no EPOS driver or dongle on the runner: pages show their
# "not connected" state. This checks start-up and drawing, not the device code.

param(
    [string]$Exe = "target\debug\eposctl-gui.exe",
    [string]$OutDir = "gui-screenshots",
    [int]$TimeoutSeconds = 90
)

$ErrorActionPreference = "Stop"
$exeDir = Split-Path -Parent (Resolve-Path $Exe)

# ---- software OpenGL ----
# Mesa for Windows (pal1000/mesa-dist-win), pinned. Its MSVC package no longer
# ships the desktop OpenGL driver; the MinGW package does.
$mesaTag = "26.2.6"
$mesaAsset = "mesa3d-$mesaTag-release-mingw.7z"
$mesa = Join-Path $env:RUNNER_TEMP "mesa"
New-Item -ItemType Directory -Force -Path $mesa | Out-Null
gh release download $mesaTag --repo pal1000/mesa-dist-win --pattern $mesaAsset --dir $mesa --clobber
if ($LASTEXITCODE) { throw "could not download $mesaAsset" }
& 7z x (Join-Path $mesa $mesaAsset) "-o$mesa" -y | Out-Null
if ($LASTEXITCODE) { throw "could not extract $mesaAsset" }
# Only the WGL driver: an EGL library next to the exe could be picked up instead.
foreach ($dll in "opengl32.dll", "libgallium_wgl.dll", "libglapi.dll") {
    $src = Join-Path $mesa "x64\$dll"
    if (Test-Path $src) { Copy-Item $src $exeDir -Force; Write-Host "using $src" }
}
if (-not (Test-Path (Join-Path $exeDir "opengl32.dll"))) { throw "no x64\opengl32.dll in $mesaAsset" }
$env:GALLIUM_DRIVER = "llvmpipe"

# ---- one run per page ----
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$failed = @()
foreach ($page in "sound", "mic", "device", "settings") {
    $shot = Join-Path (Resolve-Path $OutDir) "$page.png"
    $env:EPOSCTL_PAGE = $page
    $env:EPOSCTL_SHOT = $shot
    $env:EPOSCTL_SHOT_DELAY = "5"
    # A GUI-subsystem exe has no console; redirect so its error message is kept.
    $log = Join-Path (Resolve-Path $OutDir) "$page.log"
    $p = Start-Process -FilePath $Exe -PassThru -RedirectStandardError $log -RedirectStandardOutput "$log.out"
    $null = $p.Handle # keeps the exit code readable after the process ends
    if (-not $p.WaitForExit($TimeoutSeconds * 1000)) {
        Stop-Process -Id $p.Id -Force
        $failed += "${page}: no screenshot within $TimeoutSeconds s"
        continue
    }
    if ($p.ExitCode -ne 0) {
        $failed += "${page}: exited with code $($p.ExitCode): $((Get-Content $log -Raw) -replace '\s+', ' ')"
    } elseif (-not (Test-Path $shot) -or (Get-Item $shot).Length -lt 1000) {
        $failed += "${page}: screenshot missing or empty"
    } else {
        Write-Host "${page}: ok ($((Get-Item $shot).Length) bytes)"
    }
}

if ($failed) {
    $failed | ForEach-Object { Write-Host "::error::$_" }
    exit 1
}
