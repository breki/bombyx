//! The `[source]` table: where the guest fetches the project
//! from, and the three checked types that hold the values it
//! hands to `git`.
//!
//! Those three are `repo`, `ref` and `script`, and all three are
//! *newtypes* -- a struct wrapping one private `String`,
//! buildable only through a function that checks the value
//! first. [`RepoUrl`] explains the pattern in full; read that
//! one first. All three reach `git` and the guest's shell, so
//! they carry the checks that cannot be expressed as "a
//! non-empty string".
//!
//! Four more keys are optional. Two of them are paths rather
//! than something the guest hands to `git`, so each has its own
//! module and its own rules.
//!
//! `deploy_key` lives in `super::deploy_key`. It names a file on
//! the VM host, which `vagrant` opens.
//!
//! `env_file` lives in `super::env_file`. It names a file on the
//! workstation, which bombyx opens itself -- and that difference
//! is what makes its rules unlike the other path's.
//!
//! `repo_token` and `repo_user` are the other two, and they live
//! in `super::repo_token`. Neither is a path. They name a
//! variable inside the `env_file` and the username `git` sends
//! the value under, and they arrive here as one field rather
//! than two, because `super::RepoToken` holds both.

use serde::Deserialize;

use super::deploy_key::DeployKeyPath;
use super::env_file::EnvFilePath;
use super::error::FieldError;
use super::guards;
use super::repo_token::{RepoToken, RepoTokenVar, RepoUser};
use crate::newtype::{
    checked_str_newtype, checked_str_parse, checked_str_try_from,
};

/// Where the guest fetches the project from, as `[source]`.
///
/// The guest clones this itself, so the first three keys are
/// not paths on the workstation or the VM host -- see
/// `docs/trust-boundary.md`. `deploy_key` is the exception,
/// and its own module says why.
/// **Serde does not read this struct.** It reads the private
/// `SourceFields` below, whose fields carry the `rename` and
/// `default` attributes, and converts. So a new key is declared
/// there as well as here, and the attribute belongs on that
/// copy: one written here would be read by nothing. (Not a
/// rustdoc link, because a public page may not link to a
/// private item.)
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "SourceFields")]
pub struct Source {
    /// Repository the guest clones.
    pub repo: RepoUrl,
    /// Branch or tag to clone.
    ///
    /// Named `git_ref` because `ref` is a Rust keyword.
    pub git_ref: GitRef,
    /// Provisioning script to run, relative to the clone root.
    pub script: ScriptPath,
    /// Private key on the VM host that the guest clones a
    /// private repository with.
    ///
    /// `None` when the config names none, which is what a
    /// public repository wants: `vagrant` then uploads nothing
    /// and the guest clones without a credential.
    pub deploy_key: Option<DeployKeyPath>,
    /// File on the **workstation** holding the project's
    /// secrets, which bombyx carries into the guest.
    ///
    /// The other path in this table, `deploy_key`, names a file
    /// on the VM host. This one names one on the machine bombyx
    /// runs on, and `super::EnvFilePath` holds why that changes
    /// every rule.
    ///
    /// `None` when the config names none: bombyx then writes no
    /// secrets file and the guest gets an empty
    /// `BOMBYX_ENV_FILE`.
    pub env_file: Option<EnvFilePath>,
    /// How the guest authenticates an https clone: the
    /// variable inside `env_file` holding the token, and the
    /// username it is sent under.
    ///
    /// A name, never the token. `None` for a repository that
    /// needs no credential to clone, and for one that clones
    /// over ssh with a `deploy_key` instead.
    ///
    /// The config file spells two keys, `repo_token` and
    /// `repo_user`. One field rather than two `Option`s because
    /// neither key means anything alone, and `super::RepoToken`
    /// says why the split state is not worth being able to
    /// write down.
    pub repo_token: Option<RepoToken>,
}

