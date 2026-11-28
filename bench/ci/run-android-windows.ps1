# One CI job's share of the Android benchmark on a Windows runner: the emulator the job started
# booting first (emulator-windows.ps1; accelerated by WHPX) waited for, the benchmark apps
# installed, then every tool (or those of -Tools, comma-separated) on the scenarios given, into
# results.json.
param([string]$Scenarios, [string]$Iterations = "5", [string]$Tools = "")
$ErrorActionPreference = "Stop"
$sdk = $env:ANDROID_HOME
$env:PATH = "$sdk\platform-tools;$sdk\emulator;$env:PATH"
adb wait-for-device
$deadline = (Get-Date).AddMinutes(10)
while ((adb shell getprop sys.boot_completed 2>$null) -ne "1") {
    if ((Get-Date) -gt $deadline) { throw "the emulator did not boot in 10 minutes" }
    Start-Sleep 1
}
# (Play services' location, minutes behind the boot on WHPX, is waited for by the harness itself,
# before any tool's first run: crates/virtuoso-bench/src/location.rs.)
# Booted is not ready: the package service can still be starting ("Can't find service:
# package"), or restart under it. A failed install is tried again, and the job stops if it never
# goes in (every tool would fail every scenario without the app).
function Install-Apk([string]$apk) {
    $wait = if ($env:VIRTUOSO_INSTALL_RETRY_SECONDS) { [int]$env:VIRTUOSO_INSTALL_RETRY_SECONDS } else { 10 }
    for ($i = 1; $i -le 6; $i++) {
        adb install -r -t $apk
        if ($LASTEXITCODE -eq 0) { return }
        Start-Sleep $wait
    }
    throw "could not install $apk"
}
Install-Apk "$env:RUNNER_TEMP\apps\speedrun-bench.apk"
Install-Apk "$env:RUNNER_TEMP\apps\mobile-benchmark.apk"
# Settled before any tool runs, the same for all of them: the boot leaves app compilation
# pending (done at once now, not during a run), and the guest stays busy for minutes after
# booting on WHPX (an app's start took 13 s against 4-5 s in a run's first minutes). Until its
# load average is under 2, at most 4 minutes.
adb shell cmd package bg-dexopt-job | Out-Null
$quiet = (Get-Date).AddMinutes(4)
while ((Get-Date) -lt $quiet) {
    $load = (adb shell cat /proc/loadavg 2>$null) -split " " | Select-Object -First 1
    if ($load -and [double]$load -lt 2) { break }
    Start-Sleep 5
}
Write-Output "guest load before the runs: $(adb shell cat /proc/loadavg 2>$null)"
# The version runs (CLI start) once, in the settings job.
$versionRuns = if ($Scenarios -like "*launch*") { 10 } else { 0 }
# None in an A/B (VIRTUOSO_AB_BASE_BIN): two builds of virtuoso alone.
if ($env:VIRTUOSO_AB_BASE_BIN) { $versionRuns = 0 }
# Every tool but speedrun: it runs on Windows only in WSL, and on these runners neither WSL
# works for it (WSL 1's kernel cannot run it; WSL 2 cannot reach the forwarded ports).
$tools = if ($Tools) { $Tools } else { "virtuoso-windows,virtuoso,maestro-runner,maestro,appium,e2e,argent" }
.\target\release\virtuoso-bench.exe run --tools $tools --scenarios $Scenarios --iterations $Iterations --version-runs $versionRuns --out results.json --outputs outputs
if ($LASTEXITCODE -ne 0) { throw "the benchmark failed" }
