# Contributing to Ephemeris

Thank you for your interest in contributing to Ephemeris. This document covers the development setup, code standards, and process for submitting changes.

## Development Environment

### Prerequisites

- **Rust** -- install via [rustup](https://rustup.rs/). The project uses the stable toolchain (pinned in `rust-toolchain.toml`).
- **Docker** -- required for integration tests (testcontainers spins up PostgreSQL and MQTT broker instances automatically).
- **cargo-deny** -- install with `cargo install cargo-deny` for license auditing.

### Getting started

```bash
git clone https://github.com/saturnis-io/ephemeris.git
cd ephemeris

# Build
cargo build

# Run unit tests
cargo test --workspace --lib

# Run all tests (integration tests require Docker)
cargo test --workspace -- --test-threads=1
```

## Code Style

### Formatting

Run `cargo fmt` before every commit. CI will reject unformatted code.

### Linting

We enforce a zero-warnings policy:

```bash
cargo clippy -- -D warnings
```

For enterprise features:

```bash
cargo clippy --all-targets --features enterprise -- -D warnings
```

### Documentation

All public trait methods must have doc comments. Use `///` for item-level docs and `//!` for module-level docs.

### License audit

When adding a new dependency, run:

```bash
cargo deny check licenses
```

All dependencies in the default (non-enterprise) build must use permissive licenses (MIT, Apache-2.0, BSD, ISC, etc.). GPLv3, SSPL, BSL, and AGPL dependencies are not permitted in the default build path.

## Architecture Guidelines

- **`ephemeris-core` has zero database dependencies.** It contains only domain types, repository trait definitions, and validation logic. Never add database client crates (`tokio-postgres`, `sqlx`, `reqwest`, etc.) to `ephemeris-core`.
- **No embedded database engines.** Ephemeris is always a network client connecting to external databases over TCP/HTTP. Never add crates like `rusqlite`, `sled`, `rocksdb`, or similar.
- **Enterprise code is feature-gated.** All enterprise connectors must be behind Cargo feature flags and excluded from the default build.

## Testing

### Unit tests

Use `mockall` for trait mocking. Unit tests live alongside source code in `#[cfg(test)]` modules.

### Integration tests

Use `testcontainers-rs` for integration tests against real services. Integration tests should be run with `--test-threads=1` to avoid port conflicts.

```bash
cargo test --workspace -- --test-threads=1
```

Docker must be running for integration tests to pass.

## Pull Request Process

1. **Fork the repository** and create a feature branch from `main`.
2. **Make your changes** following the code style guidelines above.
3. **Add tests** for new functionality. Both unit and integration tests are expected for database-touching code.
4. **Run the full check suite** before opening a PR:
   ```bash
   cargo fmt --check
   cargo clippy -- -D warnings
   cargo test --workspace --lib
   cargo deny check licenses
   ```
5. **Open a pull request** with a clear description of the change and its motivation. Reference any related issues.
6. **Address review feedback.** PRs require at least one approving review before merge.

## Commit Messages

Use clear, descriptive commit messages. Prefix with a category when appropriate:

- `feat:` -- new functionality
- `fix:` -- bug fix
- `docs:` -- documentation only
- `refactor:` -- code change that neither fixes a bug nor adds a feature
- `test:` -- adding or updating tests
- `chore:` -- build, CI, or tooling changes

## License

By contributing to Ephemeris, you agree that your contributions will be licensed under the [Apache License 2.0](LICENSE).
