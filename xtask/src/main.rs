mod audit;
mod backfeed;
mod changelog;
mod check;
mod clean_cache;
mod clippy_cmd;
mod coverage;
mod dep_age;
mod doc_cmd;
mod dupes;
mod feedback;
mod fmt_cmd;
mod helpers;
mod sync;
mod test_cmd;
mod todo;
mod validate;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "xtask")]
struct Cli {
    #[command(subcommand)]
    command: XCommand,
}

#[derive(Subcommand)]
enum XCommand {
    /// Fast compilation check (no tests)
    Check,
    /// Run clippy (deny warnings)
    Clippy,
    /// Check doc comments build and every doc link resolves
    Doc,
    /// Run all tests
    Test {
        /// Optional test filter
        filter: Option<String>,
        /// Show raw cargo test output
        #[arg(long)]
        verbose: bool,
        /// Run `#[ignore]`-tagged tests (e.g. manual
        /// tools). Off by default; not run by
        /// `validate`.
        #[arg(long)]
        ignored: bool,
    },
    /// Run fmt + clippy + doc + tests + coverage + duplication
    Validate {
        /// Check formatting read-only (`fmt --check`)
        /// instead of auto-fixing it in place. Use in CI
        /// or before partial staging.
        #[arg(long)]
        check: bool,
    },
    /// Format code
    Fmt,
    /// Run coverage check (requires cargo-llvm-cov)
    Coverage,
    /// Run code duplication check (requires code-dupes)
    Dupes,
    /// Security-advisory audit (RUSTSEC); requires
    /// cargo-audit
    Audit,
    /// Report a dependency version's age and flag it if
    /// within the publish cooldown (on-demand; requires curl)
    DepAge {
        /// Registry to query
        #[arg(value_enum)]
        ecosystem: dep_age::Ecosystem,
        /// Package name
        package: String,
        /// Version to check (default: latest)
        version: Option<String>,
        /// Instead of checking a version's age, print the
        /// newest version published before the cooldown (the
        /// pin target for `/update-deps`). Ignores `version`.
        #[arg(long)]
        latest_aged: bool,
    },
    /// Cooldown-check only the dependencies added or bumped in
    /// the working tree versus HEAD (the changed-deps gate that
    /// `validate` runs; requires curl + git)
    DepAgeCheck,
    /// Pin any changed Rust dependency still within the cooldown
    /// down to its newest aged version, before compiling
    /// (front-door remediation; requires curl + git + cargo)
    DepPreflight,
    /// Empty `target/{debug,release}/incremental/` while
    /// keeping the dirs themselves (manual invocation only)
    CleanCache,
    /// Print a downstream's template-feedback entries on or
    /// after its backfeed-ledger watermark (the `/template-
    /// backfeed` delta; requires the downstream path)
    BackfeedDiff {
        /// Path to the downstream rustbase-derived project
        downstream_path: String,
        /// Ledger key for this downstream (default: the final
        /// path component). Use for worktree layouts like
        /// `../ledgerstone/main` whose basename is a branch.
        #[arg(long)]
        name: Option<String>,
    },
    /// Advance the backfeed-ledger watermark for a downstream
    /// after evaluating a batch of its feedback
    BackfeedRecord {
        /// Path to the downstream rustbase-derived project
        downstream_path: String,
        /// Newest feedback-entry date evaluated (`YYYY-MM-DD`)
        #[arg(long)]
        watermark: String,
        /// Downstream commit SHA (default: read from its `.git`)
        #[arg(long)]
        head: Option<String>,
        /// Ledger key for this downstream (default: the final
        /// path component). Use for worktree layouts like
        /// `../ledgerstone/main` whose basename is a branch.
        #[arg(long)]
        name: Option<String>,
    },
    /// Append an entry to `docs/developer/template-feedback.md`
    /// with a minted `tf-<date>-<slug>` ID (body from
    /// `--body-file` or stdin)
    FeedbackAdd {
        /// Which lifecycle section to add the entry to
        #[arg(long, value_enum)]
        section: feedback::FeedbackSection,
        /// Entry title (also drives the ID slug)
        #[arg(long)]
        title: String,
        /// Read the entry body from this file (default: stdin)
        #[arg(long)]
        body_file: Option<String>,
    },
    /// Print the categorized `/template-sync` file delta from
    /// `<last-synced>` to `template/main`, minus the never-sync
    /// bookkeeping set (requires git + a fetched `template/main`)
    SyncCandidates {
        /// The `last-synced` SHA from `.template-sync.toml`
        last_synced: String,
    },
    /// Mechanically edit `CHANGELOG.md` (used by `/commit`) --
    /// insert a bullet under the right `[Unreleased]` subsection
    Changelog {
        #[command(subcommand)]
        action: changelog::ChangelogAction,
    },
    /// Mechanically read/edit `docs/todo.md` (used by `/todo` and
    /// `/implement`) -- list, add, or complete a captured item
    Todo {
        #[command(subcommand)]
        action: todo::TodoAction,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        XCommand::Check => check::check(),
        XCommand::Clippy => clippy_cmd::clippy(),
        XCommand::Doc => doc_cmd::doc(),
        XCommand::Test {
            filter,
            verbose,
            ignored,
        } => test_cmd::test(test_cmd::TestOptions {
            filter: filter.as_deref(),
            verbose,
            ignored,
        }),
        XCommand::Validate { check } => validate::validate(check),
        XCommand::Fmt => fmt_cmd::fmt(),
        XCommand::Coverage => coverage::coverage(),
        XCommand::Dupes => dupes::dupes(),
        XCommand::Audit => audit::audit(),
        XCommand::DepAge {
            ecosystem,
            package,
            version,
            latest_aged,
        } => {
            if latest_aged {
                dep_age::dep_age_latest(ecosystem, &package)
            } else {
                dep_age::dep_age(ecosystem, &package, version.as_deref())
            }
        }
        XCommand::DepAgeCheck => dep_age::dep_age_check(),
        XCommand::DepPreflight => dep_age::dep_preflight(),
        XCommand::CleanCache => clean_cache::clean_cache(),
        XCommand::BackfeedDiff {
            downstream_path,
            name,
        } => backfeed::backfeed_diff(&downstream_path, name.as_deref()),
        XCommand::BackfeedRecord {
            downstream_path,
            watermark,
            head,
            name,
        } => backfeed::backfeed_record(
            &downstream_path,
            &watermark,
            head.as_deref(),
            name.as_deref(),
        ),
        XCommand::FeedbackAdd {
            section,
            title,
            body_file,
        } => feedback::feedback_add(section, &title, body_file.as_deref()),
        XCommand::SyncCandidates { last_synced } => {
            sync::sync_candidates(&last_synced)
        }
        XCommand::Changelog { action } => changelog::changelog(action),
        XCommand::Todo { action } => todo::todo(action),
    };

    if let Err(e) = result {
        eprintln!("xtask error: {e}");
        std::process::exit(1);
    }
}
