# Red Team Findings -- Deferred backlog

Security (Red Team) review findings. Newest first.

## rt-2026-08-18-verified-archive-reread-before-extract

**Category:** Time-of-check to time-of-use

`self-update` hashes the downloaded archive with
`std::fs::read(&archive_path)` and then hands the same *path* to
`tar`, so the bytes that were verified and the bytes that are
extracted are two separate reads. Nothing pins the file between
them: no handle is held, and it is not renamed to a private name
after verification. A process able to write that path in the window
gets an unverified binary installed, with
`bombyx: <archive> matches its published checksum` printed
immediately before.

The window is narrow and the mitigation is real but *incidental*:
`tempfile::TempDir` creates the directory mode `0o700` on Unix and
inside the per-user `%LOCALAPPDATA%\Temp` on Windows, so an
attacker must already be the same user or root. Recorded rather
than fixed blind, because the mitigation comes from `tempfile`'s
defaults rather than from a decision made here, and nothing in the
code says so.

Closing it properly means extracting from a handle opened before
hashing, or re-hashing the extracted binary. Worth doing when the
release publishes a digest of the binary itself and not only of the
archive.

## rt-2026-08-18-replayed-tag-redefines-a-version

**Category:** Release integrity

The release workflow now updates an existing release in place and
uploads assets with `--clobber`, so a re-pushed tag silently
redefines what a published version *is*. `update::decide` compares
only `MAJOR.MINOR.PATCH`, so it has no notion of "this version's
bytes changed": someone who installed the old `v0.2.0` is reported
`UpToDate` and never receives the replacement, and someone
mid-download gets `VerifyError::Mismatch`, whose message asserts
the bytes "are not the ones that were released" -- pointing at
tampering when the cause was a re-push.

Idempotency was added for a real reason (a history rewrite moved
both tags, and the release job failed on an otherwise green build),
but that is a property of this repository's habits rather than of
releases. The honest options are to refuse clobbering assets on a
release that already has them and require a new patch tag, or to
say in `README.md` that a replaced version is not picked up by
`self-update`. Neither is done yet.

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
