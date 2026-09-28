# Runs as SYSTEM at every boot of build.sh's install, from the
# `bombyx-build` startup task first-logon.ps1 registers.
#
# Each run installs the Windows updates still missing and restarts,
# through Windows' own update service. When none are left, it sets up
# sshd for vagrant's winssh communicator, cleans up, generalizes the
# install and shuts the VM down, which ends the build.
#
# Progress goes to COM1, which build.sh records in its log. The last
# line is `BOMBYX-DONE` when the script reached sysprep and
# `BOMBYX-FAILED: <why>` otherwise, so build.sh can tell a finished
# build from a failed one without opening the disk.

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

# A build that still finds updates after this many restarts is stuck,
# most often on one update that fails the same way each time.
$MaxRounds = 12
$RoundFile = 'C:\Windows\Temp\bombyx-rounds'

$Serial = New-Object System.IO.Ports.SerialPort 'COM1', 115200
$Serial.Open()

function Say([string] $Text) {
    $Serial.WriteLine("bombyx: $Text")
}

# Reports why the build failed and shuts the VM down, so build.sh
# stops waiting.
function Fail([string] $Why) {
    $Serial.WriteLine("BOMBYX-FAILED: $Why")
    Stop-Computer -Force
    exit 1
}

# Installs every update Windows Update offers and restarts, or returns
# when none is left. Driver updates are left out: the VM's emulated
# hardware needs none, and a VM made from the box runs on other hosts.
function Install-Updates {
    $round = 1
    if (Test-Path -LiteralPath $RoundFile) {
        $round = [int](Get-Content -LiteralPath $RoundFile) + 1
    }
    if ($round -gt $MaxRounds) {
        Fail "updates were still pending after $MaxRounds rounds"
    }
    Set-Content -LiteralPath $RoundFile -Value $round

    $session = New-Object -ComObject Microsoft.Update.Session
    $searcher = $session.CreateUpdateSearcher()
    Say "update round ${round}: searching"
    $found = $searcher.Search(
        "IsInstalled=0 and IsHidden=0 and Type='Software'").Updates
    if ($found.Count -eq 0) {
        if ((New-Object -ComObject Microsoft.Update.SystemInfo).RebootRequired) {
            Say 'no updates left, but a restart is pending'
            Restart-Computer -Force
            exit 0
        }
        Say 'no updates left'
        return
    }
    $batch = New-Object -ComObject Microsoft.Update.UpdateColl
    foreach ($update in $found) {
        if (-not $update.EulaAccepted) {
            $update.AcceptEula()
        }
        Say "  $($update.Title)"
        [void]$batch.Add($update)
    }
    $downloader = $session.CreateUpdateDownloader()
    $downloader.Updates = $batch
    [void]$downloader.Download()
    $installer = $session.CreateUpdateInstaller()
    $installer.Updates = $batch
    $result = $installer.Install()
    # ResultCode 2 is succeeded and 3 succeeded with errors. A failed
    # update comes back next round, and $MaxRounds ends a loop on it.
    for ($i = 0; $i -lt $batch.Count; $i++) {
        $code = $result.GetUpdateResult($i).ResultCode
        if ($code -ne 2) {
            Say "  result $code for $($batch.Item($i).Title)"
        }
    }
    Say "round ${round}: result $($result.ResultCode); restarting"
    Restart-Computer -Force
    exit 0
}

