# bombyx's secrets refresh for a running Windows guest: writes the
# file bombyx sends on standard input over the agent's copy, then,
# when the project names one, runs its secrets_refreshed hook as the
# agent.
#
# account.ps1 installs this in the bombyx folder under Program Files
# on every provision, so bombyx's command names it rather than
# carrying it: vagrant re-encodes a `vagrant ssh -c` command, and the
# guest's cmd.exe caps the result's length, which a script this size
# would not fit. The login account runs it, an administrator, as
# account.ps1's Place writes the files at provisioning.
#
# The exit status is what bombyx's RefreshOutcome reads: 0 when the
# file was written and the hook, if any, succeeded; 1 when the file
# may not have been written; and hook.ps1's 90, 91 or 92 when the
# file was written and the hook did not run, failed, or ran too long.
param(
    [int] $Interface,
    [string] $User,
    [string] $File,
    [string] $Project,
    [string] $Hook,
    [int] $Timeout
)
$ProgressPreference = 'SilentlyContinue'
$ErrorActionPreference = 'Stop'

# The version of the call bombyx makes. bombyx passes the one it was
# built for, so a guest provisioned by another version refuses
# rather than read the arguments some other way.
$Supported = 1
# RefreshOutcome's HookRefused: the file was written and the hook
# did not start.
$HookRefused = 90

function Fail([string] $Why, [int] $Code) {
    [Console]::Error.WriteLine("bombyx: $Why")
    exit $Code
}

if ($Interface -ne $Supported) {
    Fail ("this guest's refresh.ps1 takes call $Supported and bombyx " +
        "sent call $Interface, so another version of bombyx provisioned " +
        'it; run bombyx provision.') 1
}

# Gives `sid`, SYSTEM and the administrators full control of `path`
# and nobody else any access: a new list with inheritance off, which
# drops whatever the file held before, the counterpart of mode 0600.
function Protect([string] $Path, [string] $Sid) {
    $acl = New-Object Security.AccessControl.FileSecurity
    $acl.SetAccessRuleProtection($true, $false)
    foreach ($id in @($Sid, 'S-1-5-18', 'S-1-5-32-544')) {
        $who = New-Object Security.Principal.SecurityIdentifier $id
        $acl.AddAccessRule((New-Object `
            Security.AccessControl.FileSystemAccessRule $who, 'FullControl',
            'Allow'))
    }
    Set-Acl -LiteralPath $Path -AclObject $acl
}

$account = Get-LocalUser -Name $User -ErrorAction SilentlyContinue
if ($null -eq $account) {
    Fail ("this guest has no account $User, so it was never " +
        'provisioned for it; run bombyx provision.') 1
}
$sid = $account.SID.Value
$profileKey = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\' +
    "ProfileList\$sid"
$agentHome = (Get-ItemProperty -LiteralPath $profileKey `
    -ErrorAction SilentlyContinue).ProfileImagePath
if ([string]::IsNullOrEmpty($agentHome) -or
    -not (Test-Path -LiteralPath $agentHome -PathType Container)) {
    Fail "the account $User has no profile folder; run bombyx provision." 1
}

# The new copy is written beside the old one and renamed over it, so
# the agent's copy is the old file or the new one, never a partial
# one. It is protected while still empty, so no moment leaves the
# secret under the folder's inherited permissions.
$target = Join-Path $agentHome $File
$new = "$target.new"
try {
    $stdin = [Console]::OpenStandardInput()
    $buffer = New-Object IO.MemoryStream
    $stdin.CopyTo($buffer)
    Remove-Item -LiteralPath $new -Force -ErrorAction SilentlyContinue
    [IO.File]::WriteAllBytes($new, [byte[]]@())
    Protect $new $sid
    [IO.File]::WriteAllBytes($new, $buffer.ToArray())
    if (Test-Path -LiteralPath $target) {
        # Replace carries the old file's permissions over, so the
        # target is protected afresh after it. It takes no backup:
        # PowerShell hands a .NET string parameter "" for $null, which
        # Replace refuses as a path, so the null is spelled out.
        [IO.File]::Replace($new, $target, [NullString]::Value)
    } else {
        [IO.File]::Move($new, $target)
    }
    Protect $target $sid
} catch {
    Remove-Item -LiteralPath $new -Force -ErrorAction SilentlyContinue
    Fail "could not write $target in the guest: $($_.Exception.Message)" 1
}

if ([string]::IsNullOrEmpty($Hook)) {
    exit 0
}

# The hook runs as the agent, as on Linux, over the SSH login to this
# guest that account.ps1 hands bootstrap.ps1 over by, with the key it
# keeps in the login account's own `.ssh`. sshd runs the command
# under cmd.exe, and ssh.exe joins its arguments with spaces and no
# quotes, so a path with a space would arrive split. So the agent's
# side is one `-EncodedCommand`, UTF-16LE base64 of a short call to
# hook.ps1 with the path quoted inside it, and hook.ps1 takes its
# text arguments base64-encoded too; base64 holds no character
# cmd.exe or PowerShell reads.
$ssh = Join-Path $env:SystemRoot 'System32\OpenSSH\ssh.exe'
$loginSsh = Join-Path $env:USERPROFILE '.ssh'
$key = Join-Path $loginSsh 'bombyx-handover'
$known = Join-Path $loginSsh 'bombyx-localhost-known-hosts'
if (-not (Test-Path -LiteralPath $key -PathType Leaf)) {
    Fail ("the secrets are current, but this guest has no hand-over " +
        "key at $key, so the secrets_refreshed hook did not run; run " +
        'bombyx provision.') $HookRefused
}
$runner = Join-Path $PSScriptRoot 'hook.ps1'
$encode = {
    param($text)
    [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($text))
}
$escaper = [Management.Automation.Language.CodeGeneration]
$call = "& '" + $escaper::EscapeSingleQuotedStringContent($runner) +
    "' $(& $encode $Project) $(& $encode $Hook) $Timeout " +
    "$(& $encode $File); exit `$LASTEXITCODE"
$encodedCall = [Convert]::ToBase64String(
    [Text.Encoding]::Unicode.GetBytes($call))
# ssh's output is not redirected: Windows' ssh client and PowerShell
# 5.1 stall on `2>&1`, as account.ps1 records. -n gives it no input,
# because this script has read its own to the end.
$ErrorActionPreference = 'Continue'
& $ssh -n -T -i $key -o IdentitiesOnly=yes -o BatchMode=yes `
    -o StrictHostKeyChecking=yes "-oUserKnownHostsFile=$known" `
    "$User@localhost" powershell.exe -NoProfile -NonInteractive `
    -ExecutionPolicy Bypass -EncodedCommand $encodedCall
$code = $LASTEXITCODE
# ssh exits 255 when the login fails, and hook.ps1 never does, so a
# 255 is always the login's.
if ($code -eq 255) {
    Fail ("the secrets are current, but the SSH login to " +
        "$User@localhost failed, so the secrets_refreshed hook did not " +
        'run; run bombyx provision.') $HookRefused
}
exit $code