/// The `[source]` table as TOML spells it, before the rules
/// that span more than one key have run.
///
/// Every rule belonging to one value is already a type, and
/// serde has run it by the time this struct exists. What is
/// left are the rules about keys *agreeing* with each other,
/// and no single type can hold one of those.
///
/// [`Source`] deserializes through this struct rather than
/// directly, so a config breaking one of those rules is refused
/// while the file is parsing and the message names the line.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceFields {
    repo: RepoUrl,
    #[serde(rename = "ref")]
    git_ref: GitRef,
    script: ScriptPath,
    #[serde(default)]
    deploy_key: Option<DeployKeyPath>,
    #[serde(default)]
    env_file: Option<EnvFilePath>,
    #[serde(default)]
    repo_token: Option<RepoTokenVar>,
    #[serde(default)]
    repo_user: Option<RepoUser>,
}

impl TryFrom<SourceFields> for Source {
    type Error = FieldError;

    /// Runs the four rules that span more than one key.
    ///
    /// `repo_token` and `repo_user` are stated together or not
    /// at all. Each is useless without the other: a variable
    /// name with no username leaves bombyx guessing the literal,
    /// which is what naming it in the config exists to avoid,
    /// and a username with no variable name has no token to go
    /// with.
    ///
    /// `repo_token` requires `env_file`, because that file is
    /// where the variable is read from. Without it bombyx would
    /// have nothing to look in.
    ///
    /// `repo_token` requires an `https` repository. `git`
    /// sends the token to the server on every request, so an
    /// `http` URL would put it on the wire in plain text, and
    /// an `ssh` URL never asks for one at all.
    ///
    /// And that URL must name no username. `git` asks its
    /// credential helper for whichever username the URL
    /// carries, and `git-credential-store` answers only when
    /// that equals the one it stored -- so a `user@` there ships
    /// the token into the guest and leaves the clone unable to
    /// use it.
    fn try_from(raw: SourceFields) -> Result<Self, Self::Error> {
        match (&raw.repo_token, &raw.repo_user) {
            (Some(_), None) => {
                return Err(FieldError::invalid(
                    RepoTokenVar::FIELD,
                    "needs `repo_user` beside it, naming the \
                     username the token is sent with -- \
                     `x-token-auth` for a Bitbucket repository \
                     access token",
                ));
            }
            (None, Some(_)) => {
                return Err(FieldError::invalid(
                    RepoUser::FIELD,
                    "needs `repo_token` beside it, naming the \
                     variable in `env_file` that holds the token",
                ));
            }
            _ => {}
        }
        if raw.repo_token.is_some() {
            if raw.env_file.is_none() {
                return Err(FieldError::invalid(
                    RepoTokenVar::FIELD,
                    "needs `env_file`, which is the file the \
                     named variable is read from",
                ));
            }
            // `git` asks its credential helper for the
            // username the URL names, and
            // `git-credential-store` answers only when that
            // equals the one it stored -- measured against the
            // real helper. So a `repo` carrying `me@` would
            // ship the token into the guest and leave the clone
            // unable to use it. One place names the username,
            // and it is `repo_user`.
            if raw.repo.https_userinfo() {
                return Err(FieldError::invalid(
                    RepoTokenVar::FIELD,
                    "needs a `repo` naming no username: take the \
                     `user@` out of the URL and let `repo_user` \
                     name it, because `git` asks its credential \
                     helper for whichever username the URL \
                     carries",
                ));
            }
            if raw.repo.https_host().is_none() {
                return Err(FieldError::invalid(
                    RepoTokenVar::FIELD,
                    "needs `repo` to be an `https` URL naming a \
                     host: git sends the token on every request, \
                     so `http` would put it on the wire in plain \
                     text and `ssh` never asks for one",
                ));
            }
        }
        Ok(Self {
            repo: raw.repo,
            git_ref: raw.git_ref,
            script: raw.script,
            deploy_key: raw.deploy_key,
            env_file: raw.env_file,
            repo_token: raw
                .repo_token
                .zip(raw.repo_user)
                .map(|(var, user)| RepoToken { var, user }),
        })
    }
}

