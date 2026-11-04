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
$mesa = Join-Path $env:RUNNER_TEMP "mesa"
New-Item -ItemType Directory -Force -Path $mesa | Out-Null
$tag = gh release view --repo pal1000/mesa-dist-win --json tagName --jq .tagName
Write-Host "Mesa for Windows $tag"
gh release download $tag --repo pal1000/mesa-dist-win --pattern "*-release-msvc.7z" --dir $mesa --clobber
$archive = Get-ChildItem $mesa -Filter *.7z | Select-Object -First 1
& 7z x $archive.FullName "-o$mesa" -y | Out-Null
if ($LASTEXITCODE) { throw "could not extract $($archive.Name)" }
# Only the WGL driver: an EGL library next to the exe could be picked up instead.
foreach ($dll in "opengl32.dll", "libgallium_wgl.dll", "libglapi.dll") {
    $src = Join-Path $mesa "x64\$dll"
    if (Test-Path $src) { Copy-Item $src $exeDir -Force; Write-Host "using $dll" }
}
$env:GALLIUM_DRIVER = "llvmpipe"

# ---- one run per page ----
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$failed = @()
foreach ($page in "sound", "mic", "device", "settings") {
    $shot = Join-Path (Resolve-Path $OutDir) "$page.png"
    $env:EPOSCTL_PAGE = $page
    $env:EPOSCTL_SHOT = $shot
    $env:EPOSCTL_SHOT_DELAY = "5"
    $p = Start-Process -FilePath $Exe -PassThru
    $null = $p.Handle # keeps the exit code readable after the process ends
    if (-not $p.WaitForExit($TimeoutSeconds * 1000)) {
        Stop-Process -Id $p.Id -Force
        $failed += "${page}: no screenshot within $TimeoutSeconds s"
        continue
    }
    if ($p.ExitCode -ne 0) {
        $failed += "${page}: exited with code $($p.ExitCode)"
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
