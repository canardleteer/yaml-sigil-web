# YAML Sigil web playground

## See it live

> **[Live demo - canardleteer.github.io/yaml-sigil-web](https://canardleteer.github.io/yaml-sigil-web/)**

## About

In-browser YamlSigil v1alpha1 demo: sign, verify, compose, decompose, plus YAML parse checks.

It depends on [`yaml-sigil-wasm` 0.6.0-rc.1](https://crates.io/crates/yaml-sigil-wasm) from crates.io. Keys are minted in the page session and are not stored.

Browser WebAssembly does not provide the same side-channel guarantees as a hardened native cryptographic environment; do not use this playground as a key store.

## Requirements

- Rust **stable** via `rust-toolchain.toml` (MSRV **1.98.0** in `Cargo.toml`) with `wasm32-unknown-unknown`, `rustfmt`, and `clippy`
- [Trunk](https://trunkrs.dev/) 0.21.x (`cargo install --locked trunk --version 0.21.14`)
- Network on the first build: `yaml-sigil-core` runs host `build.rs` (Buf via `buf-tools`, protobuf codegen via `buffa-build`)
- WASM builds set `getrandom_backend="wasm_js"` via `.cargo/config.toml` (required by `getrandom` 0.3/0.4)

```bash
rustup show
cargo install --locked trunk --version 0.21.14
```

## Checks

```bash
cargo xtask check
```

`cargo xtask ci` is the same command. Host coverage of the library (excluding
the DOM shell in `src/app.rs` and the wasm-bindgen entry in `src/lib.rs`) is
`cargo xtask coverage` and fails under 90% lines.

## Serve (live reload)

```bash
cargo xtask serve
```

`Trunk.toml` binds `0.0.0.0:8080` on the host. `open = false` means Trunk will not auto-open a browser tab; it does not restrict private IPs.

Open [http://127.0.0.1:8080/](http://127.0.0.1:8080/) or `http://<host-lan-ip>:8080/` from another device on the network.

## Docker

The image is official **Rust 1.x on Debian Trixie** (`rust:1-trixie`), so a fresh pull tracks current stable. Override the base with `RUST_IMAGE`. The container listens on **8393** by default; set `PORT` to change both the listen port and the published host port.

```bash
cargo xtask image
docker compose up --build
# http://127.0.0.1:8393/

PORT=9000 docker compose up --build
# http://127.0.0.1:9000/

RUST_IMAGE=rust:1.98-trixie docker compose build
```

The Dockerfile has a `build` stage (`trunk build --release`) and a `runtime` stage that serves with Trunk.

## Static site

```bash
cargo xtask build
```

Output is `dist/`. `public_url` is `./` so hashed assets work locally and on GitHub project pages.

The first uncached build fetches dependencies from crates.io and may download Buf. Later builds reuse Cargo’s registry caches.

## GitHub Pages

`.github/workflows/pages.yml` runs `cargo xtask build` and uploads `dist/`. Enable GitHub Pages (Actions source) on the repository to publish.

## Selectors

Form values are exactly `yaml` and `protobuf` (case-sensitive). The UI labels protobuf as `protobuf (base64)`. YAML decompose omits outer conformance. Protobuf decompose requires `strict` or `signature_strict`. Changing the Form selector on Compose or Decompose transcodes a signed artifact between yaml and protobuf when the bytes are not already in the selected form. Verify shows a Convert button when the artifact is a signed envelope in the other form. A YAML QR code can be generated for YAML payloads and artifacts when the active document is valid yaml without unsaved edits, whereas protobuf keeps those actions disabled. Verify offers a QR code of the artifact box and of the authenticated payload box; the payload button stays disabled while that box is empty. YAML boxes are syntax-highlighted, including multi-document streams; protobuf stays plain.

Algorithms:

- `ED25519_PUREEDDSA_RAW_RS64_CANONICAL` — 32-byte seed / 32-byte public key
- `ECDSA_SECP256R1_SHA256_RAW_RS64` — 32-byte scalar / 65-byte uncompressed SEC1 public point (`0x04 || X || Y`)

Keys may be hex (`0x` optional) or base64. Each identity has an optional `keyid` hint (1 to 1024 bytes, no CR/LF); default roster entries (alice, bob, carol) start with their names. The unsigned payload is always YAML text. Protobuf artifacts and protobuf signature carriers are standard base64. Compose shows them as typed `SignedYamlArtifact` / `YamlSigilSignature` fields; Decompose keeps the artifact as base64 and shows the carrier as typed fields.

Protobuf wire encoding uses the `yaml-sigil-core` facade (Buffa stays private to that crate, including `wasm32-unknown-unknown`).