/// A repository address that `git` will download from, and not
/// run as a command.
///
/// This is a *newtype*: a struct wrapping one `String`, where
/// the `String` inside is private. You cannot build one
/// directly. You have to call [`RepoUrl::parse`], which checks
/// the value first. So if you are holding a `RepoUrl`, it has
/// already been checked, and the compiler is what promises you
/// that.
///
/// Why not just check a plain `String` somewhere? Because the
/// other fields of `Config` are public, so any code can build a
/// `Config` by hand and never call the checking function. A
/// type is harder to go around than a function call.
///
/// The danger it guards against: `git` supports "remote
/// helpers", which are addresses written as `name::rest`. One
/// of them is `ext::`, and it tells `git` to *run* the rest as
/// a shell command. So `ext::sh -c "..."` looks like an address
/// and is really an instruction, and it would run inside the
/// guest VM as the agent's own user, before any of the
/// project's own code exists. [`RepoUrl::parse`] refuses it.
///
/// You might reach for the `url` crate here. Do not: `RepoUrl`
/// accepts `git@github.com:you/repo.git`, the usual way to
/// write an SSH address for `git`, and a URL parser rejects it
/// because it is not a valid URL.
///
/// The value stays whole. It goes to `git` as written and into
/// the Vagrantfile as written, and bombyx reads two pieces out
/// of it, one per transport. [`RepoUrl::ssh_host`] returns the
/// host whose published ssh keys the guest fetches, and
/// [`RepoUrl::https_host`] returns the authority a token is
/// sent to.
///
/// `#[serde(try_from = "String")]` is what connects the type to
/// the config file. It tells serde to read a plain string and
/// then hand it to [`RepoUrl::try_from`], which may refuse it.
/// Without the attribute serde would build the struct by
/// assigning its private field directly, skipping every check --
/// so the attribute is what makes "holding a `RepoUrl` means it
/// passed" true for a value that came out of TOML.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct RepoUrl(String);

impl RepoUrl {
    /// Checks `raw` and wraps it.
    ///
    /// Takes `&str` so a caller holding a borrowed value need
    /// not copy it. The one copy this does make is the value it
    /// keeps. Serde arrives with a `String` it already owns, and
    /// [`RepoUrl::try_from`] hands that straight over rather
    /// than copying it a second time. Both run the same checks --
    /// `check_repo`, which is private, so this is not a link.
    ///
    /// # Errors
    ///
    /// Returns [`FieldError::Empty`] when `raw` is blank, and
    /// [`FieldError::Invalid`] when it begins or ends with
    /// whitespace, would break the generated Vagrantfile, would
    /// be read by `git` as an option, or names a remote helper
    /// rather than a repository.
    pub fn parse(raw: &str) -> Result<Self, FieldError> {
        check_repo(raw)?;
        Ok(Self(raw.to_owned()))
    }

    /// The host `git` opens an ssh connection to, when it opens
    /// one at all.
    ///
    /// `None` for an `https`, `http` or `git` URL. Those reach
    /// the server by another transport, so no ssh host key is
    /// involved and there is nothing for `crate::hostkeys` to
    /// look up.
    ///
    /// Two spellings carry a host over ssh, and they end the
    /// authority differently. `ssh://git@host:2222/path` ends
    /// it at the first `/`, and the scp-like
    /// `git@host:path` ends it at the first `:`. Both may carry
    /// a `user@` in front, which is not part of the host.
    ///
    /// `None` for a bracketed IP literal such as
    /// `ssh://[::1]/p.git`. `check_repo` accepts one -- it tests
    /// for `::` only on the scp-like branch, so anything after
    /// `ssh://` gets through -- and there is nothing useful to
    /// return: `crate::hostkeys` looks a *named* host up in a
    /// table, an address can never be in it, and `ssh` spells a
    /// bracketed literal in `known_hosts` in a form bombyx does
    /// not produce. Returning a piece of the address would hand
    /// a later caller a value that looks like a host and is not.
    ///
    /// `None` when the URL names a port other than 22.
    /// `known_hosts` spells such a host `[github.com]:2222`,
    /// and the guest writes bare names, so a fetched key could
    /// never match -- measured with `ssh-keygen -F` against a
    /// bare-name file, which reports the bracketed form
    /// missing. Returning `None` leaves that URL on
    /// `accept-new` rather than turning it into a clone that
    /// cannot succeed.
    ///
    /// The host comes back exactly as the operator wrote it.
    /// `crate::hostkeys::for_host` is what compares it without
    /// regard to case.
    #[must_use]
    pub fn ssh_host(&self) -> Option<&str> {
        let authority = match self.0.strip_prefix("ssh://") {
            Some(rest) => rest.split_once('/').map_or(rest, |(a, _)| a),
            None if self.0.contains("://") => return None,
            None => self.0.split_once(':').map_or(self.0.as_str(), |(a, _)| a),
        };
        let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        // A bracketed literal is refused before the port is
        // trimmed, because splitting `[::1]:22` on the first
        // colon yields `[`.
        if host.starts_with('[') {
            return None;
        }
        let (host, port) =
            host.split_once(':').map_or((host, "22"), |(h, p)| (h, p));
        if port != "22" {
            return None;
        }
        (!host.is_empty()).then_some(host)
    }

