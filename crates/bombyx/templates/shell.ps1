# bombyx's `shell` for a Windows guest: opens an interactive
# PowerShell as the agent's account, in its clone.
#
# The login account runs this, through `vagrant ssh -c`, with $User
# (the agent's account) and $Project (the clone's folder name) set
# on the lines bombyx puts before it. Windows has no `sudo -u` for
# an interactive session, so the login account reaches the agent as
# account.ps1 does to hand over bootstrap.ps1: an SSH login to
# $User@localhost with the hand-over key account.ps1 keeps in the
# login account's own `.ssh`. account.ps1 also records localhost's
# host key in that `.ssh`, so the login checks the host strictly and
# never prompts; `-t` asks it for a terminal.
#
# The login starts `powershell.exe -NoExit` for the agent, and its
# first command is `$enter` below: a `Set-Location` into
# `$env:USERPROFILE\$Project`, where bootstrap.ps1 clones. A missing
# clone prints the error and leaves the shell in the profile, so the
# operator can look into why. `-NoExit` keeps the shell open, and it
# reads the agent's PowerShell profile, as a Linux login shell does.
#
# The agent's sshd runs the login's command under its default shell,
# cmd.exe unless the box names another, so `$enter` travels as an
# `-EncodedCommand` argument: base64, which holds no character
# cmd.exe or PowerShell reads, of its UTF-16LE text, the encoding
# `-EncodedCommand` takes (.NET calls it `Unicode`). That doubles
# its length, which one short line can afford; bombyx's own command
# around this script avoids it for that reason.

# vagrant's `ssh -c` puts the same line before this text; it stays
# here so the script also keeps PowerShell's progress records, which
# arrive on stderr as CLIXML, out of a run without vagrant.
$ProgressPreference = 'SilentlyContinue'

$ssh = Join-Path $env:SystemRoot 'System32\OpenSSH\ssh.exe'
$loginSsh = Join-Path $env:USERPROFILE '.ssh'
$key = Join-Path $loginSsh 'bombyx-handover'
$known = Join-Path $loginSsh 'bombyx-localhost-known-hosts'

# A guest that account.ps1 never finished has no account, or no key
# to reach it with. The operator still gets a shell, as the login
# account, to look around.
$missing = $null
if (-not (Get-LocalUser -Name $User -ErrorAction SilentlyContinue)) {
    $missing = "has no account $User, so it was never provisioned for it"
} elseif (-not (Test-Path -LiteralPath $key -PathType Leaf)) {
    $missing = "has no hand-over key at $key to reach $User with"
}
if ($null -ne $missing) {
    [Console]::Error.WriteLine(
        "bombyx: this guest $missing; run bombyx provision, or " +
        'bombyx destroy then bombyx up if provisioning refuses. ' +
        "Opening a shell as $env:USERNAME instead.")
    & powershell.exe -NoLogo
    exit $LASTEXITCODE
}

# $Project goes inside a single-quoted literal in `$enter`, so
# PowerShell's own escaper doubles every character it reads as a
# single quote there, as bombyx's `quote` does for the lines above
# this script.
$escaper = [Management.Automation.Language.CodeGeneration]
$projectLiteral = "'" +
    $escaper::EscapeSingleQuotedStringContent($Project) + "'"
$enter = 'Set-Location -LiteralPath ' +
    "(Join-Path `$env:USERPROFILE $projectLiteral)"
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
# 255 here is ssh.exe's own, and the message says what to check. The
# script still exits with ssh's status, which a run without a
# terminal does pass on.
if ($code -eq 255) {
    [Console]::Error.WriteLine(
        "bombyx: ssh exited 255. If that was the login to " +
        "$User@localhost failing, run bombyx provision, which sets up " +
        'the hand-over key again.')
}
exit $code
