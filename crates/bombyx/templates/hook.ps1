# bombyx's runner for a project's secrets_refreshed hook on a
# Windows guest, as the agent, after refresh.ps1 has written the
# secrets file. account.ps1 installs it beside refresh.ps1, which
# starts it over the SSH login to the agent's account.
#
# The three text arguments arrive base64-encoded, because they cross
# cmd.exe, which sshd runs the command under: the clone's folder
# name, the hook's path inside the clone as the config spells it, and
# the secrets file's name in the agent's profile. The time limit in
# seconds, third in order, is a plain number.
#
# The exit status is what bombyx's RefreshOutcome reads, as on a
# Linux guest: 0 when the hook succeeded; 90 when it did not start,
# for any reason; 91 when it exited non-zero; 92 when it ran past the
# time limit and was stopped.
param(
    [string] $ProjectB64,
    [string] $HookB64,
    [int] $Timeout,
    [string] $FileB64
)
$ProgressPreference = 'SilentlyContinue'
$ErrorActionPreference = 'Stop'

$HookRefused = 90
$HookFailed = 91
$HookTimedOut = 92
# How much of the hook's output is relayed, as on Linux.
$OutputCap = 65536

# An error this script did not expect still ends with a status
# RefreshOutcome reads as "the secrets are current", because
# refresh.ps1 wrote them before starting this: 90 until Start-Process
# has started the hook, 91 after. Without this, PowerShell would exit 1,
# which reads as a failed write.
$started = $false
trap {
    [Console]::Error.WriteLine(
        "bombyx: the secrets_refreshed hook runner stopped: " +
        $_.Exception.Message)
    if ($started) { exit $HookFailed }
    exit $HookRefused
}

function Refuse([string] $Why) {
    [Console]::Error.WriteLine("bombyx: $Why")
    exit $HookRefused
}

$decode = {
    param($text)
    [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($text))
}
$Project = & $decode $ProjectB64
$Hook = & $decode $HookB64
$File = & $decode $FileB64

# The clone is where bootstrap.ps1 clones: the project's folder in
# the agent's profile.
$clone = Join-Path $env:USERPROFILE $Project
if (-not (Test-Path -LiteralPath $clone -PathType Container)) {
    Refuse "no clone at $clone, so the secrets_refreshed hook did not run"
}

