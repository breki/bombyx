//! `changelog add`: insert a bullet into the correct subsection
//! of the `## [Unreleased]` block in `CHANGELOG.md`.
//!
//! Hand-editing `CHANGELOG.md` is error-prone: the `[Unreleased]`
//! block can hold `### Added` and `### Changed` dozens of lines
//! apart, so it is easy to split a block with a duplicate
//! heading (a documented footgun in `CLAUDE.md`). This command
//! makes the placement mechanical and testable: it finds the
//! right `### <kind>` heading under `[Unreleased]` (creating it
//! in canonical order only if absent) and appends the bullet
//! there.
//!
//! The command owns *placement*; the caller supplies the
//! *content* (which subsection, the wording). Existing bullets
//! are preserved verbatim -- only blank-line spacing inside the
//! one edited subsection is normalized. Version-bump inference
//! and the `[Unreleased]` -> dated-section promotion stay with
//! `/release`; this command is only the per-commit bullet
//! writer that `/commit` calls.

use std::fs;

use clap::{Subcommand, ValueEnum};

use crate::helpers::{
    MARKDOWN_WIDTH, rejoin, require_nonempty, section_bounds, to_owned_lines,
    workspace_root, wrap_markdown,
};

/// `changelog` subcommands.
#[derive(Subcommand)]
pub enum ChangelogAction {
    /// Add a bullet under the `[Unreleased]` section.
    Add {
        /// Which subsection the change belongs to.
        #[arg(long, value_enum)]
        kind: ChangelogKind,
        /// Mark as a breaking change: prefixes the bullet with
        /// `**BREAKING:**`, which `/release` reads to infer a
        /// major version bump.
        #[arg(long)]
        breaking: bool,
        /// The change description (one entry, wrapped to 80
        /// columns).
        text: String,
    },
}

/// A `[Unreleased]` subsection, in the project's canonical order.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum ChangelogKind {
    /// New features.
    Added,
    /// Changes to existing behaviour.
    Changed,
    /// Bug fixes.
    Fixed,
    /// Removed functionality.
    Removed,
}

impl ChangelogKind {
    /// The subsections in the order they appear in the skeleton.
    const ORDER: [ChangelogKind; 4] = [
        ChangelogKind::Added,
        ChangelogKind::Changed,
        ChangelogKind::Fixed,
        ChangelogKind::Removed,
    ];

    /// The `### <label>` heading line for this kind.
    fn heading(self) -> &'static str {
        match self {
            ChangelogKind::Added => "### Added",
            ChangelogKind::Changed => "### Changed",
            ChangelogKind::Fixed => "### Fixed",
            ChangelogKind::Removed => "### Removed",
        }
    }

    /// Position in the canonical ordering (0-based).
    fn order(self) -> usize {
        Self::ORDER
            .iter()
            .position(|&k| k == self)
            .expect("every ChangelogKind is in ORDER")
    }

    /// The kind a `### <label>` heading line names, if any (the
    /// inverse of [`heading`](Self::heading)).
    fn from_heading(line: &str) -> Option<Self> {
        Self::ORDER.into_iter().find(|k| k.heading() == line.trim())
    }
}

/// Entry point for `cargo xtask changelog <action>`.
///
/// # Errors
///
/// Returns an error if `CHANGELOG.md` cannot be read/written or
/// has no `## [Unreleased]` section.
pub fn changelog(action: ChangelogAction) -> Result<(), String> {
    match action {
        ChangelogAction::Add {
            kind,
            breaking,
            text,
        } => add(kind, breaking, &text),
    }
}

fn add(kind: ChangelogKind, breaking: bool, text: &str) -> Result<(), String> {
    require_nonempty("changelog entry text", text)?;
    let path = workspace_root().join("CHANGELOG.md");
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("read {}: {e}", path.display()))?;
    let bullet = if breaking {
        format!("**BREAKING:** {text}")
    } else {
        text.to_owned()
    };
    let updated = insert_bullet(&content, kind, &bullet)?;
    fs::write(&path, &updated)
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    println!("Added to CHANGELOG [Unreleased] under {}.", kind.heading());
    Ok(())
}

