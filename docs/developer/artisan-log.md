# Artisan Findings -- Deferred backlog

Quality (Artisan) review findings. Newest first.

---

### aq-2026-10-03-windows-env-refusal-is-a-string

**Category:** Type Safety

`ConfigError::WindowsGuestEnv` carries its reason as a
`&'static str`, built in `refuse_windows_mismatch` in
`config/registry.rs`, while the other Windows refusals carry an
enum: `WindowsUserRefusal`, `WindowsHookRefusal` and
`WindowsScriptRefusal`. A `WindowsEnvRefusal` enum would let a
caller match on the reason. Raised as AQ-4 in the #175 review,
outside that diff.

### aq-2026-10-02-hand-written-parse

**Category:** Duplication

`EnvFilePath::parse` in `config/env_file.rs` and
`DeployKeyPath::parse` in `config/deploy_key.rs` hand-write the
body `checked_str_parse!` in `newtype.rs` generates. In tests,
`config.rs`'s `registry_file_in_a_dir` duplicates
`config/registry.rs`'s `registry_file`. Raised by fresh-reader
outside its lane during the artisan-backlog review, after the
artisan stage had run.
