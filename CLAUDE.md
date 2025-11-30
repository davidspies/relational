# CLAUDE.md

This is a personal project. I don't mind resolving dependency conflicts when major versions of things change. So just use `"*"` for all dependency versions in Cargo.toml to stay up to date.

## Project Guidelines

### Code Style

- **File size limit**: All non-test Rust files should be ≤ 200 lines of code. Break large files into submodules.
- Test files (files named `tests.rs` or in a `tests/` directory) are exempt from this limit.
- **Imports**: One `super::` is fine, but avoid chains like `super::super::`. Use absolute paths like `crate::module::item` instead.
- **Module file naming**: Rust supports two styles: old-style (`a/mod.rs` + `a/b.rs`) and new-style (`a.rs` + `a/b.rs`). Use new-style until you need a submodule besides just tests, then switch to old-style.
- **Visibility**: Default to private. Use `pub(crate)` when needed within the crate, and `pub` only for items that are intentionally part of the public API. **Never** use `pub` as a default out of laziness - this is a serious code smell. If you're unsure whether something should be public, make it private first.
- **Option usage**: `Option` is for values that are conceptually optional. Don't use `Option` as a placeholder because you're unsure what to fill in. If a value is required, make the field non-optional and require it in constructors.

### Error Handling: Fail Fast, Don't Hide Bugs

**Never silently handle "impossible" cases.** If something shouldn't happen, panic.

Bad patterns that hide bugs:
- `.unwrap_or_default()` when None/Err indicates a bug
- `.min()` / `.max()` to clamp values that should already be in range
- Silent fallbacks for cases that "can't happen"
- Default values that mask logic errors
- `let _ = ...` or `if let Ok(_) = ...` to discard errors silently
- `match ... { Ok(x) => use(x), Err(_) => {} }` - either handle the error or propagate it

Good patterns:
- `.unwrap()` or `.expect("explanation")` for cases that indicate bugs
- `assert!()` for invariants
- Let it panic - a crash with a stack trace is infinitely better than silent corruption

The goal is to surface bugs immediately, not hide them behind fallbacks.

### Debug Scripts

Debug scripts should be placed in the workspace directory, not in `/tmp`. Operations outside the workspace require manual approval for each action, with no way to grant blanket approval.

### Search Commands

When using `find` or `grep` commands, always exclude the `target` directory:

```bash
# find example
find . -name "*.rs" -not -path "./target/*"

# grep example
grep -r "pattern" --exclude-dir=target .
```
