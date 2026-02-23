# Fix Format Validation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make `--format` CLI argument reject invalid values instead of silently falling back to text format.

**Architecture:** Replace string-based format argument with clap's `ValueEnum` derive macro to get compile-time validated enum. This provides automatic validation and helpful error messages for invalid values.

**Tech Stack:** Rust, clap 4.5 with `derive` and `cargo` features

**Issue:** #161

---

## Task 1: Add `cargo` feature to clap in workspace

**Files:**
- Modify: `Cargo.toml:65`

**Step 1: Update clap features**

Change line 65 in `Cargo.toml`:

```toml
# Before
clap = { version = "4.5", features = ["derive"] }

# After
clap = { version = "4.5", features = ["derive", "cargo"] }
```

**Step 2: Verify build**

Run: `cargo check -p tsuzulint`
Expected: No errors

**Step 3: Commit**

```bash
git add Cargo.toml
git commit -m "build: add cargo feature to clap for ValueEnum support"
```

---

## Task 2: Define OutputFormat enum in cli.rs

**Files:**
- Modify: `crates/tsuzulint_cli/src/cli.rs`

**Step 1: Add OutputFormat enum**

Add after the imports (around line 5):

```rust
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Clone, Debug, ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
    Sarif,
}
```

**Step 2: Update format argument in Lint command**

Change lines 36-38:

```rust
# Before
/// Output format (text, json, sarif)
#[arg(short, long, default_value = "text")]
format: String,

# After
/// Output format
#[arg(short, long, default_value = "text", value_enum)]
format: OutputFormat,
```

**Step 3: Verify compilation**

