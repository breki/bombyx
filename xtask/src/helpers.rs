use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Width of the step name column (including dots).
const STEP_NAME_WIDTH: usize = 14;

/// Format a validation step result line as a string.
///
/// Returns: `[1/5] Fmt........... OK (0.3s)`
pub fn format_step(
    step: usize,
    total: usize,
    name: &str,
    status: &str,
    detail: &str,
) -> String {
    let dots = ".".repeat(STEP_NAME_WIDTH.saturating_sub(name.len()));
    if detail.is_empty() {
        format!("[{step}/{total}] {name}{dots} {status}")
    } else {
        format!(
            "[{step}/{total}] {name}{dots} {status} \
             ({detail})"
        )
    }
}

/// Print a validation step result line to stdout.
///
/// Produces: `[1/5] Fmt........... OK (0.3s)`
pub fn step_output(
    step: usize,
    total: usize,
    name: &str,
    status: &str,
    detail: &str,
) {
    println!("{}", format_step(step, total, name, status, detail));
}

/// Format elapsed time as a human-readable string.
pub fn elapsed_str(start: Instant) -> String {
    let secs = start.elapsed().as_secs_f64();
    format!("{secs:.1}s")
}

/// Format a byte count as a human-readable string with
/// a binary unit suffix (`B`, `KiB`, `MiB`, `GiB`,
/// `TiB`). One decimal place above `B`.
pub fn fmt_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    // Cache sizes routinely sum into the tens-of-GB
    // range, well under f64's 2^52 mantissa headroom,
    // so precision loss is not a real concern here.
    #[allow(clippy::cast_precision_loss)]
    let mut v = bytes as f64;
    let mut idx = 0;
    while v >= 1024.0 && idx + 1 < UNITS.len() {
        v /= 1024.0;
        idx += 1;
    }
    if idx == 0 {
        format!("{bytes} {}", UNITS[0])
    } else {
        format!("{v:.1} {}", UNITS[idx])
    }
}

/// A non-fatal problem encountered while walking a
/// directory tree for `dir_size`. The struct carries
/// the failing path separately from the message so
/// callers can filter or re-present without re-parsing
/// strings. `Display` produces `<path>: <message>`
/// which is the form users see in tool output.
#[derive(Debug, Clone)]
pub struct DirSizeWarning {
    pub path: PathBuf,
    pub message: String,
}

impl fmt::Display for DirSizeWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.message)
    }
}

impl DirSizeWarning {
    fn new(path: &Path, message: impl Into<String>) -> Self {
        Self {
            path: path.to_path_buf(),
            message: message.into(),
        }
    }
}

/// Return `true` if `meta` describes a symlink (any
/// platform) or a Windows reparse point of any kind
/// (symlink or directory junction).
///
/// On non-Windows, `meta.file_type().is_symlink()` is
/// the authoritative answer. On Windows that flag is
/// only set for `IO_REPARSE_TAG_SYMLINK`, so directory
/// junctions (`IO_REPARSE_TAG_MOUNT_POINT`, created by
/// `mklink /J`) need the `FILE_ATTRIBUTE_REPARSE_POINT`
/// bit checked explicitly. Without this guard a
/// junction below `target/` could redirect a tree
/// walk or `remove_dir_all` outside the workspace.
pub fn is_reparse_or_symlink_meta(meta: &fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Recursive byte sum for a path. Symlinks and Windows
/// reparse points (directory junctions) are not
/// followed and contribute zero bytes -- defends
/// against symlink-loop stack blow-up, against
/// attributing target sizes to the source, and against
/// walking arbitrary external trees behind a
/// `mklink /J`.
///
/// Returns `(total_bytes, warnings)`. Every failure
/// (including a failed `symlink_metadata` or
/// `read_dir` on `path` itself) is folded into the
/// warnings vector with its specific failing path
/// attached; this is the only error channel, so
/// callers handle warnings uniformly regardless of
/// recursion depth. Bytes from successfully walked
/// entries are still summed.
pub fn dir_size(path: &Path) -> (u64, Vec<DirSizeWarning>) {
    let mut warnings: Vec<DirSizeWarning> = Vec::new();

    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) => {
            warnings.push(DirSizeWarning::new(
                path,
                format!("symlink_metadata: {e}"),
            ));
            return (0, warnings);
        }
    };

    if is_reparse_or_symlink_meta(&meta) {
        return (0, warnings);
    }
    if meta.is_file() {
        return (meta.len(), warnings);
    }

    let entries = match fs::read_dir(path) {
        Ok(e) => e,
        Err(e) => {
            warnings.push(DirSizeWarning::new(path, format!("read_dir: {e}")));
            return (0, warnings);
        }
    };

    let mut total = 0u64;
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                warnings.push(DirSizeWarning::new(
                    path,
                    format!("read_dir entry: {e}"),
                ));
                continue;
            }
        };
        let (n, mut child_warnings) = dir_size(&entry.path());
        total += n;
        warnings.append(&mut child_warnings);
    }
    (total, warnings)
}

