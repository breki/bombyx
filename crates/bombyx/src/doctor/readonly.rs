//! Checking that a probe script would not change the host.
//!
//! `doctor` promises to create, delete and modify nothing, and
//! this module is the mechanism behind that promise.
//!
//! It reads the text of bombyx's own scripts. What an invoked
//! tool then does is outside its reach; see
//! [`MUTATING_COMMANDS`] for the precise claim.

/// Commands whose purpose is to write, matched on **word
/// boundaries**.
///
/// One list, shared by the unit test over the probe builders and
/// the CLI-level test over the rendered dry run. Two separately
/// maintained lists were the original problem: they disagreed
/// about what read-only meant, so each test proved something
/// slightly different and weaker.
///
/// Matching is by word, not by substring. An earlier version
/// listed `"rm "` and `"> "` with trailing spaces -- the ordinary
/// spelling, and not the only one. `>file`, `1>file`, `>|file`
/// and a tab-separated `rm` all slipped past while the tests read
/// as though the whole family were covered.
///
/// # What this does not claim
///
/// It inspects the text of **bombyx's own script** and nothing
/// else. It says nothing about what an invoked tool does once it
/// runs; see `remote::probe::provider`, where
/// `vagrant plugin list` initialises `~/.vagrant.d` on a host
/// where vagrant has never run. A passing
/// `no_probe_changes_the_host` therefore means "no probe reaches
/// for a tool whose purpose is to write", not "doctor leaves the
/// host byte-identical".
///
/// A blocklist, knowingly. A real allowlist needs a shell parser
/// to find the command in every segment of a script, and a parser
/// that is subtly wrong inspires more confidence than this list
/// while catching less.
const MUTATING_COMMANDS: &[&str] = &[
    "mkdir",
    "rmdir",
    "rm",
    "touch",
    "unzip",
    "scp",
    "cp",
    "mv",
    "dd",
    "ln",
    "chmod",
    "chown",
    "truncate",
    "tee",
    "install",
    "mkfifo",
    "mknod",
    "sed",
    "git",
    "apt",
    "apt-get",
    "systemctl",
    "tar",
];

/// `vagrant` subcommands that change something.
///
/// Enumerated rather than allow-listing the read-only ones,
/// because `vagrant` grows subcommands and a new one is likelier
/// to write than not. `plugin` is split further below, since
/// `plugin list` is the one bombyx actually needs.
const MUTATING_VAGRANT: &[&str] = &[
    "up",
    "destroy",
    "halt",
    "reload",
    "provision",
    "snapshot",
    "init",
    "box",
    "suspend",
    "resume",
    "package",
    "upload",
    "push",
];

/// `vagrant plugin` subcommands that change something.
const MUTATING_VAGRANT_PLUGIN: &[&str] = &[
    "install",
    "uninstall",
    "update",
    "repair",
    "expunge",
    "license",
];

/// Words that precede a command rather than being one.
///
/// A segment can open with a keyword (`then rm -rf x`) or with
/// environment assignments (`LC_ALL=C rm -rf x`), so the command
/// is not always the first word.
const NOT_A_COMMAND: &[&str] = &[
    "if", "then", "else", "elif", "fi", "while", "until", "do", "done", "for",
    "case", "esac", "in", "!", "time", "exec", "eval",
];

/// Commands that run *another* command, so the interesting word
/// is further along the segment.
///
/// Without this the guard stops at the wrapper and never looks
/// past it, and `sudo mkdir -p "$d"` reads as read-only. That is
/// worse than a gap: `sudo` in front of `systemctl`, `apt` or
/// `mkdir` is exactly what a probe author reaches for, so the
/// blind spot sat precisely where the command list was aimed. The
/// substring version this replaced did catch these.
const TRANSPARENT_PREFIX: &[&str] = &[
    "sudo", "doas", "env", "command", "nohup", "nice", "ionice", "setsid",
    "stdbuf", "xargs", "timeout",
];