Run: `cargo check -p tsuzulint`
Expected: Compilation errors in other files (expected - we'll fix them next)

**Step 4: Commit**

```bash
git add crates/tsuzulint_cli/src/cli.rs
git commit -m "feat(cli): add OutputFormat enum with ValueEnum derive"
```

---

## Task 3: Update output/mod.rs to use OutputFormat enum

**Files:**
- Modify: `crates/tsuzulint_cli/src/output/mod.rs`

**Step 1: Update function signature and match**

Replace entire file content:

```rust
//! Output formatting module

mod json;
mod sarif;
mod text;

use miette::Result;
use tsuzulint_core::LintResult;

use crate::cli::OutputFormat;

pub fn output_results(results: &[LintResult], format: OutputFormat, timings: bool) -> Result<bool> {
    let has_errors = results.iter().any(|r| r.has_errors());

    match format {
        OutputFormat::Sarif => sarif::output_sarif(results)?,
        OutputFormat::Json => json::output_json(results)?,
        OutputFormat::Text => text::output_text(results, timings),
    }

    Ok(has_errors)
}
```

**Step 2: Verify compilation**

Run: `cargo check -p tsuzulint`
Expected: Compilation errors in commands/lint.rs (expected - we'll fix next)

**Step 3: Commit**

```bash
git add crates/tsuzulint_cli/src/output/mod.rs
git commit -m "refactor(output): use OutputFormat enum instead of string"
```

---

## Task 4: Update commands/lint.rs signature

**Files:**
- Modify: `crates/tsuzulint_cli/src/commands/lint.rs`

**Step 1: Update function signature**

Change line 11-18:

```rust
# Before
pub fn run_lint(
    cli: &Cli,
    patterns: &[String],
    format: &str,
    fix: bool,
    dry_run: bool,
    timings: bool,
    fail_on_resolve_error: bool,
) -> Result<bool> {

# After
pub fn run_lint(
    cli: &Cli,
    patterns: &[String],
    format: OutputFormat,
    fix: bool,
    dry_run: bool,
    timings: bool,
    fail_on_resolve_error: bool,
) -> Result<bool> {
```

**Step 2: Add import for OutputFormat**

Add after line 5:

```rust
use crate::cli::OutputFormat;
```

**Step 3: Verify compilation**

Run: `cargo check -p tsuzulint`
Expected: Compilation errors in main.rs (expected - we'll fix next)

**Step 4: Commit**

```bash
git add crates/tsuzulint_cli/src/commands/lint.rs
git commit -m "refactor(lint): update run_lint to accept OutputFormat enum"
```

---

## Task 5: Update main.rs caller

**Files:**
- Modify: `crates/tsuzulint_cli/src/main.rs`

**Step 1: Update function call**

No changes needed to main.rs since `format` is already passed by reference and `OutputFormat` implements `Copy` (via `Clone` + we should add `Copy`). But let's add `Copy` to the enum first.

Actually, looking at the code, `format` is passed as `format` (not `&format`), so we need to either:
1. Add `Copy` to `OutputFormat`
2. Clone it

Let's add `Copy` to keep it simple.

**Step 2: Add Copy to OutputFormat enum**

In `cli.rs`, change:

```rust
# Before
#[derive(Clone, Debug, ValueEnum)]
pub enum OutputFormat {

# After
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum OutputFormat {
```

**Step 3: Verify compilation**

Run: `cargo check -p tsuzulint`
Expected: Success

**Step 4: Commit**

```bash
git add crates/tsuzulint_cli/src/cli.rs
git commit -m "refactor(cli): add Copy trait to OutputFormat enum"
```

---

## Task 6: Write test for invalid format rejection

**Files:**
- Modify: `crates/tsuzulint_cli/tests/cli_commands.rs`

**Step 1: Add test in lint_command_formats module**

Add after line 159 (after `outputs_text_format_by_default` test):

```rust
#[test]
fn rejects_invalid_format() {
    let sample_md = fixtures_dir().join("sample.md");

    tsuzulint_cmd()
        .arg("lint")
        .arg(&sample_md)
        .arg("--format")
        .arg("invalid")
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value").or(predicate::str::contains("possible values")));
}

#[test]
fn rejects_html_format() {
    let sample_md = fixtures_dir().join("sample.md");

    tsuzulint_cmd()
        .arg("lint")
        .arg(&sample_md)
        .arg("--format")
        .arg("html")
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value").or(predicate::str::contains("possible values")));
}
```

**Step 2: Run tests to verify they pass**

Run: `cargo nextest run -p tsuzulint --test cli_commands`
Expected: All tests pass including new ones

**Step 3: Commit**

```bash
git add crates/tsuzulint_cli/tests/cli_commands.rs
git commit -m "test(cli): add tests for invalid format rejection"
```

---

## Task 7: Run full verification

**Step 1: Run lint**

Run: `make lint`
Expected: No errors

**Step 2: Run format check**

Run: `make fmt-check`
Expected: No errors

**Step 3: Run all tests**

Run: `make test`
Expected: All tests pass

**Step 4: Format markdown (if any .md files changed)**

Run: `make fmt-md`
Expected: No changes or formatted files

---

## Task 8: Self-review

**Step 1: Review all changes**

Run: `git diff main`
Review for:
- [ ] All format references use `OutputFormat` enum
- [ ] No `String` or `&str` for format argument anymore
- [ ] Tests cover valid and invalid format values
- [ ] No commented-out code
- [ ] No debug prints or tracing left in

**Step 2: Verify help output**

Run: `cargo run --bin tzlint -- lint --help`
Expected: Shows `--format <FORMAT>` with possible values listed

---

## Task 9: Create Pull Request

**Step 1: Push branch**

```bash
git push -u origin fix/format-validation
```

**Step 2: Create PR**

```bash
gh pr create --title "fix(cli): validate --format argument, reject invalid values" --body "$(cat <<'EOF'
## Summary

- Replace string-based `--format` argument with clap's `ValueEnum` enum
- Invalid format values (e.g., `--format html`, `--format jsn`) now produce clear error messages
- No more silent fallback to text format

## Changes

- Add `OutputFormat` enum with `ValueEnum` derive
- Update `output_results` to use enum instead of string match
- Add tests for invalid format rejection

## Test Plan

- [x] `make lint` passes
- [x] `make test` passes
- [x] `--format text/json/sarif` works
- [x] `--format invalid` produces error

Fixes #161
EOF
)"
```

---

## Execution Options

**1. Subagent-Driven (this session)** - I dispatch fresh subagent per task, review between tasks, fast iteration

**2. Parallel Session (separate)** - Open new session with executing-plans, batch execution with checkpoints

**Which approach?**
