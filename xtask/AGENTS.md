# xtask Instructions for Agents

These instructions govern the standard development commands and any additional
tasks added to this crate. The crate is a root workspace member and
`.cargo/config.toml` maps `cargo xtask` to `cargo run --package xtask --`.

## Command Policy

- `check` registers `fmt`, `check`, `clippy`, `test`, and `wasm` in that order.
  It is fail-fast, runs all five by default, supports mutually exclusive
  `--only` and `--exclude`, and defaults feature-aware commands to all features.
  `ci` is a visible alias over exactly the same implementation. `wasm` is
  `cargo clippy --package yaml-sigil-web --target wasm32-unknown-unknown`.
- Keep CI/workflow declarations and scripts aligned with `check` when behavior
  changes. The xtask must remain provider-agnostic and must not inspect those
  declarations itself.
- `serve` preflights Trunk and runs `trunk serve`. Pass `--release` for a
  release serve; extra arguments after the flags are forwarded to Trunk.
- `build` preflights Trunk and runs `trunk build --release` by default. Pass
  `--debug` for an unoptimized site.
- `image` builds `Dockerfile` from the repository root as
  `yaml-sigil-web:local` (override with `--tag`). It tries Docker before
  Buildah in `auto` mode, smokes `trunk --version` and `/app/dist/index.html`
  without network access, always removes temporary containers, retains the
  local image, and never pushes it. There is no native Cargo pre-build; the
  image builds the WASM site itself.
- `coverage` supports `llvm-cov` and `tarpaulin` for host tests of
  `yaml-sigil-web`. Reports are `target/coverage/llvm-cov/html/index.html` and
  `target/coverage/tarpaulin/tarpaulin-report.html`. Both opening forms
  generate a fresh report first. Coverage excludes `xtask`, `src/app.rs`
  (DOM shell), and `src/lib.rs` (wasm-bindgen entry) and fails under 90%
  lines. In-browser WASM coverage is out of scope for these native engines.
- `profile` / `profile-open` are omitted: this crate is a WASM `cdylib`/`rlib`
  playground, and a Samply native-binary workload would be artificial.
- `mcp-test` is omitted: this repository does not expose a stdio MCP server.

## Tool Guidance

Probe only tools selected by the command. A failed launch is an unusable tool,
not an absent one. Recommend these exact Cargo installs when applicable:

```text
cargo install --locked cargo-llvm-cov
cargo install --locked cargo-tarpaulin
cargo install --locked trunk --version 0.21.14
```

Use `rustup component add rustfmt` or `rustup component add clippy` for missing
Rust components, and `rustup target add wasm32-unknown-unknown` for the WASM
target. Link to official Docker or Buildah installation instructions; do not
guess a platform package-manager command.

When a command actually needs async I/O or concurrency, `tokio` and `tracing`
are appropriate. Configure `tracing-subscriber` in `main`. Do not add them as
ceremony when the work is synchronous. Current commands are synchronous.

Prefer adding Cargo-aware cross-platform orchestration here over adding a
Python wrapper. This repository has no retained Python orchestration.

## Implementation and Validation

Represent subprocesses as a program plus OS argument vector. Keep `main.rs`
thin, use Clap derive types, and retain parser, selection, feature, alias,
command-construction, and failure-path tests.

Run from the repository root:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo xtask check --only fmt,check
```

Tool-dependent commands (image, coverage, serve, build) rely on specific local binaries
and should be exercised when their implementations change.