/// Per-test scratch directory under the system temp.
/// PID + a process-wide atomic counter keep parallel
/// test runs from colliding without adding a
/// `tempfile` dependency. The counter is shared across
/// threads, so no per-thread id is needed. Cleanup is
/// the caller's responsibility (best-effort
/// `remove_dir_all` at end of test).
#[cfg(test)]
pub(crate) fn temp_scratch(label: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    let dir = std::env::temp_dir()
        .join(format!("rustbase-xtask-{label}-{pid}-{seq}"));
    fs::create_dir_all(&dir).unwrap_or_else(|e| {
        panic!("failed to create scratch dir {}: {e}", dir.display())
    });
    dir
}

/// Resolve the cargo binary path. Prefers the `CARGO`
/// env var (set by cargo when running xtask) over a
/// PATH lookup.
pub fn cargo_bin() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".into())
}

/// Resolve the workspace root (the parent of the xtask
/// crate directory).
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate always lives under workspace root")
        .to_path_buf()
}

/// Run a cargo command and capture its output.
///
/// Strips ANSI codes via `CARGO_TERM_COLOR=never`.
pub fn run_cargo_capture(args: &[&str]) -> Result<Output, String> {
    let bin = cargo_bin();
    Command::new(&bin)
        .args(args)
        .env("CARGO_TERM_COLOR", "never")
        .output()
        .map_err(|e| format!("failed to run {bin}: {e}"))
}

/// Run a cargo command, streaming output to the
/// terminal. Used for `--verbose` mode and fmt.
pub fn run_cargo_stream(args: &[&str]) -> Result<(), String> {
    let bin = cargo_bin();
    let status = Command::new(&bin)
        .args(args)
        .status()
        .map_err(|e| format!("failed to run {bin}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        match status.code() {
            Some(code) => Err(format!("{bin} exited with {code}")),
            None => Err(format!("{bin} terminated by signal")),
        }
    }
}

/// Keep every line for which `keep` returns true, plus the
/// `-->` source-location line that immediately follows a
/// kept line. Shared by the clippy and test output
/// extractors so a diagnostic is surfaced together with
/// where it fired; without the paired location a failure
/// names the lint/error but not the `file:line`, forcing a
/// raw re-run. Only the location line *immediately* after a
/// kept line is taken, so a `-->` following a filtered line
/// is never wrongly attached.
pub fn pair_with_locations<F>(stderr: &str, keep: F) -> Vec<&str>
where
    F: Fn(&str) -> bool,
{
    let mut out: Vec<&str> = Vec::new();
    let mut prev_kept = false;
    for line in stderr.lines() {
        if keep(line) {
            out.push(line);
            prev_kept = true;
        } else if prev_kept && line.trim_start().starts_with("-->") {
            out.push(line);
            prev_kept = false;
        } else {
            prev_kept = false;
        }
    }
    out
}

/// Today's date as an ISO `YYYY-MM-DD` string (UTC).
///
/// Used to stamp `last-run` in the backfeed ledger and to
/// mint `tf-<date>-<slug>` feedback IDs. A clock read before
/// the Unix epoch (should never happen) degrades to day zero.
pub fn today_iso() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs() / 86_400).ok())
        .unwrap_or(0);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Convert a count of days since the Unix epoch back into a
/// civil `(year, month, day)`. Howard Hinnant's `civil_from_days`
/// (the inverse of the `days_from_civil` used elsewhere), valid
/// for the whole proleptic Gregorian range. Pure, so it is
/// unit-tested against known dates.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (y + i64::from(m <= 2), m, d)
}

/// True when `line` opens or closes a fenced code block (its
/// first non-whitespace run is ```` ``` ```` or `~~~`). Callers
/// toggle fence state on each such line so markdown headers
/// *inside* a code fence are not mistaken for structure.
pub fn is_fence(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("```") || t.starts_with("~~~")
}

