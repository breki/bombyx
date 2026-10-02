//! Tests that run the listing script through a real `sh`, with a
//! stub `vagrant` on `PATH`.
//!
//! The listing script is what `super::super::vagrant_status_many`
//! builds for `bombyx list`. It asks `vagrant status` about every
//! project on one host, at most `STATUS_BATCH` calls at a time,
//! and prints `LISTING_MARKER` and the project's name before each
//! reply so the workstation can split them apart. What these tests
//! cover is timing and output order, which only an executed script
//! has.

use super::*;
use std::os::unix::fs::PermissionsExt;
use tempfile::TempDir;

/// A `vagrant` that waits until every project's call has
/// started, then answers `running`, or `alone` when it gave up
/// waiting.
///
/// The wait is what makes the test fail when the calls run one
/// after another: the first call never sees the others start.
/// `EXPECT` is the number of projects to wait for.
///
/// The stub delays `p1` by 0.6 s and `p2` by 0.3 s, so with the
/// names `p1`, `p2`, `p3` the replies finish in reverse order,
/// and a script that printed each reply as it arrived would get
/// the order wrong. A test using this stub must use those
/// names, or it loses that order check.
const STUB: &str = r#"#!/bin/sh
name=$(basename "$PWD")
touch "$MARKS/$name"
started() { ls "$MARKS" | wc -l; }
i=0
while [ "$(started)" -lt "$EXPECT" ] && [ "$i" -lt 50 ]; do
    sleep 0.1; i=$((i + 1))
done
case $name in p1) sleep 0.6 ;; p2) sleep 0.3 ;; esac
if [ "$(started)" -ge "$EXPECT" ]; then state=running; else state=alone; fi
echo "1,default,state,$state"
"#;

/// A `vagrant` that notes how many calls were running when it
/// started, in `seen.<project>`, then answers after a pause
/// long enough for every call of one batch to overlap.
const COUNTING_STUB: &str = r#"#!/bin/sh
name=$(basename "$PWD")
touch "$MARKS/run.$name"
ls "$MARKS" | grep -c '^run\.' > "$MARKS/seen.$name"
sleep 0.5
rm "$MARKS/run.$name"
echo "1,default,state,running"
"#;

/// Runs the listing script for `names`, one built project
/// each, with `stub` as `vagrant`. Returns the marks directory
/// the stub wrote to, inside the `TempDir` that holds it.
fn run_listing(
    stub: &str,
    names: &[&str],
) -> (TempDir, std::path::PathBuf, std::process::Output) {
    let home = TempDir::new().unwrap();
    let bin = home.path().join("bin");
    let marks = home.path().join("marks");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&marks).unwrap();
    let vagrant = bin.join("vagrant");
    std::fs::write(&vagrant, stub).unwrap();
    std::fs::set_permissions(&vagrant, std::fs::Permissions::from_mode(0o755))
        .unwrap();

    let configs: Vec<Config> = names
        .iter()
        .map(|name| {
            let dir = home.path().join("vms").join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("Vagrantfile"), "").unwrap();
            let mut cfg = Config::for_tests_local();
            cfg.project = crate::name::ProjectName::parse(name).unwrap();
            cfg
        })
        .collect();
    let rest: Vec<&Config> = configs[1..].iter().collect();
    let cmd = vagrant_status_many(&configs[0], &rest);

    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = std::process::Command::new(&cmd.program)
        .args(&cmd.args)
        .env("HOME", home.path())
        .env("PATH", path)
        .env("MARKS", &marks)
        .env("EXPECT", names.len().to_string())
        .output()
        .unwrap();
    (home, marks, out)
}

/// The reply bombyx expects when every project says `running`.
fn all_running(names: &[&str]) -> String {
    names
        .iter()
        .map(|n| format!("{LISTING_MARKER}{n}\n1,default,state,running\n"))
        .collect::<Vec<_>>()
        .concat()
}

#[test]
fn a_host_asks_about_its_projects_at_the_same_time() {
    let names = ["p1", "p2", "p3"];
    let (_home, _marks, out) = run_listing(STUB, &names);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        all_running(&names),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{out:?}");
}

#[test]
fn a_host_runs_at_most_a_batch_of_calls_at_once() {
    // One more project than a batch holds, so an uncapped
    // script runs them all together and one call sees five.
    let names = ["p1", "p2", "p3", "p4", "p5"];
    assert_eq!(names.len(), STATUS_BATCH + 1);
    let (_home, marks, out) = run_listing(COUNTING_STUB, &names);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        all_running(&names),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let seen: Vec<usize> = names
        .iter()
        .map(|n| {
            std::fs::read_to_string(marks.join(format!("seen.{n}")))
                .unwrap()
                .trim()
                .parse()
                .unwrap()
        })
        .collect();
    assert_eq!(
        seen.iter().max(),
        Some(&STATUS_BATCH),
        "calls running at each start: {seen:?}"
    );
}