/// Shells, which hide whatever `-c` hands them.
///
/// A probe running `sh -c '<anything>'` cannot be judged by
/// reading the outer script, so the wrapper itself is treated as
/// the objection rather than pretended to be read-only.
const SHELL_COMMANDS: &[&str] = &["sh", "bash", "dash", "zsh", "ksh", "ash"];

/// Splits a script into segments, each a list of words.
///
/// Every character that can end one command and begin another is
/// a separator, `(` included -- that is what makes the `vagrant`
/// inside `out=$(vagrant plugin list)` the start of its own
/// segment rather than an argument of the assignment.
fn command_segments(script: &str) -> Vec<Vec<&str>> {
    script
        .split(|c: char| {
            matches!(c, ';' | '|' | '&' | '(' | ')' | '`' | '{' | '}' | '\n')
        })
        .map(|seg| seg.split_whitespace().collect::<Vec<&str>>())
        .filter(|words| !words.is_empty())
        .collect()
}

/// The command a segment runs, and its arguments.
///
/// Leading keywords and `VAR=value` assignments are skipped. A
/// segment that is nothing but assignments (`p=$d`) runs no
/// command and yields `None`.
///
/// A wrapper from [`TRANSPARENT_PREFIX`] is stepped past, along
/// with its own flags, so the command it runs is the one judged.
///
/// # What this does not see
///
/// It is not a shell parser. A command assembled by expansion, or
/// reached through `find . -delete`, is invisible to it. `xargs`
/// and the shells are handled -- the first as a transparent
/// prefix, the second by objecting to the wrapper -- but the
/// general case is not solvable here. The guard covers a probe
/// author writing a mutating command, which is the realistic
/// mistake; it is not a sandbox.
fn command_of<'a>(words: &'a [&'a str]) -> Option<(&'a str, &'a [&'a str])> {
    let mut i = 0;
    while i < words.len() {
        let word = base_name(words[i]);
        if NOT_A_COMMAND.contains(&word) || is_assignment(word) {
            i += 1;
            continue;
        }
        if TRANSPARENT_PREFIX.contains(&word) {
            i += 1;
            // The wrapper's own flags, then -- for `timeout` --
            // its duration, which is a bare word and would
            // otherwise be read as the command.
            let mut is_lookup = false;
            while i < words.len() && words[i].starts_with('-') {
                // `command -v tar` asks *where* `tar` is and runs
                // nothing, so the name after it is not a command
                // being invoked. Every `command -v` probe bombyx
                // has would otherwise be reported as running the
                // tool it is only looking for.
                is_lookup |=
                    word == "command" && matches!(words[i], "-v" | "-V");
                i += 1;
            }
            if is_lookup {
                return None;
            }
            if word == "timeout" && i < words.len() {
                i += 1;
            }
            continue;
        }
        return Some((words[i], &words[i + 1..]));
    }
    None
}

/// A word reduced to the name it invokes.
///
/// Strips a leading path so `/bin/rm` is judged as `rm`, and
/// surrounding quotes so `'rm'` is too -- quoting the command word
/// is the cheapest way to slip a name past a comparison.
fn base_name(word: &str) -> &str {
    let bare = word.trim_matches(|c| matches!(c, '\'' | '"'));
    bare.rsplit(['/', '\\']).next().unwrap_or(bare)
}

/// Whether `word` is a `VAR=value` assignment rather than a
/// command.
fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// Whether the text after a `>&` names a file descriptor rather
/// than a file.
///
/// This distinction is the whole correctness of the redirection
/// check, and getting it wrong is a two-character bypass.
/// `>&2` and `>&-` duplicate a descriptor and create nothing;
/// bare `>&word` is a documented bash synonym for `&>word` and
/// **truncates the file** `word`. Treating every `>&` as a
/// duplication let `vagrant plugin list >&out.txt` through.
fn is_descriptor(after_amp: &str) -> bool {
    let token: String = after_amp
        .chars()
        .take_while(|c| {
            !c.is_whitespace() && !matches!(c, ';' | '|' | '&' | ')' | '}')
        })
        .collect();
    token == "-"
        || (!token.is_empty() && token.chars().all(|c| c.is_ascii_digit()))
}

