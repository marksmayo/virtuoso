# virtuoso's debug folders of a Windows bench job, into one folder for the job's artifact: the
# Windows build's (~/.virtuoso/tests; it takes HOME, else USERPROFILE) and the Linux build's, run
# in WSL as root (/root/.virtuoso/tests). Each holds a failed flow's screenshot and screen, and
# what a flow saved; videos are left out.
param([Parameter(Mandatory)][string]$Out)
$dir = New-Item -ItemType Directory -Force $Out
foreach ($home_ in @($env:HOME, $env:USERPROFILE) | Where-Object { $_ } | Select-Object -Unique) {
    $tests = Join-Path $home_ ".virtuoso\tests"
    if (Test-Path $tests) {
        Copy-Item $tests (Join-Path $dir.FullName "windows") -Recurse -Force
    }
}
Get-ChildItem $dir.FullName -Recurse -Filter *.mp4 | Remove-Item -Force
if ($env:WSL_DISTRO) {
    $to = (wsl -d $env:WSL_DISTRO -u root -- wslpath -a ($dir.FullName -replace '\\', '/'))
    if ($LASTEXITCODE -eq 0 -and $to) {
        wsl -d $env:WSL_DISTRO -u root -- bash -c "if [ -d /root/.virtuoso/tests ]; then mkdir -p '$($to.Trim())/wsl' && cp -r /root/.virtuoso/tests/. '$($to.Trim())/wsl/' && find '$($to.Trim())/wsl' -name '*.mp4' -delete; fi"
    }
}
exit 0
