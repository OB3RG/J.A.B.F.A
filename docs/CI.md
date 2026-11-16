# Continuous Integration

The remote host identifies itself as Forgejo. `.forgejo/workflows/ci.yml` runs the required Rust quality gate on pushes to `main` and pull requests:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo test --doc
```

The job uses the `rust:1.100.0-bookworm` container and `actions/checkout@v4`, matching `rust-toolchain.toml`. Dependency downloads, action retrieval, and hosted execution are development-infrastructure network activity; JABFA's runtime retrieval remains local-only.

The workflow was pushed in `e69f78a`. A hosted Actions result has not been observed or verified, so no hosted pass is claimed. Check the remote's Actions result rather than inferring one from local tests.
