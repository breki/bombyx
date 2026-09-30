# bombyx's call to the secrets refresh on a running Windows guest.
#
# bombyx sends this through `vagrant ssh -c`, with the file on
# standard input and the lines it puts before this naming the call's
# arguments: $Interface, the version of the call; $User, the agent's
# account; $File, the file's name in the agent's profile; $Project,
# the clone's folder name; $Hook, the secrets_refreshed hook inside
# the clone, or '' for none; and $Timeout, the hook's limit in
# seconds. refresh.ps1 does the work. account.ps1 installs it on
# every provision, because it is too long to carry on the guest's
# command line.
$helper = Join-Path $env:ProgramFiles 'bombyx\refresh.ps1'
if (-not (Test-Path -LiteralPath $helper -PathType Leaf)) {
    [Console]::Error.WriteLine(
        "bombyx: this guest has no $helper; run bombyx provision, " +
        'which installs it.')
    exit 1
}
& $helper -Interface $Interface -User $User -File $File `
    -Project $Project -Hook $Hook -Timeout $Timeout
exit $LASTEXITCODE
