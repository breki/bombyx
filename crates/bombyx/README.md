# bombyx

Drive isolated AI-agent VMs on a libvirt host -- usually a
second machine reached over SSH, or this one when `host` names
it.

Running an AI coding agent on your daily driver puts your
password manager, SSH keys, cloud credentials and browser
profiles one prompt injection away from exfiltration. bombyx
is the control plane for the alternative: it runs `vagrant`
on a VM host -- usually a separate machine reached over SSH --
so the agent works inside a VM with its own kernel, no host
filesystem, and none of your credentials. A VM that clones a
private repository needs a credential of its own, and code
inside it can read that one --
see `docs/trust-boundary.md` in the repository for what is
accepted there and why.

bombyx generates the Vagrantfile and a bootstrap script from
your own `config.toml` and writes them onto the VM host on
every boot, so that machine cannot drift and holds none of your
code. The guest clones the project itself.

Every command but `self-update` takes `--project <name>`, which
picks a `[projects.<name>]` table in your own `config.toml`.
Left out below for brevity:

```bash
bombyx up                 # write the generated files, boot the VM
bombyx provision          # re-run provisioning in the guest
bombyx shell              # open a shell inside the VM
bombyx status             # vagrant status on the host
bombyx reset              # restore the fresh-install snapshot
bombyx snapshot           # save the fresh-install snapshot
bombyx down               # halt the VM

bombyx scratch pr-1234    # boot a throwaway VM
bombyx discard pr-1234    # destroy it
```

Every command accepts `--dry-run`, which prints the exact
invocations instead of running them: `ssh <host> "..."`, or
`sh -c "..."` when `host` names the machine you are on.

See the [repository](https://github.com/breki/bombyx) for
configuration, the isolation strategy, and development
instructions.

## License

MIT -- see [LICENSE](../../LICENSE).