/// Repo-relative path of the template-feedback file. Single
/// source of truth shared by the backfeed, feedback, and sync
/// commands (the last via its never-sync set).
pub const FEEDBACK_REL: &str = "docs/developer/template-feedback.md";

/// Repo-relative path of the machine-owned backfeed ledger.
pub const BACKFEED_LEDGER_REL: &str = "docs/developer/backfeed-ledger.toml";

/// Canonical markdown column width for the project (matches the
/// 80-column rule in `rustfmt.toml`). Shared by the `changelog`
/// and `todo` bullet writers.
pub const MARKDOWN_WIDTH: usize = 80;

/// Split file content into owned lines. Drops the trailing
/// newline; pair with [`rejoin`] to restore it. Shared by the
/// `changelog` and `todo` line-editing commands.
#[must_use]
pub fn to_owned_lines(content: &str) -> Vec<String> {
    content.lines().map(str::to_owned).collect()
}

/// Rejoin lines with `\n`, restoring a trailing newline when the
/// original file had one (so a rewrite stays byte-clean).
#[must_use]
pub fn rejoin(lines: &[String], ends_with_newline: bool) -> String {
    let mut out = lines.join("\n");
    if ends_with_newline {
        out.push('\n');
    }
    out
}

/// Locate a `## <heading>` section. Returns
/// `(heading_index, section_end)` where `section_end` is the
/// index of the next `## ` heading (or the line count) -- so the
/// section body is `(heading_index + 1)..section_end`. `None`
/// when the heading is absent. Shared by the `changelog` and
/// `todo` commands.
#[must_use]
pub fn section_bounds(
    lines: &[String],
    heading: &str,
) -> Option<(usize, usize)> {
    let h = lines.iter().position(|l| l.trim() == heading)?;
    let end = ((h + 1)..lines.len())
        .find(|&i| lines[i].starts_with("## "))
        .unwrap_or(lines.len());
    Some((h, end))
}

/// Reject empty / whitespace-only user input for the mechanical
/// markdown writers, so a contentless `- ` (or `- **slug** -- `)
/// stub is never written into `CHANGELOG.md` / `docs/todo.md`.
/// `label` names the offending field in the error.
///
/// # Errors
///
/// Returns an error when `value` is empty or only whitespace.
pub fn require_nonempty(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{label} must not be empty"))
    } else {
        Ok(())
    }
}

