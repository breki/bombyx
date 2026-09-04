//! Checks the claims canon prose makes about this repository.
//!
//! Some of what a canon review looks for is decidable by a
//! command: does this cross-reference resolve, does this path
//! exist, is this `git` subcommand in the skill's
//! `allowed-tools`. A reader catches most instances of such a
//! defect and never all of them, and the few classes involved
//! keep producing new instances, so the same findings come back
//! every review. A check that runs every time is not lossy that
//! way, and those findings stop reaching a reviewer at all.
//!
//! Everything here reads files and compares strings. It decides
//! nothing about phrasing, and it must stay that way: a check
//! with no stable answer is the thing `/review` says to delete
//! rather than parse harder.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;

use crate::helpers::workspace_root;

/// One thing canon claims that the tree does not support.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    /// Short tag naming which check fired.
    pub kind: &'static str,
    /// Repo-relative file the claim is in.
    pub file: String,
    /// 1-indexed line the claim is on.
    pub line: usize,
    /// What is wrong, in one clause.
    pub message: String,
}

/// Paths named in prose that are correct to be absent here.
///
/// An entry matches as a prefix, so a trailing `/` exempts a
/// whole subtree and a bare filename exempts anything
/// starting with it -- `scripts/e2e.sh` also covers
/// `scripts/e2e.sh.bak`.
///
/// Each entry needs a reason, because an unexplained entry is
/// indistinguishable from a defect somebody silenced.
const ABSENT_ON_PURPOSE: &[(&str, &str)] = &[
    // Named only by sentences stating it does not exist -- the
    // template shipped it and this project removed it.
    ("scripts/e2e.sh", "named only to say it is absent"),
    // Upstream template paths, quoted for
    // `git show template/main:<path>` in /template-sync.
    ("crates/rustbase/", "path in the upstream template"),
    // /template-backfeed runs in the template repo only, and
    // the ledger lives there with it.
    ("docs/developer/backfeed-ledger.toml", "template repo only"),
];

/// Directory prefixes that make a backticked string a claim
/// about a file in this repository rather than prose.
const REPO_PREFIXES: &[&str] =
    &["crates/", "xtask/", "docs/", "scripts/", ".claude/"];

/// Maximum prose width, matching `rustfmt.toml` and the
/// markdown rule in `CLAUDE.md`.
const MAX_COLS: usize = 80;

/// How many checks run, for the summary line only. Nothing
/// compares it against `collect`, so bump it by hand when a
/// check is added: a stale value misreports the count and
/// breaks nothing else.
const CHECK_COUNT: usize = 5;

/// Text between the first `open` and the next `close` after it,
/// with the byte index the match started at.
fn between<'a>(
    hay: &'a str,
    open: &str,
    close: &str,
) -> Option<(usize, &'a str)> {
    let start = hay.find(open)?;
    let rest = &hay[start + open.len()..];
    let end = rest.find(close)?;
    Some((start, &rest[..end]))
}

/// A paragraph, one list item or one heading, with its lines
/// joined.
///
/// Every markdown file here wraps at 80 columns, so a bold
/// lead-in or a cross-reference is regularly split across two
/// lines and a line-by-line search never sees it. `text` holds
/// the lines trimmed and joined with a single space, and
/// `first_line` with `more` remembers which source line each
/// piece came from, so a finding still names a line the
/// author can open.
struct Block {
    /// The trimmed lines, joined with one space.
    text: String,
    /// The 1-indexed source line the block opens with. It is
    /// a field rather than the first `more` entry so that
    /// `line_at` always has an answer to give.
    first_line: usize,
    /// One entry per line after the first: the byte offset
    /// where that line begins in `text`, and its 1-indexed
    /// source line.
    more: Vec<(usize, usize)>,
}

impl Block {
    /// A block holding one line.
    fn new(first_line: usize, text: &str) -> Self {
        Self {
            text: text.to_string(),
            first_line,
            more: Vec::new(),
        }
    }

