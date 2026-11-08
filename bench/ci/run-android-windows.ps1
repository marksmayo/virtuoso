# One CI job's share of the Android benchmark on a Windows runner: an emulator booted
# (accelerated by WHPX), the benchmark apps installed, then every tool on the scenarios given,
# into results.json.
param([string]$Scenarios, [string]$Iterations = "5")
$ErrorActionPreference = "Stop"
$sdk = $env:ANDROID_HOME
$image = "system-images;android-36;google_apis;x86_64"
$env:PATH = "$sdk\platform-tools;$sdk\emulator;$env:PATH"
"y`ny`ny`ny`ny`ny`n" | & "$sdk\cmdline-tools\latest\bin\sdkmanager.bat" --install "platform-tools" "emulator" $image | Out-Null
"no" | & "$sdk\cmdline-tools\latest\bin\avdmanager.bat" create avd -n bench -k $image -d pixel_7 --force | Out-Null
# gRPC on: Argent's simulator-server finds a running emulator by its gRPC endpoint.
Start-Process emulator -ArgumentList "-avd bench -no-window -gpu swiftshader_indirect -noaudio -no-boot-anim -no-snapshot -grpc 8554" -WindowStyle Hidden
adb wait-for-device
$deadline = (Get-Date).AddMinutes(10)
while ((adb shell getprop sys.boot_completed 2>$null) -ne "1") {
    if ((Get-Date) -gt $deadline) { throw "the emulator did not boot in 10 minutes" }
    Start-Sleep 5
}
adb install -r -t "$env:RUNNER_TEMP\apps\speedrun-bench.apk"
adb install -r -t "$env:RUNNER_TEMP\apps\mobile-benchmark.apk"
# The version runs (CLI start) once, in the settings job.
$versionRuns = if ($Scenarios -like "*launch*") { 10 } else { 0 }
# Every tool but speedrun: it runs on Windows only in WSL, and on these runners neither WSL
# works for it (WSL 1's kernel cannot run it; WSL 2 cannot reach the forwarded ports).
$tools = "virtuoso-windows,virtuoso,maestro-runner,maestro,appium,e2e,argent"
.\target\release\virtuoso-bench.exe run --tools $tools --scenarios $Scenarios --iterations $Iterations --version-runs $versionRuns --out results.json
if ($LASTEXITCODE -ne 0) { throw "the benchmark failed" }
