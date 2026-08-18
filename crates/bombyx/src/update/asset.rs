//! Locating and verifying a published release archive.
//!
//! The release workflow attaches one `.tar.gz` per target plus a
//! single `SHA256SUMS`, and every name is derivable from the tag
//! and the platform -- so finding the right asset needs no call
//! to the releases API and no JSON parsing.
//!
//! Nothing here performs I/O. These functions build the `curl`
//! and `tar` argv and check the bytes afterwards, which keeps the
//! parts that can be wrong (which asset, which URL, which hash)
//! testable without a network.
//!
//! **Verification fails closed.** A missing or unparsable
//! `SHA256SUMS`, an entry absent for this asset, or a mismatched
//! digest all refuse the update. There is deliberately no
//! "download anyway" path: the one thing worse than not updating
//! is replacing the binary with something unverified.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::{REPO_URL, Version};
use crate::remote::RemoteCommand;

/// Name of the checksum file attached to every release.
pub const SUMS_FILE: &str = "SHA256SUMS";

/// The release target triple this build corresponds to.
///
/// `None` on a platform the release workflow does not build for,
/// which is a clearer answer than guessing at an asset name that
/// will 404. Only the four published targets are listed.
///
/// A `windows-gnu` build maps to the `windows-msvc` asset on
/// purpose: the published binary runs on the same operating
/// system, and the distinction is about how bombyx itself was
/// compiled rather than what it can execute.
#[must_use]
pub fn target_triple() -> Option<&'static str> {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("x86_64", "windows") => Some("x86_64-pc-windows-msvc"),
        ("x86_64", "linux") => Some("x86_64-unknown-linux-gnu"),
        ("x86_64", "macos") => Some("x86_64-apple-darwin"),
        ("aarch64", "macos") => Some("aarch64-apple-darwin"),
        _ => None,
    }
}

/// The directory name inside a release archive, which is also the
/// asset's base name: `bombyx-v0.2.0-x86_64-pc-windows-msvc`.
#[must_use]
pub fn stem(version: Version, triple: &str) -> String {
    format!("bombyx-{}-{triple}", version.tag())
}

/// The archive `self-update` downloads.
///
/// Always the `.tar.gz`, including on Windows, where a `.zip` is
/// published as well. Extraction runs `tar`, and the `tar` that
/// PATH resolves to on Windows is usually GNU tar from Git for
/// Windows, which cannot read a zip -- so the one archive both
/// platforms can open is the tarball.
#[must_use]
pub fn archive_name(version: Version, triple: &str) -> String {
    format!("{}.tar.gz", stem(version, triple))
}

/// URL of a file attached to `version`'s release.
#[must_use]
pub fn asset_url(version: Version, file: &str) -> String {
    format!("{REPO_URL}/releases/download/{}/{file}", version.tag())
}

/// Largest release archive this will accept, in bytes.
///
/// The published archives are under a megabyte; 64 MiB is far
/// above any plausible growth and far below "fills the disk".
pub const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;

/// Largest `SHA256SUMS` this will accept, in bytes.
///
/// A handful of 100-character lines. 64 KiB is generous.
pub const MAX_SUMS_BYTES: u64 = 64 * 1024;

// The two caps must not be swapped: a checksum file the size of an
// archive is not a checksum file. Checked at compile time, since
// both values are constants and a test would only assert what the
// compiler already knows.
const _: () = assert!(MAX_SUMS_BYTES < MAX_ARCHIVE_BYTES);

/// Builds the `curl` command that fetches `url` into `dest`.
///
/// Flags that carry weight rather than being habit:
///
/// - **`-f`** makes an HTTP error an exit failure. Without it
///   curl writes the error body to the output file and exits
///   zero, so a missing asset becomes a nine-byte file containing
///   `Not Found` -- measured, and the reason this flag is here.
/// - **`--proto =https`** refuses any non-HTTPS transfer,
///   including one arrived at by redirect.
/// - **`-L`** is required because a release asset redirects to
///   object storage, and the flag above is what keeps that
///   redirect from leaving TLS.
/// - **`--max-filesize`, `--max-time`, `--speed-limit`** bound the
///   transfer. The checksum cannot stand in for them: it is
///   computed from the complete bytes, so an oversized or stalled
///   body has already cost the disk or the wait by the time
///   verification could run.
#[must_use]
pub fn download_command(
    url: &str,
    dest: &Path,
    max_bytes: u64,
) -> RemoteCommand {
    let max = max_bytes.to_string();
    RemoteCommand::new(
        "curl",
        &[
            "-fsSL",
            "--proto",
            "=https",
            "--tlsv1.2",
            // Bounds, because the checksum cannot help here: it is
            // computed from the *complete* bytes, so anything that
            // fills the disk or stalls forever has already done its
            // damage before verification can run. Whoever answers
            // for the host on this network -- a TLS-terminating
            // proxy, a hijacked DNS with a trusted root -- can
            // serve a body of any size.
            "--max-filesize",
            &max,
            "--connect-timeout",
            "20",
            "--max-time",
            "600",
            // Abort a transfer dribbling below 1 KiB/s for 30s.
            // `-s` hides all progress, so without this a stalled
            // download is indistinguishable from a hang.
            "--speed-limit",
            "1024",
            "--speed-time",
            "30",
            "-o",
            &dest.to_string_lossy(),
            url,
        ],
    )
}

