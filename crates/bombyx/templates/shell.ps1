# bombyx's `shell` for a Windows guest: opens an interactive
# PowerShell as the agent's account, in its clone.
#
# The login account runs this, through `vagrant ssh -c`, with $User
# (the agent's account) and $Project (the clone's folder name) set
# on the lines bombyx puts before it. Windows has no `sudo -u` for
# an interactive session, so the login account reaches the agent as
# account.ps1 does to hand over bootstrap.ps1: an SSH login to
# $User@localhost with the hand-over key account.ps1 keeps in the
# login account's own `.ssh`. `-t` asks that login for a terminal.
#
# The agent's sshd runs the command under its default shell,
# cmd.exe unless the box names another, so the shell's first step
# reaches it base64-encoded, which holds no character cmd.exe or
# PowerShell reads. That step enters
# `$env:USERPROFILE\$Project`, where bootstrap.ps1 clones; a missing
# clone prints the error and leaves the shell in the profile, so the
# operator can look into why. `-NoExit` keeps the shell open, and it
# reads the agent's PowerShell profile, as a Linux login shell does.
$ProgressPreference = 'SilentlyContinue'

$ssh = Join-Path $env:SystemRoot 'System32\OpenSSH\ssh.exe'
$loginSsh = Join-Path $env:USERPROFILE '.ssh'
$key = Join-Path $loginSsh 'bombyx-handover'
$known = Join-Path $loginSsh 'bombyx-localhost-known-hosts'

# A guest that account.ps1 never finished has no account or no key.
# The operator still gets a shell, as the login account, to look
# around.
if (-not (Get-LocalUser -Name $User -ErrorAction SilentlyContinue) -or
    -not (Test-Path -LiteralPath $key -PathType Leaf)) {
    [Console]::Error.WriteLine(
        "bombyx: this guest has no account $User set up by bombyx, so " +
        'it was never provisioned for it; run bombyx provision, or ' +
        'bombyx destroy then bombyx up if provisioning refuses. ' +
        "Opening a shell as $env:USERNAME instead.")
    & powershell.exe -NoLogo
    exit $LASTEXITCODE
}

$enter = "Set-Location -LiteralPath (Join-Path `$env:USERPROFILE '$Project')"
$encoded = [Convert]::ToBase64String(
    [Text.Encoding]::Unicode.GetBytes($enter))
& $ssh -t -i $key -o IdentitiesOnly=yes -o BatchMode=yes `
    -o StrictHostKeyChecking=yes "-oUserKnownHostsFile=$known" `
    "$User@localhost" powershell.exe -NoLogo -NoExit `
    -EncodedCommand $encoded
$code = $LASTEXITCODE
# ssh.exe exits 255 when it cannot log in. Once the login worked, the
# session's own exit status does not come back: Windows' sshd reports
# 0 for a session with a terminal, on this hop and on vagrant's. So a
# 255 here is ssh.exe's own, and the message says what to check.
if ($code -eq 255) {
    [Console]::Error.WriteLine(
        "bombyx: ssh exited 255. If that was the login to " +
        "$User@localhost failing, run bombyx provision, which sets up " +
        'the hand-over key again.')
}
exit $code
