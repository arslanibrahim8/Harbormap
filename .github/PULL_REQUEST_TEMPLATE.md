## Summary

Briefly describe what this pull request changes and why.

## Related Issues

Closes #

## Verification Checklist

- [ ] `cargo fmt --all --check` passes
- [ ] `cargo clippy --locked --workspace --all-targets -- -D warnings` passes
- [ ] `cargo test --locked --workspace` passes
- [ ] If `ui/learn/` was modified: rebuilt `ui/learn/dist/` (`npm run build`) and verified `npm run check && npm test && npm run verify`
- [ ] Added or updated tests covering the change
- [ ] Updated docs (if user-facing behavior or CLI flags changed)