/// Builds the `tar` command that extracts the binary inside
/// `work`, leaving it at `work/<binary>`.
///
/// The archive holds `<stem>/bombyx`, so a single member is named
/// and `--strip-components=1` drops the directory. Extracting the
/// whole archive would also unpack `LICENSE` and `README.md`.
///
/// **`tar` is given only bare names, and runs in `work`.** Not
/// tidiness -- the same precaution `PushArchive` documents for
/// `scp`, for the same reason and against a worse failure. GNU
/// tar applies the `host:file` rule to `-f`, so an absolute
/// Windows path makes it try to reach a machine called `C`:
///
/// ```text
/// tar (child): Cannot connect to C: resolve failed
/// ```
///
/// `--force-local` suppresses that, and is still not enough: the
/// same tar cannot parse a backslash path for `-C` either,
/// mangling it into `C\:\\Users\\...: Cannot open`. Both were
/// measured with GNU tar 1.35, which is the `tar` that Windows
/// `PATH` resolves to when Git for Windows is installed -- the
/// usual case on the platform bombyx is developed on.
///
/// So no absolute path is passed at all. The binary is left in
/// `work` and moved into place by [`super::place`], which is a
/// Rust rename rather than an argument to another program.
#[must_use]
pub fn extract_command(
    archive_file: &str,
    work: &Path,
    version: Version,
    triple: &str,
) -> RemoteCommand {
    let member = format!("{}/{}", stem(version, triple), super::BINARY);
    RemoteCommand::new(
        "tar",
        &["-xzf", archive_file, "--strip-components=1", &member],
    )
    .in_dir(work)
}

/// Everything needed to fetch, verify and unpack one release.
///
/// Assembled in the library rather than in the binary so the
/// composition is testable: which URL each command gets, that both
/// downloads land inside the work directory, and that the archive
/// is unpacked where [`super::place`] will look for it. Built by
/// hand in `main.rs`, none of that was reachable from a test.
#[derive(Debug)]
pub struct UpdatePlan {
    /// File name of the release archive.
    pub archive: String,
    /// Where the archive is downloaded to.
    pub archive_path: PathBuf,
    /// Where the checksum file is downloaded to.
    pub sums_path: PathBuf,
    /// Fetches the checksum file.
    pub get_sums: RemoteCommand,
    /// Fetches the archive.
    pub get_archive: RemoteCommand,
    /// Unpacks the binary inside the work directory.
    pub extract: RemoteCommand,
    /// Where extraction leaves the binary.
    pub extracted: PathBuf,
}

impl UpdatePlan {
    /// The three commands, in the order they must run.
    ///
    /// The order is the point, and it is why this exists rather
    /// than callers reading the fields: the checksum file is
    /// fetched **first**, so a release that cannot be verified is
    /// discovered before an archive is downloaded rather than
    /// after.
    #[must_use]
    pub fn steps(&self) -> [&RemoteCommand; 3] {
        [&self.get_sums, &self.get_archive, &self.extract]
    }
}

/// Plans an update to `version` for `triple`, staged in `work`.
#[must_use]
pub fn plan(version: Version, triple: &str, work: &Path) -> UpdatePlan {
    let archive = archive_name(version, triple);
    let archive_path = work.join(&archive);
    let sums_path = work.join(SUMS_FILE);
    UpdatePlan {
        get_sums: download_command(
            &asset_url(version, SUMS_FILE),
            &sums_path,
            MAX_SUMS_BYTES,
        ),
        get_archive: download_command(
            &asset_url(version, &archive),
            &archive_path,
            MAX_ARCHIVE_BYTES,
        ),
        extract: extract_command(&archive, work, version, triple),
        extracted: work.join(super::BINARY),
        archive,
        archive_path,
        sums_path,
    }
}