    /// Whether an `https` URL carries a `user@` in front of
    /// the host.
    ///
    /// `false` for every other spelling, including one this
    /// type accepts and [`RepoUrl::https_host`] refuses, so a
    /// caller reading it gets an answer about `https` and
    /// nothing else.
    ///
    /// Separate from `https_host`, which drops the userinfo
    /// rather than reporting it. Dropping is right for building
    /// the credential line; a caller deciding whether the URL
    /// and `repo_user` disagree about the username needs to
    /// know it was there.
    #[must_use]
    pub fn https_userinfo(&self) -> bool {
        let Some(rest) = self.0.strip_prefix("https://") else {
            return false;
        };
        let authority = rest.split_once('/').map_or(rest, |(a, _)| a);
        authority.contains('@')
    }

    /// The authority `git` sends an `https` credential to, port
    /// included when the URL names one.
    ///
    /// `None` for every other spelling, and each exclusion is a
    /// refusal rather than an omission. An `http` URL would
    /// carry the token in plain text. An `ssh` or `git` URL
    /// never asks for one. A bracketed IP literal is left out
    /// because bombyx has not established how `git` spells one
    /// when it asks a credential helper, and a credential file
    /// naming a host `git` asks about differently is a file
    /// `git` reads and finds nothing in.
    ///
    /// The port stays attached, because `git` asks its helper
    /// about the host it is contacting and a line naming the
    /// bare host would not match.
    ///
    /// The value comes back exactly as the operator wrote it,
    /// including its case. `git` compares this against what it
    /// parsed out of the same string, so the two agree by
    /// construction.
    #[must_use]
    pub fn https_host(&self) -> Option<&str> {
        let rest = self.0.strip_prefix("https://")?;
        let authority = rest.split_once('/').map_or(rest, |(a, _)| a);
        let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        if host.starts_with('[') {
            return None;
        }
        (!host.is_empty()).then_some(host)
    }
}

checked_str_newtype!(
    RepoUrl,
    "The value, as `git` and the Vagrantfile see it."
);

checked_str_try_from!(
    /// What serde calls. It already owns the `String`, so the
    /// checks run against a borrow of it and the value moves
    /// into the newtype -- no copy on the path a config load
    /// actually takes. A refused value is dropped here.
    RepoUrl,
    FieldError,
    check_repo
);

/// A path inside the cloned project, on the guest.
///
/// A newtype for the same reason as [`RepoUrl`]: the checks
/// live in [`ScriptPath::parse`], so holding one of these means
/// it has already been checked.
///
/// You would expect `PathBuf` here, and it is the wrong choice.
/// `PathBuf` answers questions about the machine bombyx was
/// compiled for -- on Windows `\` separates directories and
/// `C:` names a drive, on Linux neither does. This path is
/// resolved on the guest, which is always Linux, so a `PathBuf`
/// would answer for the wrong computer, and answer differently
/// depending on who ran `bombyx up`. Since bombyx only checks
/// this value and passes it along, a checked string is the
/// honest representation.
///
/// (`std::os::unix` cannot stand in: those modules exist only
/// when compiling *for* Unix.)
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct ScriptPath(String);

checked_str_parse!(
    /// Checks `raw` and wraps it.
    ///
    /// # Errors
    ///
    /// Returns [`FieldError::Empty`] when `raw` is blank, and
    /// [`FieldError::Invalid`] when it begins or ends with
    /// whitespace, would break the generated Vagrantfile, would
    /// be read by `git` as an option, or leaves the clone
    /// directory.
    ScriptPath,
    FieldError,
    check_script
);

