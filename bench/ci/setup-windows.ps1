# The competitors on a Windows runner, laid out as the benchmark expects on Windows (see
# crates/virtuoso-bench/src/tools.rs): Maestro and maestro-runner in bench/tools, Appium, e2e and
# Argent from npm, and speedrun with virtuoso's Linux build in the WSL distribution. Run from
# the repository; $env:WSL_DISTRO names the distribution.
$ErrorActionPreference = "Stop"
$tools = Join-Path $PWD "bench\tools"
New-Item -ItemType Directory -Force $tools | Out-Null
$temp = $env:RUNNER_TEMP

# Maestro 2.11.0.
Invoke-WebRequest https://github.com/mobile-dev-inc/maestro/releases/download/cli-2.11.0/maestro.zip -OutFile "$temp\maestro.zip"
Expand-Archive "$temp\maestro.zip" -DestinationPath "$tools\maestro-2.11.0" -Force

# maestro-runner 1.1.29 has no Windows release: built from its tag, as on the reference machine.
Push-Location $env:MAESTRO_RUNNER_HOME
$ld = "-X github.com/devicelab-dev/maestro-runner/pkg/cli.Version=1.1.29"
go build -ldflags $ld -o "$tools\maestro-runner.exe" .
Pop-Location

# Appium 3.9.0 with UiAutomator2 8.0.0; e2e and Argent.
Push-Location bench\appium; npm install --no-audit --no-fund --loglevel=error; Pop-Location
$env:APPIUM_HOME = Join-Path $HOME ".appium"
npx --prefix bench\appium appium driver install uiautomator2@8.0.0
"APPIUM_HOME=$env:APPIUM_HOME" | Out-File -Append -Encoding utf8 $env:GITHUB_ENV
Push-Location bench\e2e; npm install --no-audit --no-fund --loglevel=error; Pop-Location
Push-Location bench\argent; npm install --no-audit --no-fund --loglevel=error; Pop-Location

# In WSL: speedrun 0.2.11 and virtuoso's Linux build where the harness looks for them, and a
# Linux adb that talks to the Windows adb server (WSL 1 shares Windows' network).
$linux = (wsl -d $env:WSL_DISTRO -u root -- wslpath -a ($env:VIRTUOSO_LINUX_DIR -replace '\\', '/')).Trim()
$script = @"
set -e
mkdir -p /opt/bench/speedrun
curl -fsSL --retry 10 --retry-delay 15 --retry-all-errors https://github.com/hubastard/speedrun/releases/download/v0.2.11/speedrun-v0.2.11-linux-x86_64.tar.gz | tar xz -C /opt/bench/speedrun
ls -R /opt/bench/speedrun | head
install -m 755 $linux/virtuoso /opt/bench/virtuoso
curl -fsSL --retry 10 --retry-delay 15 --retry-all-errors -o /tmp/pt.zip https://dl.google.com/android/repository/platform-tools-latest-linux.zip
unzip -q -o /tmp/pt.zip -d /opt
ln -sf /opt/platform-tools/adb /usr/local/bin/adb
adb version
"@
wsl -d $env:WSL_DISTRO -u root -- bash -c ($script -replace "`r", "")
if ($LASTEXITCODE -ne 0) { throw "WSL setup failed" }
$sr = (wsl -d $env:WSL_DISTRO -u root -- bash -c "find /opt/bench/speedrun -type f -name speedrun | head -1").Trim()
wsl -d $env:WSL_DISTRO -u root -- chmod +x $sr
"SPEEDRUN_BIN=$sr" | Out-File -Append -Encoding utf8 $env:GITHUB_ENV
"VIRTUOSO_LINUX_BIN=/opt/bench/virtuoso" | Out-File -Append -Encoding utf8 $env:GITHUB_ENV
Write-Output "speedrun in WSL: $sr"

