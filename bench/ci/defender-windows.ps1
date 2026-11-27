# Microsoft Defender on a Windows benchmark runner. It logs real-time protection and the paths
# Defender already leaves out, either way. With -Exclude true (the workflows' defender_exclusion
# input, off by default) it also leaves out the temp folders, where every tool's browser
# profiles, test outputs and downloaded apps are written: the same for every tool, before any
# of them runs. (WSL keeps its files in its own disk, not in these folders.)
param([string]$Exclude = "false")

function Show-Defender([string]$When) {
    try {
        $status = Get-MpComputerStatus -ErrorAction Stop
        $paths = (Get-MpPreference -ErrorAction Stop).ExclusionPath
        Write-Host "Defender ($When): real-time protection $($status.RealTimeProtectionEnabled); excluded paths: $(if ($paths) { $paths -join ', ' } else { 'none' })"
    } catch {
        Write-Host "Defender ($When): not readable ($($_.Exception.Message))"
    }
}

Show-Defender "before"
if ($Exclude -eq "true") {
    $folders = @($env:TEMP, $env:TMP, $env:RUNNER_TEMP) | Where-Object { $_ } | ForEach-Object { [System.IO.Path]::GetFullPath($_) } | Select-Object -Unique
    # Asked for, it must take: the host note says the folders are left out.
    Add-MpPreference -ExclusionPath $folders -ErrorAction Stop
    Show-Defender "after excluding $($folders -join ', ')"
}
exit 0
