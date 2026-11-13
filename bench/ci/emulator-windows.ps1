# The emulator of a Windows bench job, in the workflow's steps around its cache:
#   -Step key      the system image's cache key (its revision as sdkmanager would install it now:
#                  a newer image is a new key) and folder, as step outputs
#   -Step install  the SDK packages: from sdkmanager, which does nothing for an image the cache
#                  restored at that revision
#   -Step boot     the AVD made and the emulator started booting, first thing in the job, so the
#                  rest is set up while it boots (run-android-windows.ps1 waits for it)
param([Parameter(Mandatory)][ValidateSet("key", "install", "boot")][string]$Step)
$ErrorActionPreference = "Stop"
$sdk = $env:ANDROID_HOME
$image = "system-images;android-36;google_apis;x86_64"
$env:PATH = "$sdk\platform-tools;$sdk\emulator;$env:PATH"
switch ($Step) {
    "key" {
        # The image's revision among the packages available (not those installed).
        $available = $false
        $revision = ""
        foreach ($line in (& "$sdk\cmdline-tools\latest\bin\sdkmanager.bat" --list 2>$null)) {
            if ($line -match "Available Packages:") { $available = $true; continue }
            $cols = $line -split "\|"
            if ($available -and $cols.Count -ge 2 -and $cols[0].Trim() -eq $image) { $revision = $cols[1].Trim(); break }
        }
        "dir=$sdk\system-images\android-36\google_apis\x86_64" | Out-File -Append -Encoding utf8 $env:GITHUB_OUTPUT
        # Unknown, no key: no cache, sdkmanager as before.
        if ($revision) {
            "key=android-image-windows-android-36-google_apis-x86_64-r$revision" | Out-File -Append -Encoding utf8 $env:GITHUB_OUTPUT
        } else {
            Write-Output "::warning::sdkmanager --list named no revision of ${image}: not cached"
        }
        Write-Output "$image revision: $revision"
    }
    "install" {
        "y`ny`ny`ny`ny`ny`n" | & "$sdk\cmdline-tools\latest\bin\sdkmanager.bat" --install "platform-tools" "emulator" $image | Out-Null
    }
    "boot" {
        "no" | & "$sdk\cmdline-tools\latest\bin\avdmanager.bat" create avd -n bench -k $image -d pixel_7 --force | Out-Null
        # gRPC on: Argent's simulator-server finds a running emulator by its gRPC endpoint.
        Start-Process emulator -ArgumentList "-avd bench -no-window -gpu swiftshader_indirect -noaudio -no-boot-anim -no-snapshot -grpc 8554" -WindowStyle Hidden
    }
}