checked_str_newtype!(ScriptPath, "The value, as the guest's shell sees it.");

checked_str_try_from!(
    /// What serde calls; see [`RepoUrl::try_from`].
    ScriptPath,
    FieldError,
    check_script
);

/// A branch or tag name that `git` will fetch, and not read as
/// an option.
///
/// A newtype for the same reason as [`RepoUrl`]: the rules live
/// in [`GitRef::parse`], so holding one means they have run.
///
/// The value is written into the generated Vagrantfile and then
/// handed to `git` inside the guest, so it gets both the
/// Ruby-literal rules and the leading-dash rule.
///
/// **The dash rule here is the second of two guards.** The
/// guest runs `git fetch --depth 1 origin -- "$BOMBYX_REF"`,
/// and that `--` already tells `git` that whatever follows is a
/// value rather than an option. `super::guards::check_not_an_option`
/// says why the rule is kept anyway.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct GitRef(String);

checked_str_parse!(
    /// Checks `raw` and wraps it.
    ///
    /// # Errors
    ///
    /// Returns [`FieldError::Empty`] when `raw` is blank, and
    /// [`FieldError::Invalid`] when it begins or ends with
    /// whitespace, would break the generated Vagrantfile, or
    /// would be read by `git` as an option.
    GitRef,
    FieldError,
    check_git_ref
);

checked_str_newtype!(GitRef, "The value, as `git` and the Vagrantfile see it.");

checked_str_try_from!(
    /// What serde calls; see [`RepoUrl::try_from`].
    GitRef,
    FieldError,
    check_git_ref
);

/// Every rule a `ref` value must pass, in one place.
///
/// Both [`GitRef::parse`] and [`GitRef::try_from`] call this,
/// so neither can run a different set. Both rules are shared
/// with other fields: the value is written into a Ruby file,
/// and it reaches `git` on a command line.
fn check_git_ref(value: &str) -> Result<(), FieldError> {
    guards::check_renderable("ref", value)?;
    guards::check_not_an_option("ref", value, "git")?;
    Ok(())
}

/// Every rule a `repo` value must pass, in one place.
///
/// Both [`RepoUrl::parse`] and [`RepoUrl::try_from`] call this,
/// so neither can run a different set.
///
/// The first two rules are shared with other fields: the value
/// is written into a Ruby file, and it reaches `git` on a
/// command line. The third is `repo`'s own, and it stops the
/// `ext::` problem [`RepoUrl`] describes.
///
/// That third rule is an allowlist. Rather than naming every
/// dangerous spelling, it names the safe ones and refuses
/// everything else, so a spelling nobody thought of is refused
/// by default rather than allowed by default.
///
/// Two shapes are allowed. One is a normal URL starting with a
/// recognised scheme. The other is the SSH shorthand
/// `git@github.com:you/repo.git`, which has no `://` at all --
/// it is a host, a colon, then a path. That is what `scp_like`
/// below is looking for, and it refuses any `::`, so
/// `ext::something` cannot slip through as "a host called ext
/// with an empty path".
fn check_repo(value: &str) -> Result<(), FieldError> {
    const ALLOWED: [&str; 4] = ["https://", "http://", "ssh://", "git://"];

    guards::check_renderable("repo", value)?;
    guards::check_not_an_option("repo", value, "git")?;

    let scp_like =
        !value.contains("://") && value.contains(':') && !value.contains("::");
    if ALLOWED.iter().any(|p| value.starts_with(p)) || scp_like {
        return Ok(());
    }
    Err(FieldError::Invalid {
        field: "repo",
        reason: "must be an https, http, ssh or git URL, or \
                 `user@host:path`; a `<transport>::<rest>` \
                 remote helper such as `ext::` runs a command \
                 rather than cloning"
            .to_owned(),
    })
}

