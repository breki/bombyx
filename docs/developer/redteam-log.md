# Red Team Findings -- Deferred backlog

Security (Red Team) review findings. Newest first.

## rt-2026-08-11-toml-error-echoes-source

**Category:** Information disclosure

A `toml` parse error renders the offending source line into its
`Display`, and bombyx surfaces that straight to stderr:

```
bombyx: loading bombyx.toml: invalid config in bombyx.toml:
TOML parse error at line 1, column 12
  |
1 | -----BEGIN OPENSSH PRIVATE KEY-----
```

Reproduced against the built binary during the review of the
config-overlay change.

The overlay half of this is closed: `read_optional` now refuses
anything that is not a regular file, judged with
`symlink_metadata`, so a repo cannot commit `bombyx.local.toml`
as a symlink to `~/.ssh/id_ed25519` and have a line of it
printed. The base config is not: `bombyx.toml` itself can be a
symlink, and the operator has no reason to inspect it after a
clone.

Deferred rather than fixed because the fix is a change to how
*every* parse error is reported -- line and column without the
snippet -- and that trades away the diagnostic that makes a
malformed config easy to correct. Worth doing deliberately, with
the error text designed rather than truncated.

The residual exposure is one line of a file the invoking user
can already read, on a machine that has just checked out a
hostile repo. Real, but smaller than the `vagrant_dir` primitive
found alongside it, which shipped a fix in the same commit.
