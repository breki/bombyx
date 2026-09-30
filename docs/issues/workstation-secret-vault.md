# workstation-secret-vault (#149)

Working plan for issue #149. Removed when the work lands; the PR
body and `docs/trust-boundary.md` keep what lasts.

## Decisions (operator, 2026-09-30)

- **One KeePassXC entry per secret.** `[source.vault]` names the
  database and maps each variable name to an entry path. bombyx
  reads each entry's `Password` attribute and assembles a
  `.env`-shaped `Secrets` from them, so everything downstream of
  `Config::read_staged` is unchanged.
- **One unlock, through `keepassxc-cli open`'s interactive shell.**
- **bombyx never holds the master password.** bombyx starts
  `keepassxc-cli open` and a `sh` script, joined by an OS pipe it
  never reads. The script reads the password from `/dev/tty` with
  echo off, writes it into the pipe, then forwards bombyx's `show`
  commands with `cat`. One `sh -c 'a | keepassxc-cli'` deadlocks
  on a wrong password, because the parent shell holds bombyx's
  stdout open.
- **`env_file` or `vault`, never both.** Refused at parse time.
- **`deploy_key` stays out of scope.** It never touches the
  workstation (`docs/trust-boundary.md`).
- **Linux (and any `sh` workstation) first.** On Windows a config
  naming `vault` is refused when bombyx reads it; the PowerShell
  wrapper is a follow-up issue.

## Measured (keepassxc-cli 2.7.6, Linux)

- Password prompt and errors go to stderr; the shell prompt, an
  echo of each command, and each value go to stdout.
- The shell prompt is the database's stored name followed by
  `> ` (`My Vault> `), or its file name when the stored name is
  empty, as `db-create` leaves it. So the driver learns the
  prompt from the first one rather than predicting it.
- Password read buffers stdin: commands written before the first
  prompt are swallowed. The driver waits for the prompt.
- Wrong password: exit 1, no prompt on stdout.
- Found entry: echo line, then the value and `\n`. Empty value:
  echo line, then `\n`. Missing entry: echo line only.

## Not verified

- A real terminal (the tests used a file in place of `/dev/tty`).
- Windows, and whether its `keepassxc-cli` echoes commands.
- A password attribute holding a newline.
