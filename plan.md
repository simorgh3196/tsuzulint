1. **Remove `.expect("...")` in `crates/tsuzulint_core/src/fix.rs`**
   - In `DependencyGraph::topological_sort`, we have two places where `.expect("rule must exist in dependency graph")` is used:
     - `*in_degree.get_mut(*rule).expect("...") += 1;`
     - `*in_degree.get_mut(next_rule).expect("...") -= 1;`
   - These can panic if the logic has an error and a rule is missing.
   - We will replace them with `if let Some(count) = in_degree.get_mut(...) { *count += 1; }` (or `-= 1`), which avoids the panic risk entirely.
   - We will verify with `make test`, `make lint`, and `make fmt-check`.

2. **Pre-commit checks**
   - Run `make test`, `make lint`, `make fmt-check`.
   - Call `pre_commit_instructions` to ensure proper testing, verification, review, and reflection.

3. **Submit**
   - Commit and submit changes with a branch name like `ferris/remove-expect-fix` and a commit title following Ferris' format: "🦀 Ferris: [refactor/improvement] Remove panic risk in topological_sort".