/// Every rule a `script` value must pass, in one place.
///
/// Both [`ScriptPath::parse`] and [`ScriptPath::try_from`] call
/// this, so neither can run a different set.
///
/// The first two rules are shared with other fields. The rest
/// are `script`'s own, and they matter because the guest
/// changes into the cloned project, runs `chmod +x` on this
/// path, and executes it -- so whatever this names is
/// about to be given the run of the machine.
///
/// A value escapes the clone in two ways, and both are refused:
///
/// - Starting with `/` makes it an absolute path, so it stops
///   being relative to the clone at all. `/usr/bin/env` would
///   make the guest `chmod +x` a system binary.
/// - A `..` segment steps up a directory. Enough of them and
///   you are outside the clone again, by a longer route.
///
/// Two more shapes are refused before this function is reached,
/// which is why it does not test for them. `check_renderable`
/// runs first and rejects any backslash anywhere -- so a
/// Windows-style `\windows\x` never arrives -- and it rejects
/// surrounding whitespace, so ` provision.sh` never arrives
/// either.
fn check_script(value: &str) -> Result<(), FieldError> {
    guards::check_renderable("script", value)?;
    guards::check_not_an_option("script", value, "git")?;

    let bad = if value.starts_with('/') {
        Some("must be relative to the clone root")
    } else if value.split('/').any(|s| s == "..") {
        Some("must not contain a `..` segment")
    } else {
        None
    };
    match bad {
        Some(reason) => Err(FieldError::Invalid {
            field: "script",
            reason: reason.to_owned(),
        }),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Asserts `bad` is refused with a message mentioning
    /// `reason`.
    ///
    /// The rules these three types share are tested against all
    /// four checked newtypes at once, in `super::guards`'s test
    /// module, where those rules live. What is left here is each
    /// type's own rule, so this helper takes a constructor
    /// rather than a table.
    fn refused_because(
        build: fn(&str) -> Result<(), FieldError>,
        bad: &str,
        reason: &str,
    ) {
        let err = build(bad).expect_err("must be refused").to_string();
        assert!(err.contains(reason), "{bad:?}: want {reason:?}, got {err}");
    }

    /// A `[source]` table carrying `extra`, parsed as TOML.
    ///
    /// The three required keys are written out so the cases
    /// below differ only in the keys they are about.
    fn source_with(extra: &str) -> Result<Source, toml::de::Error> {
        let text = format!(
            "repo = \"https://bitbucket.org/w/r.git\"\n\
             ref = \"main\"\n\
             script = \"vagrant/provision.sh\"\n\
             {extra}"
        );
        toml::from_str(&text)
    }

    #[test]
    fn a_token_and_a_username_are_stated_together_or_not_at_all() {
        // Each is useless alone. A variable name with no
        // username leaves bombyx guessing the literal, which is
        // what stating it exists to avoid, and a username with
        // no variable name has no token to go with.
        let only_token =
            source_with("env_file = \"~/s.env\"\nrepo_token = \"T\"\n")
                .expect_err("a token with no username must be refused")
                .to_string();
        assert!(only_token.contains("repo_user"), "{only_token}");

        let only_user = source_with("repo_user = \"x-token-auth\"\n")
            .expect_err("a username with no token must be refused")
            .to_string();
        assert!(only_user.contains("repo_token"), "{only_user}");
    }

    #[test]
    fn a_token_needs_the_file_it_is_read_out_of() {
        // `repo_token` names a variable *inside* `env_file`, so
        // without that key bombyx has nothing to look in.
        let err =
            source_with("repo_token = \"T\"\nrepo_user = \"x-token-auth\"\n")
                .expect_err("a token with no env_file must be refused")
                .to_string();
        assert!(err.contains("env_file"), "{err}");
    }

    #[test]
    fn a_token_needs_an_https_repository() {
        // `git` sends the token on every request, so `http`
        // would put it on the wire in plain text, and an `ssh`
        // URL never asks for one.
        for repo in [
            "http://bitbucket.org/w/r.git",
            "ssh://git@bitbucket.org/w/r.git",
            "git@bitbucket.org:w/r.git",
        ] {
            let text = format!(
                "repo = {repo:?}\n\
                 ref = \"main\"\n\
                 script = \"vagrant/provision.sh\"\n\
                 env_file = \"~/s.env\"\n\
                 repo_token = \"T\"\n\
                 repo_user = \"x-token-auth\"\n"
            );
            let err = toml::from_str::<Source>(&text)
                .expect_err("{repo} must be refused")
                .to_string();
            assert!(err.contains("https"), "{repo}: {err}");
        }
    }

    #[test]
    fn a_token_needs_a_repository_naming_no_username() {
        // `git` asks its credential helper for the username the
        // URL names, and `git-credential-store` answers only
        // when that matches the one it stored -- measured. So a
        // `repo` carrying `me@` makes the staged credential
        // unusable: the token reaches the guest and the clone
        // still cannot authenticate.
        let text = "repo = \"https://me@bitbucket.org/w/r.git\"\n\
                    ref = \"main\"\n\
                    script = \"vagrant/provision.sh\"\n\
                    env_file = \"~/s.env\"\n\
                    repo_token = \"T\"\n\
                    repo_user = \"x-token-auth\"\n";
        let err = toml::from_str::<Source>(text)
            .expect_err("a username in the URL must be refused")
            .to_string();
        assert!(err.contains("repo_user"), "{err}");
    }

    #[test]
    fn a_token_with_everything_it_needs_loads() {
        let source = source_with(
            "env_file = \"~/s.env\"\n\
             repo_token = \"BITBUCKET_TOKEN\"\n\
             repo_user = \"x-token-auth\"\n",
        )
        .expect("every rule is satisfied");
        assert_eq!(
            source.repo_token.map(|t| t.var.as_str().to_owned()),
            Some("BITBUCKET_TOKEN".to_owned())
        );
    }

    #[test]
    fn the_https_host_keeps_the_port_and_drops_the_userinfo() {
        // `git` asks its credential helper about the host it is
        // contacting, port included, so a line naming the bare
        // host would not match. Userinfo is not part of the
        // host at all.
        for (url, want) in [
            ("https://bitbucket.org/w/r.git", Some("bitbucket.org")),
            (
                "https://git.example.com:8443/r.git",
                Some("git.example.com:8443"),
            ),
            ("https://me@bitbucket.org/w/r.git", Some("bitbucket.org")),
            ("http://bitbucket.org/w/r.git", None),
            ("ssh://git@bitbucket.org/w/r.git", None),
            ("https://[::1]/r.git", None),
        ] {
            let repo = RepoUrl::parse(url).expect("a legal repo value");
            assert_eq!(repo.https_host(), want, "{url}");
        }
    }

    #[test]
    fn a_windows_style_script_path_is_caught_by_the_character_rule() {
        // `check_script` says a backslash never reaches it,
        // because `check_renderable` runs first and refuses one
        // anywhere in the value. This is the case holding that
        // claim up: `\windows\x` looks like a path mistake and
        // is refused as a character mistake.
        refused_because(
            |s| ScriptPath::parse(s).map(|_| ()),
            "\\windows\\x",
            "would end or escape",
        );
    }

    #[test]
    fn a_repo_url_refuses_anything_git_would_not_clone() {
        // `git` remote helpers are written `name::rest`, and
        // `ext::` runs the rest as a shell command rather than
        // cloning anything.
        for bad in ["ext::sh -c 'id > /pwned'", "fd::7", "not-a-url"] {
            refused_because(
                |s| RepoUrl::parse(s).map(|_| ()),
                bad,
                "must be an https",
            );
        }
    }

    #[test]
    fn a_repo_url_keeps_the_spellings_people_write() {
        for good in [
            "https://github.com/breki/bombyx",
            "http://example.invalid/p.git",
            "ssh://git@example.invalid/p.git",
            "git://example.invalid/p.git",
            "git@github.com:breki/bombyx.git",
        ] {
            let parsed = RepoUrl::parse(good)
                .unwrap_or_else(|e| panic!("{good:?}: {e}"));
            assert_eq!(parsed.as_str(), good);
        }
    }

    #[test]
    fn only_an_ssh_url_names_a_host_whose_key_matters() {
        // The whole family, so the answer is not read off the
        // one spelling that prompted the work. A host key
        // matters when `git` opens an ssh connection and not
        // otherwise: `https` and `http` are verified by TLS,
        // and `git://` carries no verification bombyx could
        // improve here.
        for (url, want) in [
            ("ssh://git@github.com/you/repo.git", Some("github.com")),
            ("ssh://github.com/you/repo.git", Some("github.com")),
            // An explicit default port is still the default
            // port, and `known_hosts` spells that host with a
            // bare name.
            ("ssh://git@github.com:22/you/r.git", Some("github.com")),
            // Any other port is refused. `known_hosts` spells
            // such a host `[github.com]:2222` -- measured with
            // `ssh-keygen -F` against a bare-name file, which
            // reports it missing -- and the guest writes bare
            // names, so verification could only fail. Returning
            // `None` keeps `accept-new` instead.
            ("ssh://git@github.com:2222/you/r.git", None),
            ("ssh://github.com:443/you/r.git", None),
            ("git@github.com:you/repo.git", Some("github.com")),
            ("github.com:you/repo.git", Some("github.com")),
            // Returned as written. `hostkeys::for_host` is what
            // folds case, because DNS does.
            ("ssh://git@GitHub.com/you/r.git", Some("GitHub.com")),
            ("https://github.com/you/repo.git", None),
            ("http://example.invalid/p.git", None),
            ("git://example.invalid/p.git", None),
            // No authority to read. `check_repo` accepts it,
            // because it only looks at the prefix.
            ("ssh:///p.git", None),
            // A bracketed IP literal reaches this. `check_repo`
            // tests for `::` only on the scp-like branch, so a
            // value starting `ssh://` is accepted whatever
            // follows.
            ("ssh://[::1]/p.git", None),
            ("ssh://[::1]:22/p.git", None),
            ("ssh://git@[2001:db8::1]/p.git", None),
            // A password in the authority is not part of the
            // host.
            ("ssh://user:secret@github.com/p.git", Some("github.com")),
        ] {
            let repo =
                RepoUrl::parse(url).unwrap_or_else(|e| panic!("{url:?}: {e}"));
            assert_eq!(repo.ssh_host(), want, "{url:?}");
        }
    }

    #[test]
    fn a_script_path_refuses_one_that_leaves_the_clone() {
        // Whatever this names is about to be made executable
        // and run in the guest, as the agent's own user.
        for (bad, reason) in [
            ("/usr/bin/env", "relative to the clone root"),
            ("../../usr/bin/env", "`..` segment"),
            ("a/../../../etc/x", "`..` segment"),
        ] {
            refused_because(|s| ScriptPath::parse(s).map(|_| ()), bad, reason);
        }

        let ok = ScriptPath::parse("vagrant/provision.sh")
            .expect("a plain relative path");
        assert_eq!(ok.as_str(), "vagrant/provision.sh");
    }

    #[test]
    fn the_newtypes_render_as_the_value_they_hold() {
        let repo = RepoUrl::parse("https://example.invalid/p.git").unwrap();
        assert_eq!(repo.to_string(), "https://example.invalid/p.git");
        assert_eq!(repo.as_ref(), "https://example.invalid/p.git");

        let script = ScriptPath::parse("vagrant/provision.sh").unwrap();
        assert_eq!(script.to_string(), "vagrant/provision.sh");
        assert_eq!(script.as_ref(), "vagrant/provision.sh");

        let git_ref = GitRef::parse("main").unwrap();
        assert_eq!(git_ref.as_str(), "main");
        assert_eq!(git_ref.to_string(), "main");
        assert_eq!(git_ref.as_ref(), "main");
    }

    #[test]
    fn serde_runs_every_check_while_the_table_is_read() {
        // `try_from` is a second entry point into each type, and
        // the attribute that selects it is easy to drop. Without
        // it serde assigns the private field and no check runs
        // at all, so this asserts against the deserializer
        // rather than against the three `parse` functions.
        let good = "repo = \"https://example.invalid/p.git\"\n\
                    ref = \"main\"\n\
                    script = \"vagrant/provision.sh\"\n";
        toml::from_str::<Source>(good).expect("the fixture must load");

        for (from, to, reason) in [
            (
                "https://example.invalid/p.git",
                "ext::sh -c id",
                "must be an https",
            ),
            (
                "ref = \"main\"",
                "ref = \"--upload-pack=x\"",
                "as an option",
            ),
            (
                "vagrant/provision.sh",
                "/usr/bin/env",
                "relative to the clone",
            ),
        ] {
            let err = toml::from_str::<Source>(&good.replace(from, to))
                .expect_err("must be refused");
            assert!(err.to_string().contains(reason), "{to}: {err}");
        }
    }
}