/// The end (exclusive) of a `### ` subsection whose body starts
/// at `start`: the next `### ` heading within `(start..u_end)`,
/// or `u_end`.
fn subsection_block_end(lines: &[String], start: usize, u_end: usize) -> usize {
    (start..u_end)
        .find(|&i| lines[i].trim_start().starts_with("### "))
        .unwrap_or(u_end)
}

/// Insert `text` as a bullet under `kind` in the `[Unreleased]`
/// section, returning the new file content. Pure -- no I/O -- so
/// it is unit-testable against fixtures.
fn insert_bullet(
    content: &str,
    kind: ChangelogKind,
    text: &str,
) -> Result<String, String> {
    let ends_with_newline = content.ends_with('\n');
    let mut lines = to_owned_lines(content);

    let (u_start, u_end) = section_bounds(&lines, "## [Unreleased]")
        .ok_or("CHANGELOG.md has no '## [Unreleased]' section")?;

    let bullet = wrap_markdown(text, "- ", "  ", MARKDOWN_WIDTH);

    if let Some(h) =
        ((u_start + 1)..u_end).find(|&i| lines[i].trim() == kind.heading())
    {
        append_to_existing_block(&mut lines, h, u_end, bullet);
    } else {
        insert_new_block(&mut lines, u_start, u_end, kind, bullet);
    }

    Ok(rejoin(&lines, ends_with_newline))
}

/// Append the bullet to an existing `### <kind>` block whose
/// heading is at `h`. Existing bullet lines are preserved; only
/// the block's blank-line spacing is normalized (one leading and
/// one trailing blank around the bullets).
fn append_to_existing_block(
    lines: &mut Vec<String>,
    h: usize,
    u_end: usize,
    bullet: Vec<String>,
) {
    let block_end = subsection_block_end(lines, h + 1, u_end);
    // Insert after the last non-blank line so existing bullets --
    // including any interior blank lines of a multi-paragraph
    // entry -- survive verbatim. Only an empty block (blank lines
    // only) is rewritten, to give canonical heading/blank/bullet/
    // blank spacing.
    if let Some(last) = ((h + 1)..block_end)
        .rev()
        .find(|&i| !lines[i].trim().is_empty())
    {
        let at = last + 1;
        lines.splice(at..at, bullet);
    } else {
        let mut new_block = vec![String::new()];
        new_block.extend(bullet);
        new_block.push(String::new());
        lines.splice((h + 1)..block_end, new_block);
    }
}

/// Create a new `### <kind>` block in canonical order (used only
/// when the skeleton is missing that heading) and put the bullet
/// in it.
fn insert_new_block(
    lines: &mut Vec<String>,
    u_start: usize,
    u_end: usize,
    kind: ChangelogKind,
    bullet: Vec<String>,
) {
    // Insert before the first existing heading whose canonical
    // order is greater than this kind's; else at the section end.
    let at = ((u_start + 1)..u_end)
        .find(|&i| {
            ChangelogKind::from_heading(&lines[i])
                .is_some_and(|k| k.order() > kind.order())
        })
        .unwrap_or(u_end);
    let mut block = vec![kind.heading().to_owned(), String::new()];
    block.extend(bullet);
    block.push(String::new());
    lines.splice(at..at, block);
}

#[cfg(test)]
mod tests {
    use super::*;

    const SKELETON: &str = "\
# Changelog

## [Unreleased]

### Added

### Changed

### Fixed

### Removed

## [0.1.0] - 2026-01-01

### Added

- First release.
";