/// Lowercase hex alphabet, for rendering a digest.
const HEX: &[u8; 16] = b"0123456789abcdef";

/// The SHA-256 of `bytes`, as lowercase hex.
///
/// Hand-rolled nibble to hex rather than a `hex` dependency or a
/// `format!` per byte: this runs once per update over a whole
/// archive, and the alternative was one allocation for every one
/// of the 32 output bytes.
#[must_use]
pub fn digest(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}

/// Finds `file`'s expected digest in a `SHA256SUMS` body.
///
/// The format is `sha256sum`'s own: a 64-character hex digest,
/// separator, then the file name. A `*` before the name marks
/// binary mode and is not part of it.
///
/// Strict about the digest -- exactly 64 hex characters -- so a
/// truncated or HTML error page cannot yield something that later
/// compares equal to anything.
#[must_use]
pub fn expected_digest(sums: &str, file: &str) -> Option<String> {
    for line in sums.lines() {
        // `continue`, not `?`. Written with `?` this returned
        // `None` for the *whole file* on the first line without
        // whitespace -- a blank line, a comment, a stray token --
        // even when the correct entry sat below it. The caller
        // then reported "no entry for this asset", and the command
        // above that reported "this release predates checksummed
        // releases": two false statements about the release, from
        // one unparsable line.
        let Some((hash, name)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let name = name.trim_start().trim_start_matches('*').trim();
        if name != file {
            continue;
        }
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        return Some(hash.to_ascii_lowercase());
    }
    None
}

/// Whether `bytes` matches the digest `sums` records for `file`.
///
/// `Err` carries what went wrong, because the two failures need
/// different responses: a missing entry means the release was
/// published without this asset, and a mismatch means the bytes
/// are not the ones that were released.
///
/// # Errors
///
/// [`VerifyError::NoEntry`] when `sums` names no such file, and
/// [`VerifyError::Mismatch`] when the digest differs.
pub fn verify(sums: &str, file: &str, bytes: &[u8]) -> Result<(), VerifyError> {
    let expected =
        expected_digest(sums, file).ok_or_else(|| VerifyError::NoEntry {
            file: file.to_owned(),
        })?;
    let actual = digest(bytes);
    if actual == expected {
        return Ok(());
    }
    Err(VerifyError::Mismatch {
        file: file.to_owned(),
        expected,
        actual,
    })
}

/// Why a downloaded asset was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VerifyError {
    /// The checksum file does not mention this asset.
    #[error(
        "{SUMS_FILE} has no entry for {file}, so the download \
         cannot be verified"
    )]
    NoEntry {
        /// Asset that is missing an entry.
        file: String,
    },

    /// The bytes do not match the published digest.
    #[error(
        "{file} does not match the published checksum\n  \
         expected {expected}\n  got      {actual}"
    )]
    Mismatch {
        /// Asset whose digest differs.
        file: String,
        /// What the release says it should be.
        expected: String,
        /// What was actually downloaded.
        actual: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v() -> Version {
        Version::parse("0.2.0").unwrap()
    }

    const TRIPLE: &str = "x86_64-pc-windows-msvc";

    #[test]
    fn names_the_asset_after_the_tag_and_triple() {
        // Pinned against the real published asset name, so a
        // change to the workflow's naming breaks this rather than
        // producing a 404 at update time.
        assert_eq!(
            archive_name(v(), TRIPLE),
            "bombyx-v0.2.0-x86_64-pc-windows-msvc.tar.gz"
        );
        assert_eq!(stem(v(), TRIPLE), "bombyx-v0.2.0-x86_64-pc-windows-msvc");
    }

    #[test]
    fn builds_the_release_download_url() {
        assert_eq!(
            asset_url(v(), "SHA256SUMS"),
            format!("{REPO_URL}/releases/download/v0.2.0/SHA256SUMS")
        );
    }

    #[test]
    fn this_platform_has_a_published_target() {
        // Every platform the test suite runs on is one the release
        // workflow builds for, so `None` here means the two lists
        // have drifted apart.
        assert!(
            target_triple().is_some(),
            "no published asset for {}/{}",
            std::env::consts::ARCH,
            std::env::consts::OS
        );
    }

    #[test]
    fn download_refuses_http_errors_and_plain_http() {
        // `-f` is what keeps an error page from being saved as the
        // asset: without it curl exits zero having written
        // `Not Found` to the output file.
        let c = download_command(
            "https://x/y",
            Path::new("/tmp/a.tgz"),
            MAX_ARCHIVE_BYTES,
        );
        assert_eq!(c.program, "curl");
        assert!(c.args.iter().any(|a| a == "-fsSL"), "{:?}", c.args);
        assert!(c.args.iter().any(|a| a == "=https"), "{:?}", c.args);
    }

    #[test]
    fn extracts_only_the_binary() {
        let work = Path::new(r"C:\Users\igor\AppData\Local\Temp\x");
        let c = extract_command("a.tar.gz", work, v(), TRIPLE);
        assert_eq!(c.program, "tar");
        assert_eq!(c.dir.as_deref(), Some(work));
        assert!(c.args.iter().any(|a| a == "--strip-components=1"));
        // The named member, so LICENSE and README are not
        // unpacked alongside it.
        assert!(
            c.args.iter().any(|a| a
                == &format!("bombyx-v0.2.0-{TRIPLE}/{}", super::super::BINARY)),
            "{:?}",
            c.args
        );
    }

    #[test]
    fn no_tar_argument_carries_a_drive_letter() {
        // The regression guard. An absolute Windows path in `-f`
        // makes GNU tar read `C:` as a host name and fail with
        // "Cannot connect to C: resolve failed"; a backslash path
        // in `-C` is mangled instead. Both were measured. The
        // command therefore runs *in* the work directory and every
        // argument is a bare name -- the same shape `push_dir`
        // uses for `scp`, and asserted the same way.
        let work = Path::new(r"C:\Users\igor\AppData\Local\Temp\x");
        let c = extract_command("a.tar.gz", work, v(), TRIPLE);
        for arg in &c.args {
            assert!(!arg.contains(':'), "tar argument {arg:?} has a colon");
            assert!(
                !arg.contains('\\'),
                "tar argument {arg:?} has a backslash"
            );
        }
        assert!(
            !c.args.iter().any(|a| a == "-C"),
            "no -C: an absolute dest is what breaks on Windows"
        );
    }

    /// The digest of `abc`, from the SHA-256 specification.
    const ABC: &str =
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn the_plan_fetches_the_checksum_file_first() {
        // The ordering invariant verification rests on: discover a
        // release that cannot be verified *before* downloading its
        // archive. Assembled by hand in the binary, this was
        // reachable by no test at all.
        let work = Path::new("/work");
        let p = plan(v(), TRIPLE, work);
        let order: Vec<&str> =
            p.steps().iter().map(|c| c.program.as_str()).collect();
        assert_eq!(order, vec!["curl", "curl", "tar"]);
        assert!(
            p.get_sums.args.iter().any(|a| a.ends_with(SUMS_FILE)),
            "the first step must fetch {SUMS_FILE}: {:?}",
            p.get_sums.args
        );
    }

    #[test]
    fn the_plan_stages_everything_inside_the_work_dir() {
        // A path escaping the temp directory would leave downloads
        // behind, or worse write them next to the installed binary.
        let work = Path::new("/work");
        let p = plan(v(), TRIPLE, work);
        for path in [&p.archive_path, &p.sums_path, &p.extracted] {
            assert!(
                path.starts_with(work),
                "{} escapes {work:?}",
                path.display()
            );
        }
        // Extraction must leave the binary exactly where `place`
        // will look for it.
        assert_eq!(p.extracted, work.join(super::super::BINARY));
        assert_eq!(p.archive_path, work.join(&p.archive));
    }

    #[test]
    fn the_plan_points_each_download_at_its_own_url() {
        let p = plan(v(), TRIPLE, Path::new("/work"));
        let url_of = |c: &RemoteCommand| c.args.last().unwrap().clone();
        assert_eq!(url_of(&p.get_sums), asset_url(v(), SUMS_FILE));
        assert_eq!(url_of(&p.get_archive), asset_url(v(), &p.archive));
        // Swapping the two would download the archive over the
        // checksum file and verify it against itself.
        assert_ne!(url_of(&p.get_sums), url_of(&p.get_archive));
    }

    #[test]
    fn the_plan_bounds_the_two_downloads_differently() {
        // The archive cap must not be applied to the checksum file:
        // a 64 MiB SHA256SUMS is not a checksum file.
        let p = plan(v(), TRIPLE, Path::new("/work"));
        let cap = |c: &RemoteCommand| {
            let i = c.args.iter().position(|a| a == "--max-filesize").unwrap();
            c.args[i + 1].clone()
        };
        assert_eq!(cap(&p.get_sums), MAX_SUMS_BYTES.to_string());
        assert_eq!(cap(&p.get_archive), MAX_ARCHIVE_BYTES.to_string());
    }

    #[test]
    fn hashes_against_a_published_test_vector() {
        // A known-answer test rather than a round-trip: hashing
        // and comparing our own output would pass with any
        // consistent-but-wrong implementation.
        assert_eq!(digest(b"abc"), ABC);
        assert_eq!(
            digest(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    fn sums() -> String {
        format!("{ABC}  bombyx-v0.2.0-{TRIPLE}.tar.gz\n{ABC}  other.tar.gz\n")
    }

    #[test]
    fn finds_the_digest_for_a_named_file() {
        let file = format!("bombyx-v0.2.0-{TRIPLE}.tar.gz");
        assert_eq!(expected_digest(&sums(), &file), Some(ABC.to_owned()));
    }

    #[test]
    fn accepts_the_binary_mode_marker() {
        // `sha256sum -b` writes `hash *name`, and the star is not
        // part of the file name.
        let body = format!("{ABC} *asset.tar.gz\n");
        assert_eq!(
            expected_digest(&body, "asset.tar.gz"),
            Some(ABC.to_owned())
        );
    }

    #[test]
    fn uppercase_digests_are_normalised() {
        let body = format!("{}  a.tgz\n", ABC.to_uppercase());
        assert_eq!(expected_digest(&body, "a.tgz"), Some(ABC.to_owned()));
    }

    #[test]
    fn a_missing_or_malformed_entry_yields_nothing() {
        // The family, written out before the guard: an absent
        // name, a short digest, a long one, and non-hex. Anything
        // this lets through becomes something the update compares
        // against.
        let file = "a.tgz";
        for body in [
            String::new(),
            format!("{ABC}  other.tgz\n"),
            format!("{}  a.tgz\n", &ABC[..63]),
            format!("{ABC}0  a.tgz\n"),
            format!("{}  a.tgz\n", "z".repeat(64)),
        ] {
            assert_eq!(
                expected_digest(&body, file),
                None,
                "{body:?} must not yield a digest"
            );
        }
    }

    #[test]
    fn a_junk_line_does_not_hide_a_later_entry() {
        // The regression both reviewers caught: written with `?`,
        // any whitespace-free line aborted the entire lookup, so
        // a blank line above the real entry made a verifiable
        // release report itself unverifiable.
        let file = "a.tgz";
        for prefix in [
            "\n",                  // a blank line
            "# generated by CI\n", // a comment (has whitespace)
            "junk\n",              // one bare token
            "\n\n\n",              // several blank lines
            "SHA256SUMS\n",        // a lone file name
        ] {
            let body = format!("{prefix}{ABC}  a.tgz\n");
            assert_eq!(
                expected_digest(&body, file),
                Some(ABC.to_owned()),
                "{prefix:?} must be skipped, not fatal"
            );
        }
    }

    #[test]
    fn survives_a_crlf_checksum_file() {
        // `lines()` strips the `\r`, but the assertion is cheap
        // and a mirror that rewrites line endings should not make
        // a release unverifiable.
        let body = format!("{ABC}  a.tgz\r\n");
        assert_eq!(expected_digest(&body, "a.tgz"), Some(ABC.to_owned()));
    }

    #[test]
    fn verifies_matching_bytes() {
        let file = format!("bombyx-v0.2.0-{TRIPLE}.tar.gz");
        assert_eq!(verify(&sums(), &file, b"abc"), Ok(()));
    }

    #[test]
    fn refuses_bytes_that_do_not_match() {
        let file = format!("bombyx-v0.2.0-{TRIPLE}.tar.gz");
        let err = verify(&sums(), &file, b"tampered").unwrap_err();
        assert!(matches!(err, VerifyError::Mismatch { .. }), "{err}");
        // Both digests in the message, so the operator can tell a
        // stale asset from a substituted one.
        let text = err.to_string();
        assert!(text.contains(ABC), "{text}");
    }

    #[test]
    fn refuses_an_asset_with_no_checksum_entry() {
        let err = verify(&sums(), "unlisted.tar.gz", b"abc").unwrap_err();
        assert!(matches!(err, VerifyError::NoEntry { .. }), "{err}");
        assert!(err.to_string().contains(SUMS_FILE), "{err}");
    }

    #[test]
    fn an_html_error_page_is_not_a_checksum_file() {
        // The realistic failure: a 404 body saved as SHA256SUMS.
        // It must refuse rather than parse into something.
        let body = "<!DOCTYPE html><html><body>Not Found</body></html>";
        assert_eq!(expected_digest(body, "a.tgz"), None);
    }
}
