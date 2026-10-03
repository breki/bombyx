# Runs as SYSTEM at every boot of build.sh's install, from the
# `bombyx-build` startup task first-logon.ps1 registers.
#
# Each run installs the Windows updates still missing and restarts,
# through Windows' own update service. When none are left, it sets up
# sshd for vagrant's winssh communicator, cleans up, generalizes the
# install and shuts the VM down, which ends the build.
#
# Progress goes to COM1, which build.sh records in its log. Two lines
# end a build, each at the start of a line: `BOMBYX-DONE` when the
# script hands the VM to sysprep, and `BOMBYX-FAILED: <why>` when a
# step failed, sysprep included. build.sh reads a failure first, so it
# can tell a finished build from a failed one without opening the
# disk.

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
    #   password does not open sshd. WinRM, the other remote logon a
    #   server enables, is disabled below.
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

    # The built-in Administrator gets a fresh random password, made here
    # and written nowhere, and is then disabled. The install's password
    # may survive in the image, in freed disk blocks or the page file,
    # so replacing it leaves any such copy stale rather than trying to
    # erase every one. WinRM is disabled too: bombyx reaches the guest
    # over SSH alone, and WinRM would otherwise let another VM on the
    # same network try the well-known vagrant password.
    Say 'disabling Administrator and WinRM'
    $bytes = New-Object byte[] 24
    [Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($bytes)
    ([ADSI]'WinNT://./Administrator,user').SetPassword(
        [Convert]::ToBase64String($bytes) + 'aA1!')
    $bytes = $null
    & net.exe user Administrator /active:no | Out-Null
    if ($LASTEXITCODE -ne 0) {
        Fail "could not disable the Administrator account"
    }
    Stop-Service -Name WinRM -Force
    Set-Service -Name WinRM -StartupType Disabled
    Get-NetFirewallRule -Name 'WINRM-HTTP-In-TCP*' -ErrorAction SilentlyContinue |
        Disable-NetFirewallRule

    # Windows runs SetupComplete.cmd at the end of each VM's first boot.
    # It disables Administrator again, as a precaution: nobody has seen
    # setup enable the account, and the audited VMs showed it disabled.
    # It also deletes bombyx's answer file, which holds no secret, so
    # no file of the build is left in the VM.
    $oobe = 'C:\Windows\Panther\bombyx-oobe.xml'
    $scripts = 'C:\Windows\Setup\Scripts'
    New-Item -ItemType Directory -Force -Path $scripts | Out-Null
    Set-Content -LiteralPath (Join-Path $scripts 'SetupComplete.cmd') `
        -Encoding ASCII -Value @(
            '@echo off',
            'rem Written by bombyx build: the vagrant account is the way in.',
            'net user Administrator /active:no',
            "del /f /q $oobe")

    Say 'cleaning up'
    Unregister-ScheduledTask -TaskName 'bombyx-build' -Confirm:$false
    # This script too: PowerShell read the whole file before it ran.
    Remove-Item -LiteralPath $RoundFile, $PSCommandPath -Force
    # The answer files setup cached from the install hold the install's
    # Administrator password, stale since the reset above. They are
    # removed so no file in the box names it. A file Windows holds
    # locked is reported and left, rather than failing the build after
    # its update rounds, because the password it names opens nothing.
    Get-ChildItem -Path 'C:\Windows\Panther' -Recurse -Include '*.xml' -File `
            -ErrorAction SilentlyContinue |
        Where-Object {
            Select-String -LiteralPath $_.FullName -Pattern '<PlainText>' `
                -Quiet -ErrorAction SilentlyContinue
        } |
        ForEach-Object {
            $file = $_.FullName
            Say "removing $file"
            try {
                Remove-Item -LiteralPath $file -Force
            } catch {
                Say ("could not remove $file, which names " +
                    "only the stale password: $($_.Exception.Message)")
            }
        }
    # Clean-up for size: superseded update files go, and freed blocks
    # are handed back to the disk image, which build.sh then copies
    # without them. Only a smaller box depends on it, so a failure of
    # dism or of the trim is reported and the build goes on. 'Continue'
    # inside the block, because under 'Stop' Windows PowerShell 5.1
    # turns a redirected native stderr line into an error that ends the
    # script.
    $out = & {
        $ErrorActionPreference = 'Continue'
        & dism.exe /Online /Cleanup-Image /StartComponentCleanup /ResetBase 2>&1
    }
    if ($LASTEXITCODE -ne 0) {
        Say "dism clean-up exited ${LASTEXITCODE}; continuing: $($out | Select-Object -Last 1)"
    }
    Optimize-Volume -DriveLetter C -ReTrim -ErrorAction SilentlyContinue `
        -ErrorVariable trimError
    if ($trimError) {
        Say "trimming the disk failed; continuing: $trimError"
    }

    # Generalize, so each VM gets its own identity and its own 10-day
    # evaluation grace from its first boot, not from the build. The
    # answer file completes that first boot's setup unattended.
    Copy-Item -LiteralPath (Join-Path $cfg 'unattend-oobe.xml') -Destination $oobe
    $Serial.WriteLine('BOMBYX-DONE')
    # sysprep.exe is a GUI program, so `&` would neither wait for it nor
    # set $LASTEXITCODE; Start-Process -Wait does both. Its exit status
    # is checked because a failed sysprep would otherwise leave the VM
    # running until build.sh's timeout. On success sysprep shuts the VM
    # down, so this script may never see it return.
    $sysprep = Start-Process -Wait -PassThru `
        -FilePath "$env:SystemRoot\System32\Sysprep\sysprep.exe" `
        -ArgumentList '/generalize', '/oobe', '/shutdown', '/quiet',
            "/unattend:$oobe"
    if ($sysprep.ExitCode -ne 0) {
        Fail ("sysprep exited $($sysprep.ExitCode); see " +
            'C:\Windows\System32\Sysprep\Panther\setuperr.log')
    }
} catch {
    Fail $_.Exception.Message
}