    /// Append a wrapped continuation line.
    fn push(&mut self, source_line: usize, text: &str) {
        self.text.push(' ');
        self.more.push((self.text.len(), source_line));
        self.text.push_str(text);
    }

    /// Where each joined line begins in `text`.
    fn line_offsets(&self) -> impl Iterator<Item = usize> + '_ {
        std::iter::once(0).chain(self.more.iter().map(|(at, _)| *at))
    }

    /// The 1-indexed source line that byte `offset` fell in.
    fn line_at(&self, offset: usize) -> usize {
        self.more
            .iter()
            .rev()
            .find(|(at, _)| *at <= offset)
            .map_or(self.first_line, |(_, line)| *line)
    }
}

/// The text after a `- `, `* `, `+ ` or `12. ` list marker, or
/// the whole line when it carries none.
fn strip_list_marker(line: &str) -> &str {
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(marker) {
            return rest.trim_start();
        }
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && line[digits..].starts_with(". ") {
        return line[digits + 2..].trim_start();
    }
    line
}

/// Split the file into blocks of consecutive non-blank lines.
///
/// A blank line, a heading and a new list item each end the
/// block before them. Ending at a blank line is what stops a
/// `**` left open in one paragraph from closing in the next
/// and inventing a target that spans both.
///
/// A heading is a block of its own: it ends the block before
/// it and is closed straight away, so its text never joins
/// the paragraph beneath it. Without that, a `**` opened in a
/// heading pairs with one in the prose below and the checker
/// reports a pointer nobody wrote.
fn blocks(content: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut cur: Option<Block> = None;
    for (i, raw) in content.lines().enumerate() {
        let line = raw.trim();
        let heading = line.starts_with('#');
        let breaks = line.is_empty()
            || heading
            || strip_list_marker(line).len() != line.len();
        if breaks && let Some(block) = cur.take() {
            out.push(block);
        }
        if line.is_empty() {
            continue;
        }
        if heading {
            out.push(Block::new(i + 1, line));
            continue;
        }
        match &mut cur {
            Some(block) => block.push(i + 1, line),
            None => cur = Some(Block::new(i + 1, line)),
        }
    }
    out.extend(cur);
    out
}

/// Every `**Bold**` run that opens a line, after any list
/// marker, plus every heading. These are what a
/// cross-reference may name.
///
/// A line, not a paragraph: a rule as often opens the third
/// line of a paragraph as the first, and a bold run that
/// merely lands at a line start by an accident of wrapping
/// becomes citable too. Accepting those costs nothing -- a
/// target nobody names is never looked up -- while missing
/// one fails a correct pointer.
pub fn reference_targets(content: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for raw in content.lines() {
        let line = raw.trim_start();
        if let Some(rest) = line.strip_prefix('#') {
            let title = rest.trim_start_matches('#').trim();
            // `### 2. Snapshot` is referred to as **Snapshot**.
            let title = title.split_once(". ").map_or(title, |(head, tail)| {
                if head.chars().all(|c| c.is_ascii_digit()) {
                    tail
                } else {
                    title
                }
            });
            out.insert(title.to_string());
        }
    }
    for block in blocks(content) {
        // Scanning from a line start rather than from the
        // whole block is what lets a `**` run that wraps
        // still close.
        for offset in block.line_offsets() {
            let lead = strip_list_marker(&block.text[offset..]);
            if lead.starts_with("**")
                && let Some((_, inner)) = between(lead, "**", "**")
            {
                out.insert(inner.trim_end_matches('.').to_string());
            }
        }
    }
    out
}