/// Up to eight characters of `text`, for a failure message.
///
/// Characters, not bytes. Slicing at `i + 8` bytes panics when a
/// multi-byte character straddles the boundary, and this runs over
/// script text that can carry a non-ASCII path.
fn excerpt(text: &str) -> String {
    text.chars()
        .take(8)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

/// The write-capable redirection in `script`, if any.
///
/// Structural rather than a list of spellings: any `>` opens a
/// file for writing, with three exceptions that are not
/// redirections at all --- a descriptor duplication (`2>&1`,
/// `>&2`), the comparison `>=`, and the arrow `->`.
///
/// Quoted runs are skipped. Without that, a probe printing
/// `'expected >= 2 vCPUs'` is reported as writing a file, and a
/// guard that misfires on ordinary prose is a guard the next
/// author relaxes rather than obeys.
fn redirection_that_writes(script: &str) -> Option<String> {
    let mut quote: Option<char> = None;
    for (i, c) in script.char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        if matches!(c, '\'' | '"') {
            quote = Some(c);
            continue;
        }
        if c != '>' {
            continue;
        }
        // `->`: the previous character makes it an arrow.
        if script[..i].ends_with('-') {
            continue;
        }
        let rest = &script[i + 1..];
        // `>>file` writes just as `>file` does; step past the
        // second `>` to read what follows either form.
        let after = rest.strip_prefix('>').unwrap_or(rest);
        // `>=`: a comparison.
        if after.starts_with('=') {
            continue;
        }
        if let Some(dup) = after.strip_prefix('&')
            && is_descriptor(dup)
        {
            continue;
        }
        return Some(excerpt(&script[i..]));
    }
    None
}

/// The mutating `vagrant` use in `args`, if any.
///
/// The subcommand is the first non-flag word. `ssh` is judged on
/// its flags rather than its name: `vagrant ssh` alone is
/// interactive and harmless here, while `vagrant ssh -c '<cmd>'`
/// runs an arbitrary command inside the guest, and what that
/// command is cannot be read from the outer script.
fn mutating_vagrant_use(args: &[&str]) -> Option<String> {
    let mut words = args.iter().filter(|w| !w.starts_with('-'));
    let sub = words.next()?;
    if *sub == "plugin" {
        let action = words.next()?;
        return MUTATING_VAGRANT_PLUGIN
            .contains(action)
            .then(|| format!("plugin {action}"));
    }
    if *sub == "ssh" && args.contains(&"-c") {
        return Some("ssh -c".to_owned());
    }
    MUTATING_VAGRANT.contains(sub).then(|| (*sub).to_owned())
}

