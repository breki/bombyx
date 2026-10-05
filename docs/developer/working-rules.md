# Working rules

Three habits that keep a fix honest before anyone reviews it.
`CLAUDE.md` under **Collaboration** points here; each section is
the rule and the reason it holds.

## Fix or log a gap your own summary names

When a summary of your own work names a likely gap -- a list you
did not check, a case you suspect is missing -- fix it before the
commit if the fix is cheap and safe, and log it in the reviewer
backlog otherwise. A gap left as a caveat in the hand-off reaches
review as a defect, where it costs a round instead of a line.

## A deliberate break must still compile

To show a test catches a defect, break the code on purpose and
watch the test fail. The broken code must still compile under
deny-warnings, and the run must print the failing test's name.
Empty output from a filtered `cargo xtask test ... | grep` means
the build failed and no test ran, not that the tests pass. So
choose a break that keeps every variable used -- an early `break`
rather than a deleted assignment -- and grep for
`FAILED|Test OK|compilation`, so a build failure shows.

## Read the tool's source before trusting its file

When a guard decides something from a file another tool owns --
vagrant's `.vagrant/machines/...` files, a libvirt record -- read
that tool's installed source for every path that writes, deletes
or ignores the file, and state each case in the guard's doc
before the first test. A file's presence rarely means only one
thing: vagrant's `action_provision` is written before the
provisioners run, so a failed provision keeps it, and loading a
machine whose provider no longer has it wipes it. On frosti,
vagrant's source is under `/opt/vagrant/embedded/gems/gems/` and
its plugins under `~/.vagrant.d/gems`.
