# CLAUDE.md

This is a personal project. I don't mind resolving dependency conflicts when major versions of things change. So just use `"*"` for all dependency versions in Cargo.toml to stay up to date.

## FAIL FAST. FAIL FAST. FAIL FAST.

**NEVER SILENTLY HANDLE "IMPOSSIBLE" CASES.**
**NEVER SILENTLY HANDLE "IMPOSSIBLE" CASES.**
**NEVER SILENTLY HANDLE "IMPOSSIBLE" CASES.**

If something shouldn't happen, PANIC. A crash with a stack trace is infinitely better than silent corruption.

### BAD patterns that hide bugs - DO NOT USE:

- `.unwrap_or_default()` when None/Err indicates a bug
- `.unwrap_or(some_value)` when None indicates a bug
- `.min()` / `.max()` to clamp values that should already be in range
- Silent fallbacks for cases that "can't happen"
- Default values that mask logic errors
- `let _ = ...` or `if let Ok(_) = ...` to discard errors silently
- `match ... { Ok(x) => use(x), Err(_) => {} }` - either handle the error or propagate it
- `if let Some(x) = map.get(&key)` when the key should always exist - use `map.get(&key).unwrap()` instead so missing keys panic rather than silently skip
- Silently enforcing invariants instead of asserting them - if something should be true, `assert!` it rather than making it true:
  - `.clear()` on a collection that should already be empty → `assert!(x.is_empty())`
  - `.first()` / `.next()` when expecting exactly one element → assert there's exactly one

### GOOD patterns - USE THESE:

- `.unwrap()` or `.expect("explanation")` for cases that indicate bugs
- `assert!()` for invariants
- Let it panic - surface bugs immediately

## Project Guidelines

### Code Style

- **File size limit**: All non-test Rust files should be ≤ 200 lines of code. Break large files into submodules.
- Test files (files named `tests.rs` or in a `tests/` directory) are exempt from this limit.
- **Imports**: One `super::` is fine, but avoid chains like `super::super::`. Use absolute paths like `crate::module::item` instead.
- **Module file naming**: Rust supports two styles: old-style (`a/mod.rs` + `a/b.rs`) and new-style (`a.rs` + `a/b.rs`). Use new-style until you need a submodule besides just tests, then switch to old-style.
- **Visibility**: Default to private. Use `pub(crate)` when needed within the crate, and `pub` only for items that are intentionally part of the public API. **Never** use `pub` as a default out of laziness - this is a serious code smell. If you're unsure whether something should be public, make it private first.
- **Option usage**: `Option` is for values that are conceptually optional. Don't use `Option` as a placeholder because you're unsure what to fill in. If a value is required, make the field non-optional and require it in constructors.
- **Bundle related data**: Never rely on parallel vectors or iterators being the same length. If data belongs together, put it in a struct. For example, instead of `Vec<Constraint>` and `Vec<ConstraintKind>`, use `Vec<TaggedConstraint>` where `TaggedConstraint` bundles both.

### Avoid Unnecessary Conditionals

**Every `if` statement must be justified.** Don't add special cases unless the specification requires them.

Bad patterns:
- Adding `if` branches "just in case" without a clear reason
- Special-casing edge cases that aren't in the spec (if the spec says "do X", don't add "unless Y")
- Defensive checks for conditions that "might" happen but aren't documented
- `if !has_foo { do_fallback }` when the code path should always have foo

Good patterns:
- `if` that directly implements a branch in the specification
- `if` for fundamentally different cases (e.g., choice rules vs disjunctive rules have different semantics)
- `if` with a comment explaining why this branch is necessary

When implementing an algorithm from a formalization, the code structure should mirror the formalization. If the formalization has no conditionals, the code shouldn't either. Add a comment to every `if` explaining which part of the spec requires it.

### Communication Style

When explaining something, use full sentences (not sentence fragments).

### Debugging: Understand Before Retrying

When something doesn't work, **figure out why** before trying a different approach.

Bad pattern:
- "That didn't work. Let me try a different approach."

Good pattern:
- "That didn't work. Let me figure out why."

Blindly trying alternatives wastes time and teaches nothing. Understand the failure first.

### Debug Scripts

Debug scripts should be placed in the workspace directory, not in `/tmp`. Operations outside the workspace require manual approval for each action, with no way to grant blanket approval.

### ASP Solver

When running gringo to ground ASP programs for our solver, always use `--output=smodels`:

```bash
gringo --output=smodels program.lp | target/release/asp
```

The parser only supports smodels format, not the default gringo output format.

### Environment Variables in Pipelines

When setting environment variables for a command in a pipeline, put the variable directly before the command that needs it, not at the start of the pipeline:

```bash
# WRONG - ASP_DEBUG is set for gringo, not for asp
ASP_DEBUG=1 gringo --output=smodels program.lp | target/release/asp

# RIGHT - ASP_DEBUG is set for asp
gringo --output=smodels program.lp | ASP_DEBUG=1 target/release/asp
```

Each command in a pipeline runs in its own process, so environment variables only apply to the command they directly precede.

### Exit Codes in Pipelines

Commands like `grep`, `head`, and `tail` do NOT forward the exit code of the upstream command. They return their own exit code (0 if they successfully processed input). To check if a command in a pipeline failed, either:
- Write output to a file and check the exit code separately
- Use `set -o pipefail` in bash to fail on any pipeline component failure
- Capture output to a variable and check `${PIPESTATUS[@]}`

```bash
# WRONG - will show exit code 0 even if asp panics
gringo ... | target/release/asp 2>&1 | tail -20
echo $?  # Always 0 if tail succeeded

# RIGHT - capture to file, check exit code separately
gringo ... | target/release/asp > /tmp/out.txt 2>&1
echo "Exit code: $?"
tail -20 /tmp/out.txt
```

### Search Commands

When using `find` or `grep` commands, always exclude the `target` directory:

```bash
# find example
find . -name "*.rs" -not -path "./target/*"

# grep example
grep -r "pattern" --exclude-dir=target .
```