/// The first sign in `script` that it would change the host.
///
/// Returns the offending word so a failure names what it objected
/// to, not merely that it objected.
#[must_use]
pub fn mutating_token(script: &str) -> Option<String> {
    if let Some(redirect) = redirection_that_writes(script) {
        return Some(format!("redirection {redirect}"));
    }
    for words in command_segments(script) {
        let Some((command, args)) = command_of(&words) else {
            continue;
        };
        let bare = base_name(command);
        if MUTATING_COMMANDS.contains(&bare) {
            return Some(command.to_owned());
        }
        if SHELL_COMMANDS.contains(&bare) && args.contains(&"-c") {
            return Some(format!("{bare} -c"));
        }
        if bare == "vagrant"
            && let Some(found) = mutating_vagrant_use(args)
        {
            return Some(format!("vagrant {found}"));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutating_token_catches_the_whole_family_not_one_spelling() {
        // The guard is only worth having if it fires, and the
        // earlier substring version fired on exactly the
        // spellings it happened to list. Each line below is a
        // form that slipped past it.
        for (script, want) in [
            // Trailing-space matching missed every one of these.
            ("printf x >out", "redirection >out"),
            ("printf x 1>out", "redirection >out"),
            ("printf x >|out", "redirection >|out"),
            ("printf x >>out", "redirection >>out"),
            ("printf x >\tout", "redirection >\tout"),
            // A path-qualified command, and a tab separator.
            ("/bin/rm -rf x", "/bin/rm"),
            ("rm\t-rf x", "rm"),
            // Reached through a keyword or an assignment rather
            // than at the start of the segment.
            ("if true; then rm -rf x; fi", "rm"),
            ("LC_ALL=C rm -rf x", "rm"),
            ("out=$(mkdir -p y)", "mkdir"),
            // vagrant subcommands beyond the four once listed.
            ("vagrant init", "vagrant init"),
            ("vagrant box add x", "vagrant box"),
            ("vagrant plugin uninstall x", "vagrant plugin uninstall"),
            ("vagrant upload f g", "vagrant upload"),
            // Not the subcommand's name but its flags: `-c` runs
            // an arbitrary command inside the guest.
            ("vagrant ssh -c 'rm -rf /vagrant'", "vagrant ssh -c"),
            ("cd x && mkdir -p y", "mkdir"),
            // A wrapper that runs another command. Stopping at
            // the wrapper made `sudo mkdir` read as read-only --
            // and `sudo` in front of `mkdir`, `apt` or `systemctl`
            // is exactly what a probe author reaches for.
            ("sudo mkdir -p \"$d\"", "mkdir"),
            ("sudo systemctl restart libvirtd", "systemctl"),
            ("env mkdir -p y", "mkdir"),
            ("command rm -f f", "rm"),
            ("nohup tar cf a.tar .", "tar"),
            ("timeout 5 rm -rf x", "rm"),
            ("xargs rm", "rm"),
            // Quoting the command word is the cheapest bypass.
            ("'rm' -rf x", "'rm'"),
            ("\"mkdir\" -p y", "\"mkdir\""),
            // A shell hides its payload, so the wrapper itself is
            // the objection.
            ("sh -c 'rm -rf /'", "sh -c"),
            // `>&word` is a bash synonym for `&>word`: it
            // truncates the file. Only a descriptor is harmless.
            ("vagrant plugin list >&out.txt", "redirection >&out.tx"),
            ("printf x >>&out", "redirection >>&out"),
        ] {
            assert_eq!(
                mutating_token(script).as_deref(),
                Some(want),
                "{script:?}"
            );
        }
    }

    #[test]
    fn mutating_token_leaves_a_read_only_script_alone() {
        // The other half: a guard that flags everything is as
        // useless as one that flags nothing. `tar` and `scp` are
        // mutating commands appearing here as *arguments*, which
        // is why the check reads the command word of each segment
        // rather than every word.
        for script in [
            "command -v 'tar'",
            "command -v 'scp'",
            "true",
            "x=1; if [ \"$x\" = 1 ]; then printf 'posix\\n'; fi",
            "out=$(VAGRANT_CHECKPOINT_DISABLE=1 vagrant plugin list 2>&1) \
             || { printf 'failed\\n%s\\n' \"$out\" >&2; exit 1; }",
            "echo \"$p is not writable\" >&2",
            // Descriptor duplication, which creates nothing.
            "printf x >&2",
            "printf x >&-",
            "exec 3>&1",
            // A `>` that is not a redirection at all. Misfiring
            // on ordinary prose is how a guard gets relaxed, and
            // the relaxation is what widens the real hole.
            "printf 'expected >= 2 vCPUs\\n'",
            "echo \"$p -> $d\"",
            "vagrant ssh",
            "vagrant plugin list",
            "vagrant status",
        ] {
            assert_eq!(mutating_token(script), None, "{script:?}");
        }
    }

    #[test]
    fn mutating_token_answers_rather_than_panicking() {
        // It is a `pub` function reading script text that can
        // carry a non-ASCII path. Taking eight *bytes* of context
        // after the `>` panicked when a character straddled the
        // boundary; eight characters cannot.
        assert!(mutating_token(">\u{3b1}\u{3b1}\u{3b1}\u{3b1}").is_some());
        assert!(mutating_token("echo \u{65e5}\u{672c}\u{8a9e}").is_none());
        assert_eq!(mutating_token(""), None);
        assert_eq!(mutating_token("   ;;  && ||"), None);
    }
}