/// Cross-references of the form `under **X**` whose `X` names
/// no heading and no bold-led paragraph anywhere in canon.
///
/// Only `under` is scanned. That is the form these files
/// actually use for a pointer, and widening it to `in` or `at`
/// matches ordinary prose such as "the only one asked what
/// **worked**".
pub fn unresolved_xrefs(
    file: &str,
    content: &str,
    targets: &BTreeSet<String>,
) -> Vec<Finding> {
    let mut out = Vec::new();
    for block in blocks(content) {
        let mut rest = block.text.as_str();
        let mut base = 0;
        while let Some((at, inner)) = between(rest, "under **", "**") {
            if !targets.contains(inner) {
                out.push(Finding {
                    kind: "xref",
                    file: file.to_string(),
                    line: block.line_at(base + at),
                    message: format!("**{inner}** names no heading"),
                });
            }
            // Resume just past `under **` rather than past
            // the closing `**`. An unclosed run pairs with
            // the *opening* `**` of the next pointer, and
            // skipping the pair would swallow that pointer's
            // `under **` and stop checking it.
            let step = at + "under **".len();
            base += step;
            rest = &rest[step..];
        }
    }
    out
}

/// Backticked repository paths that do not exist.
///
/// `exists` is injected so the tests need no files on disk.
pub fn missing_paths(
    file: &str,
    content: &str,
    exists: &dyn Fn(&str) -> bool,
) -> Vec<Finding> {
    let mut out = Vec::new();
    for (i, line) in content.lines().enumerate() {
        for span in line.split('`').skip(1).step_by(2) {
            let p = span.trim();
            if !REPO_PREFIXES.iter().any(|d| p.starts_with(d)) {
                continue;
            }
            // A glob, a placeholder such as `<slug>` and a
            // bare directory are all claims about a shape
            // rather than about one file.
            if p.contains('*') || p.contains('<') || p.ends_with('/') {
                continue;
            }
            if ABSENT_ON_PURPOSE
                .iter()
                .any(|(a, _)| p == *a || p.starts_with(a))
            {
                continue;
            }
            if !exists(p) {
                out.push(Finding {
                    kind: "path",
                    file: file.to_string(),
                    line: i + 1,
                    message: format!("`{p}` does not exist"),
                });
            }
        }
    }
    out
}

/// `git` subcommands a command file tells the agent to run that
/// its own `allowed-tools` does not grant.
///
/// Two things are skipped, because neither is a command the
/// skill runs itself. A command inside a double-quoted span is
/// a line the skill prints for the operator. And a file that
/// says in prose it has ``no `git <sub>` grant`` has declared
/// the omission deliberate.
pub fn ungranted_git(file: &str, content: &str) -> Vec<Finding> {
    let Some(grants) =
        content.lines().find(|l| l.starts_with("allowed-tools:"))
    else {
        return Vec::new();
    };
    // The declaration may wrap across lines, so match it
    // against the text with every whitespace run collapsed.
    let flat = content.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = Vec::new();
    for (i, line) in content.lines().enumerate() {
        if line.starts_with("allowed-tools:") {
            continue;
        }
        if line.contains('"') {
            continue;
        }
        for span in line.split('`').skip(1).step_by(2) {
            let Some(rest) = span.trim().strip_prefix("git ") else {
                continue;
            };
            let sub = rest.split_whitespace().next().unwrap_or_default();
            if sub.is_empty() || sub.starts_with('<') {
                continue;
            }
            if grants.contains(&format!("Bash(git {sub}")) {
                continue;
            }
            // A file may state that it deliberately lacks a
            // grant, when the command is one it tells the
            // operator to run rather than running it. The
            // declaration is in the prose, so it greps.
            if flat.contains(&format!("no `git {sub}` grant")) {
                continue;
            }
            out.push(Finding {
                kind: "grant",
                file: file.to_string(),
                line: i + 1,
                message: format!("runs `git {sub}`, not in allowed-tools"),
            });
        }
    }
    out
}

/// Prose lines wider than 80 columns.
///
/// Frontmatter values, table rows and fenced or indented code
/// are exempt: none of them can be wrapped without changing
/// what they mean.
pub fn over_wide(file: &str, content: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut in_front = false;
    let mut in_fence = false;
    for (i, line) in content.lines().enumerate() {
        if i == 0 && line.trim() == "---" {
            in_front = true;
            continue;
        }
        if in_front {
            if line.trim() == "---" {
                in_front = false;
            }
            continue;
        }
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence
            || line.starts_with('|')
            || line.starts_with("    ")
            || line.starts_with('\t')
        {
            continue;
        }
        if line.chars().count() > MAX_COLS {
            out.push(Finding {
                kind: "cols",
                file: file.to_string(),
                line: i + 1,
                message: format!("{} columns", line.chars().count()),
            });
        }
    }
    out
}