try {
    $cfg = (Get-Volume -FileSystemLabel BOMBYXCFG).DriveLetter + ':'

    Install-Updates
    $os = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
    Say "patched to build $($os.CurrentBuild).$($os.UBR)"

    # Server 2025 ships the OpenSSH server as an installed capability;
    # installing it covers an image that does not.
    $name = 'OpenSSH.Server~~~~0.0.1.0'
    if ((Get-WindowsCapability -Online -Name $name).State -ne 'Installed') {
        Say 'installing the OpenSSH server capability'
        Add-WindowsCapability -Online -Name $name | Out-Null
    }

    # The first start writes sshd_config and the host keys.
    Start-Service sshd
    Stop-Service sshd
    $sshDir = Join-Path $env:ProgramData 'ssh'
    $config = Join-Path $sshDir 'sshd_config'

    # Three edits to the stock config, placed at the top because
    # sshd_config takes the first value it reads:
    # - Password logins are refused, so the well-known vagrant
    #   password opens only the console.
    # - Only the RSA, ECDSA and Ed25519 host keys are offered; DSA is
    #   a weak key type.
    # - The administrators block is commented out. With it, sshd
    #   reads every administrator's keys from one shared file, while
    #   vagrant's key swap and bombyx's hand-over key both write the
    #   account's own .ssh\authorized_keys.
    Say 'configuring sshd'
    $edited = @(
        'PasswordAuthentication no',
        'HostKey __PROGRAMDATA__/ssh/ssh_host_rsa_key',
        'HostKey __PROGRAMDATA__/ssh/ssh_host_ecdsa_key',
        'HostKey __PROGRAMDATA__/ssh/ssh_host_ed25519_key',
        '')
    foreach ($line in Get-Content -LiteralPath $config) {
        if ($line -match '^\s*Match\s+Group\s+administrators' -or
            $line -match '^\s*AuthorizedKeysFile\s+__PROGRAMDATA__') {
            $edited += "#$line"
        } else {
            $edited += $line
        }
    }
    Set-Content -LiteralPath $config -Value $edited -Encoding ASCII

    # Each VM made from the box generates its own host keys at its
    # first sshd start, rather than all sharing the build's.
    Get-ChildItem -Path $sshDir -Filter 'ssh_host_*' | Remove-Item -Force

    Set-Service -Name sshd -StartupType Automatic
    # The rule applies to every network profile. Server 2025 ships it
    # limited to the Private profile, and Windows puts a VM's libvirt
    # network in the Public one, where sshd's port would stay closed.
    $rule = 'OpenSSH-Server-In-TCP'
    if ($null -eq (Get-NetFirewallRule -Name $rule -ErrorAction SilentlyContinue)) {
        New-NetFirewallRule -Name $rule -DisplayName 'OpenSSH Server (sshd)' `
            -Enabled True -Profile Any -Direction Inbound -Protocol TCP `
            -Action Allow -LocalPort 22 | Out-Null
    } else {
        Set-NetFirewallRule -Name $rule -Enabled True -Profile Any
    }

    # vagrant's well-known insecure key, which vagrant replaces with a
    # generated one at a VM's first boot. build.sh copies it from the
    # vagrant install on the build host.
    Say "authorizing vagrant's insecure key"
    $keysDir = 'C:\Users\vagrant\.ssh'
    New-Item -ItemType Directory -Force -Path $keysDir | Out-Null
    $keys = Join-Path $keysDir 'authorized_keys'
    Copy-Item -LiteralPath (Join-Path $cfg 'vagrant.pub') -Destination $keys
    $out = & icacls.exe $keys /inheritance:r /grant:r 'vagrant:F' `
        '*S-1-5-18:F' '*S-1-5-32-544:F' 2>&1
    if ($LASTEXITCODE -ne 0) {
        Fail "could not set the permissions on ${keys}: $out"
    }

    # The built-in Administrator is left with a random password nobody
    # holds, and disabled. Windows runs SetupComplete.cmd at the end of
    # each VM's first boot, after the answer file has set that
    # password, which also enables the account.
    $scripts = 'C:\Windows\Setup\Scripts'
    New-Item -ItemType Directory -Force -Path $scripts | Out-Null
    Set-Content -LiteralPath (Join-Path $scripts 'SetupComplete.cmd') `
        -Encoding ASCII -Value @(
            '@echo off',
            'rem Written by bombyx build: the vagrant account is the way in.',
            'net user Administrator /active:no')

    # Smaller box: superseded update files go, and freed blocks are
    # handed back to the disk image, which build.sh then copies
    # without them.
    Say 'cleaning up'
    Unregister-ScheduledTask -TaskName 'bombyx-build' -Confirm:$false
    # This script too: PowerShell read the whole file before it ran.
    Remove-Item -LiteralPath $RoundFile, $PSCommandPath -Force
    & dism.exe /Online /Cleanup-Image /StartComponentCleanup /ResetBase |
        Out-Null
    Optimize-Volume -DriveLetter C -ReTrim

    # Generalize, so each VM gets its own identity and its own 10-day
    # evaluation grace from its first boot, not from the build. The
    # answer file completes that first boot's setup unattended.
    $oobe = 'C:\Windows\Panther\bombyx-oobe.xml'
    Copy-Item -LiteralPath (Join-Path $cfg 'unattend-oobe.xml') -Destination $oobe
    Say 'BOMBYX-DONE'
    & "$env:SystemRoot\System32\Sysprep\sysprep.exe" /generalize /oobe `
        /shutdown /quiet "/unattend:$oobe"
} catch {
    Fail $_.Exception.Message
}
