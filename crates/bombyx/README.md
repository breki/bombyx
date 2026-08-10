# bombyx

Drive isolated AI-agent VMs on a remote libvirt host over
SSH.

Running an AI coding agent on your daily driver puts your
password manager, SSH keys, cloud credentials and browser
profiles one prompt injection away from exfiltration. bombyx
is the control plane for the alternative: it runs `vagrant`
on a separate VM host over SSH, so the agent works inside a
VM with its own kernel, no host filesystem, and no
credentials.

The project repo holds the `vagrant/` directory and is the
source of truth; `bombyx up` pushes it to the host before
booting, so the host cannot silently drift.

```bash
bombyx up                 # push vagrant/, boot the VM
bombyx provision          # push vagrant/, re-run provisioning
bombyx shell              # open a shell inside the VM
bombyx status             # vagrant status on the host
bombyx reset              # restore the fresh-install snapshot
bombyx down               # halt the VM

bombyx scratch pr-1234    # boot a throwaway VM
bombyx discard pr-1234    # destroy it
```

Every command accepts `--dry-run`, which prints the exact
`ssh`/`scp`/`tar` invocations instead of running them.

See the [repository](https://github.com/breki/bombyx) for
configuration, the isolation strategy, and development
instructions.

## License

MIT -- see [LICENSE](../../LICENSE).
