//! Build automation for the workspace. Every quality gate
//! runs from here, so a gate is a Rust function with tests
//! rather than a script that has to be written twice.

// The `#[cfg(feature)]` groups are the optional command groups
// declared in `Cargo.toml`.
#[cfg(feature = "template-tools")]
mod backfeed;
#[cfg(feature = "clean-cache")]
mod clean_cache;
#[cfg(feature = "deploy")]
mod deploy;
#[cfg(feature = "deploy")]
mod deploy_config;
#[cfg(feature = "deploy")]
mod deploy_guard;
#[cfg(feature = "deploy")]
mod deploy_remote;
#[cfg(feature = "deploy")]
mod deploy_setup;
#[cfg(feature = "template-tools")]
mod feedback;
#[cfg(feature = "frontend")]
mod frontend;
#[cfg(feature = "frontend")]
mod frontend_check;
#[cfg(feature = "frontend")]
mod frontend_dupes;
#[cfg(feature = "frontend")]
mod frontend_fmt;
#[cfg(feature = "frontend")]
mod frontend_test;
mod helpers;
#[cfg(feature = "template-tools")]
mod sync;
mod validate;

use clap::{Parser, Subcommand};
use rustbase_xtask::cli::{self, LibCommand};

use helpers::tasks_config;

// No description: clap would otherwise print the doc comment of a
// command enum, the library's included, as one.
#[derive(Parser)]
#[command(name = "xtask", about = None, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: XCommand,
}

/// The library's subcommands, then this project's own.
#[derive(Subcommand)]
enum XCommand {
    #[command(flatten)]
    Lib(LibCommand),
    /// Run fmt + clippy + tests + coverage + duplication
    Validate {
        /// Check formatting read-only (`fmt --check`)
        /// instead of auto-fixing it in place. Use in CI
        /// or before partial staging.
        #[arg(long)]
        check: bool,
    },
    /// Type-check the frontend (svelte-check); skips
    /// cleanly when there is no frontend
    #[cfg(feature = "frontend")]
    FrontendCheck,
    /// Format the frontend with Prettier; skips cleanly
    /// when there is no frontend
    #[cfg(feature = "frontend")]
    FrontendFmt {
        /// Check formatting read-only instead of
        /// auto-fixing in place
        #[arg(long)]
        check: bool,
    },
    /// Check frontend code duplication (requires jscpd);
    /// skips cleanly when there is no frontend
    #[cfg(feature = "frontend")]
    FrontendDupes,
    /// Run the frontend unit suite (vitest); skips cleanly
    /// when there is no frontend
    #[cfg(feature = "frontend")]
    FrontendTest,
    /// Deploy to a remote Linux host (build frontend, sync,
    /// build on remote, restart service)
    #[cfg(feature = "deploy")]
    Deploy,
    /// One-time remote provisioning (user, dirs, service)
    #[cfg(feature = "deploy")]
    DeploySetup,
    /// Empty `target/{debug,release}/incremental/` while
    /// keeping the dirs themselves (manual invocation only)
    #[cfg(feature = "clean-cache")]
    CleanCache,
    /// Print a downstream's template-feedback entries on or
    /// after its backfeed-ledger watermark (the `/template-
    /// backfeed` delta; requires the downstream path)
    #[cfg(feature = "template-tools")]
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
    #[cfg(feature = "template-tools")]
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
    #[cfg(feature = "template-tools")]
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
    #[cfg(feature = "template-tools")]
    SyncCandidates {
        /// The `last-synced` SHA from `.template-sync.toml`
        last_synced: String,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        XCommand::Lib(cmd) => cli::run(cmd, &tasks_config()),
        XCommand::Validate { check } => validate::validate(check),
        #[cfg(feature = "frontend")]
        XCommand::FrontendCheck => frontend_check::frontend_check_cmd(),
        #[cfg(feature = "frontend")]
        XCommand::FrontendFmt { check } => {
            frontend_fmt::frontend_fmt_cmd(check)
        }
        #[cfg(feature = "frontend")]
        XCommand::FrontendDupes => frontend_dupes::frontend_dupes_cmd(),
        #[cfg(feature = "frontend")]
        XCommand::FrontendTest => frontend_test::frontend_test_cmd(),
        #[cfg(feature = "deploy")]
        XCommand::Deploy => deploy::deploy(),
        #[cfg(feature = "deploy")]
        XCommand::DeploySetup => deploy_setup::deploy_setup(),
        #[cfg(feature = "clean-cache")]
        XCommand::CleanCache => clean_cache::clean_cache(),
        #[cfg(feature = "template-tools")]
        XCommand::BackfeedDiff {
            downstream_path,
            name,
        } => backfeed::backfeed_diff(&downstream_path, name.as_deref()),
        #[cfg(feature = "template-tools")]
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
        #[cfg(feature = "template-tools")]
        XCommand::FeedbackAdd {
            section,
            title,
            body_file,
        } => feedback::feedback_add(section, &title, body_file.as_deref()),
        #[cfg(feature = "template-tools")]
        XCommand::SyncCandidates { last_synced } => {
            sync::sync_candidates(&last_synced)
        }
    };

    if let Err(e) = result {
        eprintln!("{}", error_line(&e));
        std::process::exit(1);
    }
}

/// The line a failed command ends with, which `validate` also writes
/// to its log.
fn error_line(e: &str) -> String {
    format!("xtask error: {e}")
}