/// Backlog IDs cited in prose that appear in no backlog.
///
/// The ID scheme exists so an ID greps; a citation that finds
/// nothing defeats it.
pub fn unknown_ids(
    file: &str,
    content: &str,
    known: &BTreeSet<String>,
) -> Vec<Finding> {
    let mut out = Vec::new();
    for (i, line) in content.lines().enumerate() {
        for word in line.split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        {
            if !is_backlog_id(word) || known.contains(word) {
                continue;
            }
            out.push(Finding {
                kind: "id",
                file: file.to_string(),
                line: i + 1,
                message: format!("{word} is in no backlog"),
            });
        }
    }
    out
}

/// True for `rt-`/`aq-`/`fr-` followed by an ISO date and a
/// non-empty slug.
fn is_backlog_id(word: &str) -> bool {
    let Some(rest) = ["rt-", "aq-", "fr-"]
        .iter()
        .find_map(|p| word.strip_prefix(*p))
    else {
        return false;
    };
    let parts: Vec<&str> = rest.splitn(4, '-').collect();
    parts.len() == 4
        && parts[0].len() == 4
        && parts[1].len() == 2
        && parts[2].len() == 2
        && parts[..3]
            .iter()
            .all(|p| p.chars().all(|c| c.is_ascii_digit()))
        && !parts[3].is_empty()
}

/// The `### <id>` headings in a backlog file.
pub fn ids_in_backlog(content: &str) -> BTreeSet<String> {
    content
        .lines()
        .filter_map(|l| l.strip_prefix("### "))
        .map(|s| s.trim().to_string())
        .filter(|s| is_backlog_id(s))
        .collect()
}

/// Canon files, repo-relative, in a stable order.
fn canon_files(root: &Path) -> Vec<String> {
    let mut out = vec!["CLAUDE.md".to_string(), "llms.txt".to_string()];
    for dir in [".claude/commands", ".claude/agents"] {
        let Ok(entries) = std::fs::read_dir(root.join(dir)) else {
            continue;
        };
        let mut names: Vec<String> = entries
            .filter_map(Result::ok)
            .filter_map(|e| {
                let path = e.path();
                if path.extension()? != "md" {
                    return None;
                }
                let n = e.file_name().to_string_lossy().to_string();
                Some(format!("{dir}/{n}"))
            })
            .collect();
        names.sort();
        out.extend(names);
    }
    out.retain(|f| root.join(f).is_file());
    out
}

/// Run every canon check over the tree.
///
/// The findings come out grouped by check, then by file and
/// line, so two runs over an unchanged tree print the same
/// text.
///
/// Returns the findings and the number of files read, so both
/// callers can report the same counts.
pub fn collect() -> Result<(Vec<Finding>, usize), String> {
    let root = workspace_root();
    let files = canon_files(&root);

    let mut sources: Vec<(String, String)> = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(root.join(f))
            .map_err(|e| format!("cannot read {f}: {e}"))?;
        sources.push((f.clone(), text));
    }

    let mut targets = BTreeSet::new();
    for (_, text) in &sources {
        targets.extend(reference_targets(text));
    }

    let mut known_ids = BTreeSet::new();
    if let Ok(entries) = std::fs::read_dir(root.join("docs/developer")) {
        for e in entries.filter_map(Result::ok) {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.ends_with("-log.md") {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(e.path()) {
                known_ids.extend(ids_in_backlog(&text));
            }
        }
    }

    let exists = |p: &str| root.join(p).exists();
    let mut findings = Vec::new();
    for (f, text) in &sources {
        findings.extend(unresolved_xrefs(f, text, &targets));
        findings.extend(missing_paths(f, text, &exists));
        findings.extend(ungranted_git(f, text));
        findings.extend(over_wide(f, text));
        findings.extend(unknown_ids(f, text, &known_ids));
    }
    findings.sort();
    Ok((findings, sources.len()))
}

