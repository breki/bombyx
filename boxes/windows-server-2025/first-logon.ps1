# Runs once, at the vagrant account's automatic first logon during
# build.sh's install. It checks the installed edition, then hands the
# rest of the build to stage.ps1, which a startup task runs as SYSTEM
# at every boot until the build ends.
#
# A startup task rather than this logon, because installing updates
# restarts Windows several times, and a first-logon command runs only
# once. Windows' update API also refuses to run from a remote session,
# which a task started by Windows is not.
#
# Progress goes to COM1, which build.sh records in its log.
#
# Targets Windows PowerShell 5.1, which Server 2025 ships.

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$Serial = New-Object System.IO.Ports.SerialPort 'COM1', 115200
$Serial.Open()

# Reports why the build failed and shuts the VM down, so build.sh
# stops waiting.
function Fail([string] $Why) {
    $Serial.WriteLine("BOMBYX-FAILED: $Why")
    Stop-Computer -Force
    exit 1
}

try {
    $cfg = (Get-Volume -FileSystemLabel BOMBYXCFG).DriveLetter + ':'

    # The answer file picks the ISO's first image by index, which is
    # expected to be Standard, Server Core. Checked here rather than
    # assumed, so a different ISO cannot build the wrong box.
    $os = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
    $Serial.WriteLine("bombyx: installed $($os.EditionID), " +
        "$($os.InstallationType), build $($os.CurrentBuild).$($os.UBR)")
    if ($os.EditionID -ne 'ServerStandardEval' -or
        $os.InstallationType -ne 'Server Core') {
        Fail ("the install is $($os.EditionID), $($os.InstallationType); " +
            'the box must be ServerStandardEval, Server Core')
    }

    # stage.ps1 is copied off the config CD, so the task does not
    # depend on which drive letter the CD gets at a later boot.
    $stage = 'C:\Windows\Temp\bombyx-stage.ps1'
    Copy-Item -LiteralPath (Join-Path $cfg 'stage.ps1') -Destination $stage
    $action = New-ScheduledTaskAction -Execute 'powershell.exe' `
        -Argument "-NoProfile -ExecutionPolicy Bypass -File $stage"
    $trigger = New-ScheduledTaskTrigger -AtStartup
    $principal = New-ScheduledTaskPrincipal -UserId 'SYSTEM' `
        -LogonType ServiceAccount -RunLevel Highest
    # No time limit: a large update runs for a long while.
    $settings = New-ScheduledTaskSettingsSet `
        -ExecutionTimeLimit ([TimeSpan]::Zero)
    Register-ScheduledTask -TaskName 'bombyx-build' -Action $action `
        -Trigger $trigger -Principal $principal -Settings $settings `
        -Force | Out-Null
    $Serial.WriteLine('bombyx: restarting into the build stage')
    Restart-Computer -Force
} catch {
    Fail $_.Exception.Message
}
