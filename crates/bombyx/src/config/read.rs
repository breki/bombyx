//! Reading a configuration file off disk, and reporting what
//! went wrong with it.
//!
//! Everything here is about the *file*: whether the path may be
//! a symlink, how large a file may be, and how a TOML error is
//! summarised. What the values inside
//! the file must look like is `super::guards` and the field
//! modules beside it.

use std::path::Path;

use super::ConfigError;

/// Largest configuration file that will be read.
///
/// Generous for a handful of keys, and small enough that a file
/// committed to a repo cannot make bombyx read it into memory
/// without bound.
pub(super) const MAX_CONFIG_BYTES: u64 = 64 * 1024;

/// A path as it appears in a message.
pub(super) fn path_display(path: &Path) -> String {
    path.display().to_string()
}

/// Deserializes TOML, naming `path` in any error.
pub(super) fn from_toml<T>(source: &str, path: &Path) -> Result<T, ConfigError>
where
    T: serde::de::DeserializeOwned,
{
    toml::from_str(source).map_err(|e| ConfigError::Parse {
        path: path.to_path_buf(),
        summary: toml_summary(source, &e),
    })
}

/// Position and reason from a TOML error, without the source line.
///
/// Why the `toml` crate's own `Display` is not used is argued at
/// [`ConfigError::Parse`], with the reproduction. In short: it
/// quotes the offending source line into the message.
///
/// `message()` gives the reason alone -- "expected an equals,
/// found a newline" -- with no snippet and no position. `span()`
/// gives a byte range, and turning that into a line and column
/// needs the source, which is why `source` is a parameter here
/// and never reaches the result.
fn toml_summary(source: &str, e: &toml::de::Error) -> String {
    let reason = e.message().trim();
    match e.span() {
        Some(span) => {
            let (line, column) = line_column(source, span.start);
            format!("line {line}, column {column}: {reason}")
        }
        // No span on a shape mismatch -- a missing field, an
        // unknown key -- and the reason identifies the field there,
        // which is the whole answer.
        None => reason.to_owned(),
    }
}

/// One-based line and column for a byte offset into `source`.
///
/// The column counts *characters*, not bytes, so a non-ASCII line
/// does not report a position past where the operator sees the
/// problem. An offset past the end clamps to the last line, which is
/// what a truncated file produces.
fn line_column(source: &str, offset: usize) -> (usize, usize) {
    let upto = &source[..offset.min(source.len())];
    let line = upto.bytes().filter(|b| *b == b'\n').count() + 1;
    let column =
        upto.rsplit('\n').next().unwrap_or_default().chars().count() + 1;
    (line, column)
}

/// Reads a config file that is allowed not to exist.
///
/// Absence is `None`. Anything else is an error rather than a
/// fallback: a config that exists but cannot be read is a
/// problem to report, not a reason to quietly send commands to
/// a host the operator did not name.
///
/// Anything that is not a regular file is rejected -- pointed at
/// `/dev/zero` or a FIFO, reading would hang or allocate without
/// bound -- and the size cap bounds an ordinary large file.
///
/// **A symlink is followed**, and still has to end at a regular
/// file. The one file bombyx reads is the operator's own, and
/// dotfile managers (`stow`, `chezmoi`, a hand-made `ln -s`)
/// symlink exactly this kind of file into place; refusing one
/// would fail every subcommand with a message about regular
/// files that never mentions symlinks.
pub(super) fn read_optional(
    path: &Path,
) -> Result<Option<String>, ConfigError> {
    use std::io::Read as _;

    let meta = match std::fs::metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(source) => {
            return Err(ConfigError::Read {
                path: path.to_path_buf(),
                source,
            });
        }
    };

    if !meta.is_file() {
        return Err(ConfigError::NotAFile(path.to_path_buf()));
    }

    let read = |source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    };

    let file = std::fs::File::open(path).map_err(read)?;
    let mut source = String::new();
    // One byte past the cap, so a file *at* the limit is
    // accepted and anything beyond it is detectable rather than
    // silently truncated into a confusing parse error.
    file.take(MAX_CONFIG_BYTES + 1)
        .read_to_string(&mut source)
        .map_err(read)?;

    if source.len() as u64 > MAX_CONFIG_BYTES {
        return Err(ConfigError::TooLarge(path.to_path_buf()));
    }

    Ok(Some(source))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_position_counts_lines_and_characters() {
        assert_eq!(line_column("abc", 0), (1, 1));
        assert_eq!(line_column("abc", 2), (1, 3));
        assert_eq!(line_column("a\nbc", 2), (2, 1));
        assert_eq!(line_column("a\nbc", 3), (2, 2));
        assert_eq!(line_column("a\nb\nc", 4), (3, 1));
        // Characters, not bytes: a multi-byte line must not report
        // a column past where the operator sees the problem.
        assert_eq!(line_column("äöü=", 6), (1, 4));
        // Past the end clamps rather than panicking, which is what
        // a truncated file produces.
        assert_eq!(line_column("ab", 99), (1, 3));
    }
}
