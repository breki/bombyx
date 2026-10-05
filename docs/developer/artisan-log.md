# Artisan Findings -- Deferred backlog

Quality (Artisan) review findings. Newest first.

---

### aq-2026-10-05-write-file-of-hidden-size-has-no-caller

**Category:** Dead code

`remote::write_file_of_hidden_size` lost its only production caller
when the git credential moved to `remote::write_secret_of_hidden_size`
on `fix/stage-secrets-only-when-provisioning`. It is still exported,
so no dead-code lint flags it, and `write_secret_of_hidden_size`'s
doc still explains itself by analogy to it. Remove it, or fold the
dry-run rule it documents into `write_secret_of_hidden_size`. A
public item, so the review's prose stage could not.

### aq-2026-10-05-load-machine-reads-as-a-boot

**Category:** Naming

`remote::load_machine` runs `vagrant status` to have vagrant load
the machine's record, but next to `up` the name reads as "start
the VM". Its doc now defines the vagrant term; a name such as
`refresh_machine_record` would not need the definition. Raised by
fresh-reader on `fix/stage-secrets-only-when-provisioning`, whose
prose stage cannot rename code.

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
