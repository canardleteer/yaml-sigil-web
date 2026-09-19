# Repository Instructions for Agents

## Development Tasks

- For changes under `xtask/`, also read and follow `xtask/AGENTS.md`.
- Use `cargo xtask check` as the canonical local quality command;
  `cargo xtask ci` is the same command. Keep CI/workflow declarations and other
  scripts aligned with its registered steps (`fmt`, `check`, `clippy`, `test`,
  `wasm`), but do not make the xtask inspect a CI provider declaration.
- Use `cargo xtask serve` and `cargo xtask build` for Trunk. Use
  `cargo xtask image` for the repository-owned Docker image, and
  `cargo xtask coverage` / `coverage-open` for host coverage of
  `yaml-sigil-web` (excludes `src/app.rs` and `src/lib.rs`; 90% line floor).
- `profile` / `profile-open` are omitted because there is no representative
  native binary to record with Samply. In-browser WASM profiling is out of
  scope for the xtask.
- Prefer `cargo xtask` over new Python scripts for typed, cross-platform,
  Cargo-aware development orchestration. Inspect overlapping Python scripts and
  propose a migration, but obtain user approval before replacing a mature
  script or changing callers. Retain Python where specialized libraries or
  data-processing strengths materially fit better. This repository has no
  retained Python orchestration. When an xtask command needs async I/O or
  concurrency, `tokio` and `tracing` are appropriate. Leave synchronous
  commands synchronous.

## Rust CLI conventions

- Use `clap` with derive syntax for command-line argument parsing.
- For every `clap`-based binary, add and retain a unit test that builds its
  root parser with `clap::CommandFactory` and calls `debug_assert()`.
- Put reusable CLI behavior in a public library API. Keep the binary entry
  point thin.
- Before every commit, run and require success from `cargo xtask check`.
