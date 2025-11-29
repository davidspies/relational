# CLAUDE.md

## Project Guidelines

### Code Style

- **File size limit**: All non-test Rust files should be ≤ 200 lines of code. Break large files into submodules.
- Test files (files named `tests.rs` or in a `tests/` directory) are exempt from this limit.

### Search Commands

When using `find` or `grep` commands, always exclude the `target` directory:

```bash
# find example
find . -name "*.rs" -not -path "./target/*"

# grep example
grep -r "pattern" --exclude-dir=target .
```
