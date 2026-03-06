1. **Understand**:
   - The CI is failing for Codecov patch/project coverage. The "26.66% of diff hit (target 90.10%)" indicates the new lines added in my fix are not sufficiently covered by tests.
   - Specifically, my new branch `if metadata.len() > MAX_WASM_SIZE { return Err(...) }` is completely untested.
   - The memory context says: "The project enforces a strict 90.01% overall Codecov project coverage threshold. When adding helper functions with branching logic (like Option parameters), ensure all branches (e.g., both Some and None) are explicitly covered by unit tests to prevent CI check suite failures."

2. **Implementation Plan**:
   - I need to add tests for the file size limits.
   - I will create a test that triggers the `MAX_WASM_SIZE` error. Since creating a 50MB file during testing might be slow/consume space, maybe I can use a smaller limit just for the test? No, `MAX_WASM_SIZE` is a constant `50 * 1024 * 1024`.
   - Creating a 50MB sparse file or an actual 50MB file is usually fast on modern OSes using `file.set_len(50 * 1024 * 1024 + 1)`.
   - I will add a test in `crates/tsuzulint_plugin/src/executor.rs` or `crates/tsuzulint_plugin/src/lib.rs` (if it has `mod tests`) that creates a temp file larger than `MAX_WASM_SIZE`, calls `RuleExecutor::load_file`, and asserts that it returns `PluginError::LoadError("WASM file '...' is too large...")`.
   - However, `RuleExecutor` is a trait, and testing its default `load_file` method might require a dummy implementation or using one of the existing implementations.
   - Testing `ExtismExecutor`'s `load_file` with a >50MB file should cover both cases if they use the same logic, but they are separate functions. I will add a test specifically for `ExtismExecutor::load_file` returning a too-large error.
