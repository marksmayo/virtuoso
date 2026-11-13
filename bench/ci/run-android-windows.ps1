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
adb install -r -t "$env:RUNNER_TEMP\apps\speedrun-bench.apk"
adb install -r -t "$env:RUNNER_TEMP\apps\mobile-benchmark.apk"
# The version runs (CLI start) once, in the settings job.
$versionRuns = if ($Scenarios -like "*launch*") { 10 } else { 0 }
# Every tool but speedrun: it runs on Windows only in WSL, and on these runners neither WSL
# works for it (WSL 1's kernel cannot run it; WSL 2 cannot reach the forwarded ports).
$tools = if ($Tools) { $Tools } else { "virtuoso-windows,virtuoso,maestro-runner,maestro,appium,e2e,argent" }
.\target\release\virtuoso-bench.exe run --tools $tools --scenarios $Scenarios --iterations $Iterations --version-runs $versionRuns --out results.json --outputs outputs
if ($LASTEXITCODE -ne 0) { throw "the benchmark failed" }
