# YAML Sigil web playground

## See it live

> **[Live demo - canardleteer.github.io/yaml-sigil-web](https://canardleteer.github.io/yaml-sigil-web/)**

## About

In-browser YamlSigil v1alpha1 demo: **Sign**, **Verify**, **Compose**, **Decompose**, plus YAML parse checks.

It depends on [`canardleteer/yaml-sigil-rs` `feat/wasm`](https://github.com/canardleteer/yaml-sigil-rs/tree/feat/wasm). Keys are minted in the page session and are not stored.

This is a demo, not a key store. Browser WebAssembly does not provide the same side-channel guarantees as a hardened native cryptographic environment.

## Requirements

- Rust **stable** via `rust-toolchain.toml` (MSRV **1.98.0** in `Cargo.toml`) with `wasm32-unknown-unknown`, `rustfmt`, and `clippy`
- [Trunk](https://trunkrs.dev/) 0.21.x (`cargo install --locked trunk --version 0.21.14`)
- Network on the first build: `yaml-sigil-core` runs host `build.rs` (Buf via `buf-tools`, protobuf codegen via `buffa-build`)

```bash
rustup show
cargo install --locked trunk --version 0.21.14
```

## Checks

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

## Serve (live reload)

```bash
trunk serve
```

`Trunk.toml` binds `0.0.0.0:8080` on the host. `open = false` means Trunk will not auto-open a browser tab; it does not restrict private IPs.

Open [http://127.0.0.1:8080/](http://127.0.0.1:8080/) or `http://<host-lan-ip>:8080/` from another device on the network.

## Docker

The image is official **Rust 1.x on Debian Trixie** (`rust:1-trixie`), so a fresh pull tracks current stable. Override the base with `RUST_IMAGE`. The container listens on **8393** by default; set `PORT` to change both the listen port and the published host port.

```bash
docker compose up --build
# http://127.0.0.1:8393/

PORT=9000 docker compose up --build
# http://127.0.0.1:9000/

RUST_IMAGE=rust:1.98-trixie docker compose build
```

The Dockerfile has a `build` stage (`trunk build --release`) and a `runtime` stage that serves with Trunk.

## Static site

```bash
trunk build --release
```

Output is `dist/`. `public_url` is `./` so hashed assets work locally and on GitHub project pages.

The first uncached build clones `yaml-sigil-rs` and may download Buf. Later builds reuse Cargo’s git and registry caches.

## GitHub Pages

`.github/workflows/pages.yml` builds with Trunk and uploads `dist/`. Enable GitHub Pages (Actions source) on the repository to publish.

## Selectors

Form values are exactly `yaml` and `protobuf` (case-sensitive). The UI labels protobuf as **protobuf (base64)**. YAML decompose omits outer conformance. Protobuf decompose requires `strict` or `signature_strict`.

Algorithms:

- `ED25519_PUREEDDSA_RAW_RS64_CANONICAL` — 32-byte seed / 32-byte public key
- `ECDSA_SECP256R1_SHA256_RAW_RS64` — 32-byte scalar / SEC1 public point (compressed or uncompressed)

Keys may be hex (`0x` optional) or base64. The unsigned payload is always YAML text. Protobuf artifacts and protobuf signature carriers are standard base64; Compose and Decompose show them as typed `SignedYamlArtifact` / `YamlSigilSignature` fields.

Protobuf wire encoding uses [buffa](https://crates.io/crates/buffa) inside `yaml-sigil-core` (pure Rust, including `wasm32-unknown-unknown`).