/// The findings rendered one per line, for an error message.
fn render(findings: &[Finding]) -> String {
    let mut msg = String::new();
    for f in findings {
        let _ =
            writeln!(msg, "  [{}] {}:{} {}", f.kind, f.file, f.line, f.message);
    }
    let _ = write!(msg, "{} canon finding(s)", findings.len());
    msg
}

/// Run the checks and return a one-line summary, for
/// `validate` to print beside the step name.
pub fn canon_check_detail() -> Result<String, String> {
    let (findings, files) = collect()?;
    if findings.is_empty() {
        Ok(format!("{files} files, {CHECK_COUNT} checks"))
    } else {
        Err(render(&findings))
    }
}

/// Run the checks standalone, printing the outcome.
pub fn canon_check() -> Result<(), String> {
    let detail = canon_check_detail()?;
    println!("Canon OK ({detail})");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(from: &str) -> BTreeSet<String> {
        reference_targets(from)
    }

    #[test]
    fn heading_and_bold_lead_are_both_reference_targets() {
        let t = targets("## When to run\n\n**Diff handoff.** x\n");
        assert!(t.contains("When to run"));
        assert!(t.contains("Diff handoff"));
    }

    #[test]
    fn numbered_heading_is_named_without_its_number() {
        let t = targets("### 2. Run it before anyone reads it\n");
        assert!(t.contains("Run it before anyone reads it"));
    }

    #[test]
    fn unresolved_xref_is_reported_and_resolved_one_is_not() {
        let t = targets("## Snapshot\n");
        assert!(
            unresolved_xrefs("a.md", "see under **Snapshot** ok", &t)
                .is_empty()
        );
        let bad = unresolved_xrefs("a.md", "under **Nowhere** x", &t);
        assert_eq!(bad.len(), 1);
        assert_eq!(bad[0].kind, "xref");
    }

    /// Ordinary prose puts bold after `in` and `at` constantly,
    /// so only the pointer form counts.
    #[test]
    fn bold_after_other_prepositions_is_not_a_reference() {
        let t = BTreeSet::new();
        assert!(
            unresolved_xrefs("a.md", "the only one asked what **worked**", &t)
                .is_empty()
        );
    }

    /// Every markdown file here wraps at 80 columns, so a
    /// bold lead-in longer than about 70 characters spans two
    /// lines. Reading it line by line makes it invisible.
    #[test]
    fn a_bold_lead_in_that_wraps_is_still_a_reference_target() {
        let t = targets(
            "**Ask before testing something that is\nnot the program.** Then x\n",
        );
        assert!(
            t.contains("Ask before testing something that is not the program")
        );
    }

    /// Most rules in `CLAUDE.md` are list items, so the bold
    /// lead sits behind a `- ` marker.
    #[test]
    fn a_list_item_bold_lead_is_a_reference_target() {
        let t = targets(
            "- **Do not grep canon prose for a\n  phrase.** Every file wraps.\n",
        );
        assert!(t.contains("Do not grep canon prose for a phrase"));
    }

    /// Joining must stop at a blank line, or a bold run left
    /// open in one paragraph closes in the next and invents a
    /// target spanning both.
    #[test]
    fn a_bold_run_does_not_close_across_a_paragraph_break() {
        let t = targets("**Unclosed lead\n\nand a later** bold\n");
        assert!(!t.contains("Unclosed lead and a later"));
    }

    /// A rule often opens a line partway through a paragraph.
    /// Reading only the paragraph's first line loses it, which
    /// is the defect the whole change exists to remove.
    #[test]
    fn a_bold_lead_on_a_continuation_line_is_a_target() {
        let t = targets(
            "Prefer strong types. Avoid primitive obsession.\n**The representation has to be argued for.**\n`ScriptPath` is a checked `String`.\n",
        );
        assert!(t.contains("The representation has to be argued for"));
    }

    /// An unclosed `under **` pairs with the *opening* `**`
    /// of the next pointer. Resuming after that pair would
    /// eat the second pointer's `under **` and stop checking
    /// it, and joining the lines widened the loss from the
    /// rest of a line to the rest of a paragraph.
    #[test]
    fn an_unclosed_bold_does_not_hide_the_next_pointer() {
        let bad = unresolved_xrefs(
            "a.md",
            "see under **Unclosed and\nalso under **Nope** here\n",
            &BTreeSet::new(),
        );
        assert_eq!(bad.len(), 2, "both pointers are checked");
        assert!(bad[1].message.contains("Nope"));
        assert_eq!(bad[1].line, 2);
    }

    /// A heading is not always followed by a blank line, and
    /// the bold rule under one is still a target.
    #[test]
    fn a_bold_lead_directly_under_a_heading_is_a_target() {
        let t = targets("## Rules\n**Say it once.** No preamble.\n");
        assert!(t.contains("Say it once"));
    }

    /// What the heading break is for: a heading's text must
    /// not join the paragraph beneath it, or a `**` opened in
    /// the heading pairs with one below and the checker
    /// reports a pointer nobody wrote.
    #[test]
    fn a_heading_does_not_join_the_paragraph_below_it() {
        let bad = unresolved_xrefs(
            "a.md",
            "## Rules under **No\nsuch rule** x\n",
            &BTreeSet::new(),
        );
        assert!(bad.is_empty(), "got {bad:?}");
    }

    /// Two bullets with no blank line between them are one
    /// paragraph to a naive join, which would lose the second
    /// lead and merge the first into it.
    #[test]
    fn adjacent_list_items_stay_separate_targets() {
        let t = targets("- **First rule.** x\n- **Second rule.** y\n");
        assert!(t.contains("First rule"));
        assert!(t.contains("Second rule"));
    }

    #[test]
    fn list_markers_are_stripped_and_ordinary_text_is_not() {
        for (line, want) in [
            ("- item", "item"),
            ("* item", "item"),
            ("+ item", "item"),
            ("12. item", "item"),
            ("12 item", "12 item"),
            ("12.item", "12.item"),
            ("item", "item"),
        ] {
            assert_eq!(strip_list_marker(line), want, "on {line}");
        }
    }

    /// The second pointer in a joined paragraph has to be
    /// attributed to its own source line, not the first's.
    #[test]
    fn each_pointer_in_a_joined_paragraph_names_its_own_line() {
        let bad = unresolved_xrefs(
            "a.md",
            "see under **Nope one** and\nalso under **Nope two** here\n",
            &BTreeSet::new(),
        );
        assert_eq!(bad.len(), 2);
        assert_eq!((bad[0].line, bad[1].line), (1, 2));
    }

    /// The citing side wraps too, and a pointer split over two
    /// lines was checked by nothing at all.
    #[test]
    fn a_wrapped_xref_is_checked_and_reported_on_its_own_line() {
        let t = targets("## Snapshot\n");
        assert!(
            unresolved_xrefs(
                "a.md",
                "as it says\nunder **Snapshot**, do it\n",
                &t
            )
            .is_empty()
        );
        let bad = unresolved_xrefs(
            "a.md",
            "one\ntwo under **No\nsuch rule** x\n",
            &t,
        );
        assert_eq!(bad.len(), 1);
        assert_eq!(bad[0].message, "**No such rule** names no heading");
        assert_eq!(bad[0].line, 2);
    }

    #[test]
    fn missing_repo_path_is_reported() {
        let none = |_: &str| false;
        let f = missing_paths("a.md", "see `xtask/src/gone.rs`", &none);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "path");
    }

    #[test]
    fn globs_dirs_and_allowlisted_paths_are_skipped() {
        let none = |_: &str| false;
        for text in [
            "`docs/developer/*-log.md`",
            "`docs/issues/`",
            "`scripts/e2e.sh`",
            "`crates/rustbase/Cargo.toml`",
            "`docs/developer/backfeed-ledger.toml`",
        ] {
            assert!(
                missing_paths("a.md", text, &none).is_empty(),
                "{text} should be skipped"
            );
        }
    }

    #[test]
    fn non_repo_backticks_are_not_paths() {
        let none = |_: &str| false;
        assert!(
            missing_paths("a.md", "`--dry-run` and `String`", &none).is_empty()
        );
    }

    #[test]
    fn ungranted_git_subcommand_is_reported() {
        let doc = "allowed-tools: Bash(git status:*)\n\nrun `git tag` now\n";
        let f = ungranted_git("c.md", doc);
        assert_eq!(f.len(), 1);
        assert!(f[0].message.contains("git tag"));
    }

    #[test]
    fn granted_subcommand_and_narrower_grant_both_pass() {
        let doc = "allowed-tools: Bash(git status:*), Bash(git config --get:*)\n\n`git status` and `git config --get user.email`\n";
        assert!(ungranted_git("c.md", doc).is_empty());
    }

    /// A quoted line is text the skill prints for the operator,
    /// not a command it runs.
    #[test]
    fn git_inside_a_quoted_span_is_not_the_skills_own_command() {
        let doc =
            "allowed-tools: Bash(git log:*)\n\n- \"Push with `git push`\"\n";
        assert!(ungranted_git("c.md", doc).is_empty());
    }

    /// The declaration is prose and wraps, so the match has
    /// to survive a newline landing inside it.
    #[test]
    fn a_declared_missing_grant_is_accepted_even_when_wrapped() {
        let doc = "allowed-tools: Bash(git log:*)\n\ntell them to run\n`git reset -- <path>` themselves; this skill has no\n`git reset` grant, deliberately.\n";
        assert!(ungranted_git("c.md", doc).is_empty());
    }

    #[test]
    fn a_file_without_allowed_tools_is_not_checked_for_grants() {
        assert!(ungranted_git("x.md", "run `git push`\n").is_empty());
    }

    #[test]
    fn over_wide_prose_is_reported() {
        let long = "x".repeat(81);
        assert_eq!(over_wide("a.md", &long).len(), 1);
        assert!(over_wide("a.md", &"y".repeat(80)).is_empty());
    }

    #[test]
    fn frontmatter_tables_fences_and_indents_are_exempt() {
        let long = "x".repeat(120);
        let cases = [
            format!("---\ndescription: {long}\n---\n"),
            format!("| {long} |\n"),
            format!("```\n{long}\n```\n"),
            format!("    {long}\n"),
        ];
        for c in &cases {
            assert!(
                over_wide("a.md", c).is_empty(),
                "should be exempt: {}",
                &c[..12.min(c.len())]
            );
        }
    }

    #[test]
    fn unknown_backlog_id_is_reported_and_known_one_is_not() {
        let known: BTreeSet<String> =
            ["rt-2026-09-03-real".to_string()].into_iter().collect();
        assert!(
            unknown_ids("a.md", "see rt-2026-09-03-real", &known).is_empty()
        );
        let f = unknown_ids("a.md", "see rt-2026-09-03-ghost", &known);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "id");
    }

    #[test]
    fn id_shape_is_prefix_date_and_slug() {
        assert!(is_backlog_id("aq-2026-09-03-a-slug"));
        assert!(!is_backlog_id("rt-2026-09-03"));
        assert!(!is_backlog_id("rt-26-9-3-slug"));
        assert!(!is_backlog_id("xx-2026-09-03-slug"));
        assert!(!is_backlog_id("review"));
    }

    #[test]
    fn backlog_headings_yield_their_ids() {
        let ids =
            ids_in_backlog("# Log\n\n### rt-2026-09-03-one\n\n### Not an id\n");
        assert_eq!(ids.len(), 1);
        assert!(ids.contains("rt-2026-09-03-one"));
    }
}