# The hook, which has to be a .ps1 file inside the clone with no link
# on the way to it, the rules bootstrap.ps1 holds the project's
# script to. A link could point anywhere on the guest, and bombyx
# runs only what the project's own tree holds.
$path = Join-Path $clone ($Hook -replace '/', '\')
$full = [IO.Path]::GetFullPath($path)
$root = [IO.Path]::GetFullPath($clone).TrimEnd('\') + '\'
if (-not $full.StartsWith($root, [StringComparison]::OrdinalIgnoreCase)) {
    Refuse ("the secrets_refreshed hook $Hook leads outside the clone, " +
        'so it did not run')
}
if (-not (Test-Path -LiteralPath $full -PathType Leaf)) {
    Refuse "no secrets_refreshed hook at $Hook in the clone"
}
if ([IO.Path]::GetExtension($full) -ne '.ps1') {
    Refuse ("the secrets_refreshed hook $Hook is not a .ps1 file, so it " +
        'did not run')
}
# Every step from the hook up to the clone, the hook included, as
# bootstrap.ps1 checks the script.
$step = $full
while ($step.Length -ge $root.Length) {
    $item = Get-Item -LiteralPath $step -Force
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
        Refuse ("the secrets_refreshed hook $Hook passes through a link " +
            "at $step, so it did not run")
    }
    $step = Split-Path -Parent $step
}

# The hook starts from a pruned environment, as on Linux, where it
# starts from an empty one. That guards against the agent's own
# settings reaching the refresh by accident; it is no boundary
# against the agent, an administrator here. Windows programs need
# these names to find the system and the profile, so they stay;
# PATH is set to the system's folders and bombyx's git, and the two
# names provisioning also sets are added.
$keep = @('ALLUSERSPROFILE', 'APPDATA', 'CommonProgramFiles',
    'CommonProgramFiles(x86)', 'CommonProgramW6432', 'COMPUTERNAME',
    'ComSpec', 'HOMEDRIVE', 'HOMEPATH', 'LOCALAPPDATA',
    'NUMBER_OF_PROCESSORS', 'OS', 'PATHEXT', 'PROCESSOR_ARCHITECTURE',
    'ProgramData', 'ProgramFiles', 'ProgramFiles(x86)', 'ProgramW6432',
    'PUBLIC', 'SystemDrive', 'SystemRoot', 'TEMP', 'TMP', 'USERDOMAIN',
    'USERNAME', 'USERPROFILE', 'windir')
foreach ($variable in @(Get-ChildItem Env:)) {
    if ($keep -notcontains $variable.Name) {
        Remove-Item -LiteralPath "Env:$($variable.Name)"
    }
}
$env:Path = @(
    (Join-Path $env:SystemRoot 'System32'),
    $env:SystemRoot,
    (Join-Path $env:SystemRoot 'System32\Wbem'),
    (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0'),
    (Join-Path $env:ProgramFiles 'bombyx\git\cmd')) -join ';'
$env:BOMBYX_PROJECT = $Project
$env:BOMBYX_ENV_FILE = Join-Path $env:USERPROFILE $File

# The hook's output goes to two temporary files, not to the pipe
# bombyx reads. A hook may leave a process running, such as a dev
# server it restarted, and that process keeps whatever the hook's
# output was; pointed at the pipe, it would hold the refresh, and
# `shell` behind it, open. Start-Process hands the hook the files
# themselves, so this waits for the hook alone, then relays the
# first $OutputCap bytes of its output and then of its errors.
try {
    $out = [IO.Path]::GetTempFileName()
    $err = [IO.Path]::GetTempFileName()
} catch {
    Refuse ("no temporary file for the secrets_refreshed hook's output, " +
        'so it did not run')
}
$powershell = Join-Path $env:SystemRoot `
    'System32\WindowsPowerShell\v1.0\powershell.exe'
$process = Start-Process -FilePath $powershell -ArgumentList @(
        '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
        '-File', "`"$full`"") `
    -WorkingDirectory $clone -NoNewWindow -PassThru `
    -RedirectStandardOutput $out -RedirectStandardError $err
$started = $true
# Windows PowerShell 5.1 reports no exit code for a process whose
# handle was never read.
$null = $process.Handle
$finished = $process.WaitForExit($Timeout * 1000)
if (-not $finished) {
    # The hook alone is stopped, as on Linux, where `timeout` signals
    # the hook's own process.
    try { $process.Kill() } catch { }
    $null = $process.WaitForExit(5000)
}

# A process the hook left may still hold the files, so they are read
# with sharing on, and removed only if Windows lets go of them. The
# hook has finished by now, so a relay that fails is said on its own
# and leaves the hook's status as it is.
try {
    $stdout = [Console]::OpenStandardOutput()
    $left = $OutputCap
    $total = 0
    foreach ($name in @($out, $err)) {
        $stream = [IO.File]::Open($name, 'Open', 'Read', 'ReadWrite, Delete')
        try {
            $total += $stream.Length
            $take = [int][Math]::Min($left, $stream.Length)
            if ($take -gt 0) {
                $bytes = New-Object byte[] $take
                $read = $stream.Read($bytes, 0, $take)
                $stdout.Write($bytes, 0, $read)
                $left -= $read
            }
        } finally {
            $stream.Dispose()
        }
        Remove-Item -LiteralPath $name -Force -ErrorAction SilentlyContinue
    }
    $stdout.Flush()
    if ($total -gt $OutputCap) {
        [Console]::Error.WriteLine(
            "bombyx: the secrets_refreshed hook printed $total bytes; the " +
            "rest was dropped after $OutputCap")
    }
} catch {
    [Console]::Error.WriteLine(
        "bombyx: could not relay the secrets_refreshed hook's output: " +
        $_.Exception.Message)
}

if (-not $finished) {
    [Console]::Error.WriteLine(
        'bombyx: the secrets_refreshed hook ran longer than ' +
        "$Timeout seconds and was stopped")
    exit $HookTimedOut
}
if ($process.ExitCode -eq 0) {
    exit 0
}
[Console]::Error.WriteLine(
    "bombyx: the secrets_refreshed hook exited $($process.ExitCode)")
exit $HookFailed
