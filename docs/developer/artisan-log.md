# Artisan Findings -- Deferred backlog

Quality (Artisan) review findings. Newest first.

---

### aq-2026-10-02-hand-written-parse

**Category:** Duplication

`EnvFilePath::parse` in `config/env_file.rs` and
`DeployKeyPath::parse` in `config/deploy_key.rs` hand-write the
body `checked_str_parse!` in `newtype.rs` generates. In tests,
`config.rs`'s `registry_file_in_a_dir` duplicates
`config/registry.rs`'s `registry_file`. Raised by fresh-reader
outside its lane during the artisan-backlog review, after the
artisan stage had run.
