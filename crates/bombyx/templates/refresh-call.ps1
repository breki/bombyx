# bombyx's call to the secrets refresh on a running Windows guest.
#
# bombyx sends this through `vagrant ssh -c`, with the file on
# standard input and, before it, the lines
# `remote::windows::refresh_command` writes, which name the call's
# arguments (raise HELPER_CALL there whenever they change): $Interface, the version of the call; $User, the agent's
# account; $File, the file's name in the agent's profile; $Project,
# the clone's folder name; $Hook, the secrets_refreshed hook inside
# the clone, or '' for none; and $Timeout, the hook's limit in
# seconds. refresh.ps1 does the work. account.ps1 installs it on
# every provision, because it is too long to carry on the guest's
# command line.
$helper = Join-Path $env:ProgramFiles 'bombyx\refresh.ps1'
if (-not (Test-Path -LiteralPath $helper -PathType Leaf)) {
    [Console]::Error.WriteLine(
        "bombyx: this guest has no $helper; run provision for this " +
        'project, which installs it.')
    exit 1
}
# A helper that does not start -- one that no longer parses, say --
# is an error here, not an exit status, and PowerShell would then
# leave $LASTEXITCODE unset, which exits 0 and reads as success. So
# the call stops on it and exits 1: the file was not written.
$ErrorActionPreference = 'Stop'
$global:LASTEXITCODE = 1
try {
    & $helper -Interface $Interface -User $User -File $File `
        -Project $Project -Hook $Hook -Timeout $Timeout
} catch {
    [Console]::Error.WriteLine("bombyx: $helper did not run: $_")
    exit 1
}
exit $LASTEXITCODE
