# The competitors on a Windows runner, laid out as the benchmark expects on Windows (see
# crates/virtuoso-bench/src/tools.rs): Maestro and maestro-runner in bench/tools, Appium, e2e and
# Argent from npm, and virtuoso's Linux build in the WSL distribution. Run from the repository;
# $env:WSL_DISTRO names the distribution. Only the tools in $env:BENCH_TOOLS (comma-separated;
# all when unset) are set up. speedrun only when named: it cannot run on these runners (see
# run-android-windows.ps1), so the default set leaves it out.
$ErrorActionPreference = "Stop"
$tools = Join-Path $PWD "bench\tools"
New-Item -ItemType Directory -Force $tools | Out-Null
$temp = $env:RUNNER_TEMP
$bench = $env:BENCH_TOOLS -replace " ", ""
function Wanted([string]$tool) { -not $bench -or ",$bench," -like "*,$tool,*" }
function Named([string]$tool) { $bench -and ",$bench," -like "*,$tool,*" }
# A committed lockfile pins a tool's dependency tree: npm ci installs exactly it, from npm's
# cache. e2e's and Argent's are not committed (a fresh checkout has none): npm install for those.
function Npm-Ci([string]$dir) {
    Push-Location $dir
    if (Test-Path package-lock.json) { npm ci --no-audit --no-fund --loglevel=error } else { npm install --no-audit --no-fund --loglevel=error }
    Pop-Location
}

# Maestro 2.11.0.
if (Wanted "maestro") {
    Invoke-WebRequest https://github.com/mobile-dev-inc/maestro/releases/download/cli-2.11.0/maestro.zip -OutFile "$temp\maestro.zip"
    Expand-Archive "$temp\maestro.zip" -DestinationPath "$tools\maestro-2.11.0" -Force
}

# maestro-runner 1.1.29 has no Windows release: built from its tag, as on the reference machine
# (the workflow builds it once and puts it in bench\tools for every job).
if ((Wanted "maestro-runner") -and -not (Test-Path "$tools\maestro-runner.exe")) {
    Push-Location $env:MAESTRO_RUNNER_HOME
    $ld = "-X github.com/devicelab-dev/maestro-runner/pkg/cli.Version=1.1.29"
    go build -ldflags $ld -o "$tools\maestro-runner.exe" .
    Pop-Location
}

# Appium 3.9.0 with UiAutomator2 8.0.0, the driver installed in every job (a cached APPIUM_HOME
# left the driver unable to find the appium package it imports); e2e and Argent.
if (Wanted "appium") {
    Npm-Ci bench\appium
    $env:APPIUM_HOME = Join-Path $HOME ".appium"
    npx --prefix bench\appium appium driver install uiautomator2@8.0.0
    "APPIUM_HOME=$env:APPIUM_HOME" | Out-File -Append -Encoding utf8 $env:GITHUB_ENV
}
if (Wanted "e2e") { Npm-Ci bench\e2e }
if (Wanted "argent") { Npm-Ci bench\argent }

# In WSL: virtuoso's Linux build (and speedrun 0.2.11, when named) where the harness looks for
# them, and a Linux adb that talks to the Windows adb server (WSL 1 shares Windows' network).
$speedrun = Named "speedrun"
if ((Wanted "virtuoso") -or $speedrun) {
    $wslPath = { param($p) (wsl -d $env:WSL_DISTRO -u root -- wslpath -a ($p -replace '\\', '/')).Trim() }
    $linux = & $wslPath $env:VIRTUOSO_LINUX_DIR
    # The downloads are made here, not in WSL (which then needs neither curl nor unzip).
    # platform-tools r37.0.1: what platform-tools-latest was for every batch so far, pinned so the
    # adb client virtuoso's Linux build talks through stays the same; from the workflow's cache.
    $pt = Join-Path $tools "platform-tools_r37.0.1-linux.zip"
    if (-not (Test-Path $pt)) {
        Invoke-WebRequest https://dl.google.com/android/repository/platform-tools_r37.0.1-linux.zip -OutFile $pt -MaximumRetryCount 10 -RetryIntervalSec 15
    }
    if ((Get-FileHash $pt -Algorithm SHA1).Hash -ne "477254AA5F903C15CF51001717BDF347FB6B53E0") {
        Remove-Item $pt
        throw "platform-tools_r37.0.1-linux.zip is not the one Google lists"
    }
    $ptLinux = & $wslPath $pt
    $getSpeedrun = if ($speedrun) {
        $tar = Join-Path $temp "speedrun-linux.tar.gz"
        Invoke-WebRequest https://github.com/hubastard/speedrun/releases/download/v0.2.11/speedrun-v0.2.11-linux-x86_64.tar.gz -OutFile $tar -MaximumRetryCount 10 -RetryIntervalSec 15
        $tarLinux = & $wslPath $tar
        @"
mkdir -p /opt/bench/speedrun
tar xzf '$tarLinux' -C /opt/bench/speedrun
ls -R /opt/bench/speedrun | head
"@
    } else { "" }
    # The zip unpacked by WSL's Python (the WSL timer's), which keeps no file modes: adb made
    # executable again.
    $script = @"
set -e
mkdir -p /opt/bench
$getSpeedrun
install -m 755 $linux/virtuoso /opt/bench/virtuoso
rm -rf /opt/platform-tools
python3 -m zipfile -e '$ptLinux' /opt
chmod 755 /opt/platform-tools/adb
ln -sf /opt/platform-tools/adb /usr/local/bin/adb
adb version
"@
    wsl -d $env:WSL_DISTRO -u root -- bash -c ($script -replace "`r", "")
    if ($LASTEXITCODE -ne 0) { throw "WSL setup failed" }
    if ($speedrun) {
        $sr = (wsl -d $env:WSL_DISTRO -u root -- bash -c "find /opt/bench/speedrun -type f -name speedrun | head -1").Trim()
        wsl -d $env:WSL_DISTRO -u root -- chmod +x $sr
        "SPEEDRUN_BIN=$sr" | Out-File -Append -Encoding utf8 $env:GITHUB_ENV
        Write-Output "speedrun in WSL: $sr"
    }
    "VIRTUOSO_LINUX_BIN=/opt/bench/virtuoso" | Out-File -Append -Encoding utf8 $env:GITHUB_ENV
}