/// Word-wrap `text` into markdown bullet lines that stay within
/// `width` bytes (equal to display columns for the ASCII-leaning
/// markdown this project writes; a multibyte char counts as its
/// UTF-8 byte length, so a line never *exceeds* `width` columns
/// but may wrap a little early). The first line is prefixed with
/// `first` (e.g. `"- "`), continuation lines with `cont` (e.g.
/// `"  "`), matching the project's 80-column markdown + 2-space
/// hanging indent for list items. A single word longer than the
/// available width overflows onto its own line rather than being
/// split mid-word. Shared by the `changelog` and `todo` writers.
#[must_use]
pub fn wrap_markdown(
    text: &str,
    first: &str,
    cont: &str,
    width: usize,
) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::from(first);
    let mut has_word = false;
    for word in text.split_whitespace() {
        let would_be = cur.len() + 1 + word.len();
        if has_word && would_be > width {
            lines.push(cur);
            cur = String::from(cont);
            cur.push_str(word);
        } else {
            if has_word {
                cur.push(' ');
            }
            cur.push_str(word);
        }
        has_word = true;
    }
    lines.push(cur);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_from_days_epoch_and_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(31), (1970, 2, 1));
        // 2026-07-16 is 20650 days after the epoch.
        assert_eq!(civil_from_days(20650), (2026, 7, 16));
    }

    #[test]
    fn today_iso_is_well_formed() {
        // `dddd-dd-dd` shape, without depending on the
        // ISO-date validator (which now lives in `backfeed`).
        let s = today_iso();
        let b = s.as_bytes();
        assert_eq!(b.len(), 10);
        assert_eq!(b[4], b'-');
        assert_eq!(b[7], b'-');
        assert!(
            b.iter()
                .enumerate()
                .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
        );
    }

    #[test]
    fn is_fence_detects_both_markers() {
        assert!(is_fence("```"));
        assert!(is_fence("```rust"));
        assert!(is_fence("  ~~~"));
        assert!(!is_fence("### heading"));
        assert!(!is_fence("plain"));
    }

    #[test]
    fn pair_with_locations_keeps_kept_lines_and_following_arrows() {
        let input = "\
keep me
    --> a/b.rs:1:1
drop me
    --> should/not/appear.rs:2:2
keep me too";
        let out = pair_with_locations(input, |l| l.starts_with("keep"));
        assert_eq!(out.len(), 3);
        assert!(out[0].starts_with("keep me"));
        assert!(out[1].contains("--> a/b.rs:1:1"));
        assert!(out[2].starts_with("keep me too"));
        // The `-->` after the dropped line must not appear.
        assert!(!out.iter().any(|l| l.contains("should/not/appear")));
    }

    #[test]
    fn format_step_with_detail() {
        assert_eq!(
            format_step(1, 5, "Fmt", "OK", "0.3s"),
            "[1/5] Fmt........... OK (0.3s)"
        );
    }

    #[test]
    fn format_step_without_detail() {
        assert_eq!(
            format_step(2, 5, "Clippy", "FAILED", ""),
            "[2/5] Clippy........ FAILED"
        );
    }

    #[test]
    fn format_step_long_name_no_overflow() {
        let result = format_step(1, 1, "VeryLongStepName", "OK", "");
        assert_eq!(result, "[1/1] VeryLongStepName OK");
    }

    #[test]
    fn workspace_root_contains_cargo_toml() {
        let root = workspace_root();
        assert!(
            root.join("Cargo.toml").is_file(),
            "workspace root should contain Cargo.toml: {}",
            root.display()
        );
    }

    #[test]
    fn fmt_bytes_under_kib() {
        assert_eq!(fmt_bytes(0), "0 B");
        assert_eq!(fmt_bytes(512), "512 B");
        assert_eq!(fmt_bytes(1023), "1023 B");
    }

    #[test]
    fn fmt_bytes_scaling() {
        assert_eq!(fmt_bytes(1024), "1.0 KiB");
        assert_eq!(fmt_bytes(1536), "1.5 KiB");
        assert_eq!(fmt_bytes(1024 * 1024), "1.0 MiB");
        assert_eq!(fmt_bytes(3 * 1024 * 1024 * 1024), "3.0 GiB");
    }

    #[test]
    fn elapsed_str_format() {
        let start = Instant::now();
        let result = elapsed_str(start);
        assert!(result.ends_with('s'), "should end with 's': {result}");
        assert!(result.contains('.'), "should have decimal: {result}");
    }

    #[test]
    fn wrap_markdown_short_text_is_one_line() {
        assert_eq!(
            wrap_markdown("hello world", "- ", "  ", 80),
            vec!["- hello world"],
        );
    }

    #[test]
    fn wrap_markdown_wraps_at_width_with_hanging_indent() {
        // Width 20 forces a wrap; continuation uses the 2-space
        // prefix, and no line exceeds the width.
        let lines = wrap_markdown("alpha beta gamma delta", "- ", "  ", 20);
        assert_eq!(lines, vec!["- alpha beta gamma", "  delta"],);
        assert!(lines.iter().all(|l| l.len() <= 20));
    }

    #[test]
    fn wrap_markdown_overflows_a_single_long_word() {
        let lines =
            wrap_markdown("short superlongwordthatoverflows", "- ", "  ", 10);
        assert_eq!(lines, vec!["- short", "  superlongwordthatoverflows"]);
    }

    #[test]
    fn section_bounds_finds_body_range() {
        let lines = to_owned_lines("## A\n\n- one\n\n## B\n\n- two\n");
        assert_eq!(section_bounds(&lines, "## A"), Some((0, 4)));
        assert_eq!(section_bounds(&lines, "## B"), Some((4, lines.len())));
        assert_eq!(section_bounds(&lines, "## Missing"), None);
    }

    #[test]
    fn rejoin_round_trips_trailing_newline() {
        let with = "a\nb\n";
        assert_eq!(rejoin(&to_owned_lines(with), true), with);
        let without = "a\nb";
        assert_eq!(rejoin(&to_owned_lines(without), false), without);
    }

    #[test]
    fn require_nonempty_accepts_content_rejects_blank() {
        assert!(require_nonempty("x", "hello").is_ok());
        let err = require_nonempty("--summary", "   ").unwrap_err();
        assert!(err.contains("--summary"));
        assert!(require_nonempty("x", "").is_err());
    }
}