    #[test]
    fn adds_to_empty_subsection() {
        let out =
            insert_bullet(SKELETON, ChangelogKind::Changed, "did a thing")
                .unwrap();
        assert!(out.contains("### Changed\n\n- did a thing\n\n### Fixed"));
        // The released section is untouched.
        assert!(out.contains(
            "## [0.1.0] - 2026-01-01\n\n### Added\n\n- First release.\n"
        ));
    }

    #[test]
    fn appends_after_an_existing_bullet() {
        let start = "\
## [Unreleased]

### Added

- existing entry

### Changed
";
        let out =
            insert_bullet(start, ChangelogKind::Added, "second entry").unwrap();
        assert!(out.contains(
            "### Added\n\n- existing entry\n- second entry\n\n### Changed"
        ));
    }

    #[test]
    fn does_not_touch_a_released_section_with_the_same_kind() {
        // Adding to [Unreleased] Added must not land in [0.1.0].
        let out = insert_bullet(SKELETON, ChangelogKind::Added, "new feature")
            .unwrap();
        let unreleased_added = out.find("### Added\n\n- new feature").unwrap();
        let released = out.find("## [0.1.0]").unwrap();
        assert!(unreleased_added < released);
    }

    #[test]
    fn breaking_prefix_is_applied_by_caller_path() {
        // insert_bullet takes the already-prefixed text.
        let out = insert_bullet(
            SKELETON,
            ChangelogKind::Removed,
            "**BREAKING:** dropped the old API",
        )
        .unwrap();
        assert!(out.contains("- **BREAKING:** dropped the old API"));
    }

    #[test]
    fn creates_a_missing_heading_in_canonical_order() {
        let start = "\
## [Unreleased]

### Added

- a feature

### Removed

- a removal
";
        // Fixed is missing; it must land between Added and Removed.
        let out = insert_bullet(start, ChangelogKind::Fixed, "a fix").unwrap();
        let added = out.find("### Added").unwrap();
        let fixed = out.find("### Fixed").unwrap();
        let removed = out.find("### Removed").unwrap();
        assert!(added < fixed && fixed < removed);
        assert!(out.contains("### Fixed\n\n- a fix\n"));
    }

    #[test]
    fn preserves_interior_blank_lines_of_existing_bullets() {
        // A multi-paragraph existing bullet must keep its interior
        // blank line when a new bullet is appended to the block.
        let start = "\
## [Unreleased]

### Changed

- first entry

  second paragraph of first entry

### Fixed
";
        let out =
            insert_bullet(start, ChangelogKind::Changed, "new entry").unwrap();
        assert!(out.contains(
            "- first entry\n\n  second paragraph of first entry\n- new entry"
        ));
    }

    #[test]
    fn errors_without_an_unreleased_section() {
        let err = insert_bullet(
            "# Changelog\n\n## [0.1.0] - 2026-01-01\n",
            ChangelogKind::Added,
            "x",
        )
        .unwrap_err();
        assert!(err.contains("Unreleased"));
    }

    #[test]
    fn preserves_trailing_newline() {
        assert!(
            insert_bullet(SKELETON, ChangelogKind::Added, "x")
                .unwrap()
                .ends_with('\n')
        );
    }

    #[test]
    fn add_rejects_blank_entry_text_before_any_io() {
        // The guard fires before `CHANGELOG.md` is read, so this
        // never touches the real file.
        let err = add(ChangelogKind::Added, false, "   ").unwrap_err();
        assert!(err.contains("empty"));
    }

    #[test]
    fn wraps_a_long_entry_to_the_markdown_width() {
        let long = "this is a fairly long changelog entry that should be \
                    wrapped across more than one line by the markdown wrapper";
        let out = insert_bullet(SKELETON, ChangelogKind::Added, long).unwrap();
        // The entry landed under Added and no line exceeds the width.
        assert!(out.contains("### Added\n\n- this is a fairly"));
        assert!(out.lines().all(|l| l.len() <= MARKDOWN_WIDTH));
    }
}
