# syntax=docker/dockerfile:1

# Official Rust 1.x on Debian Trixie (tracks current stable). Override with e.g.
#   --build-arg RUST_IMAGE=rust:1.98-trixie
ARG RUST_IMAGE=rust:1-trixie

FROM ${RUST_IMAGE} AS build
ARG TRUNK_VERSION=0.21.14
WORKDIR /app

ENV CARGO_TERM_COLOR=never \
    NO_COLOR=true

RUN rustup target add wasm32-unknown-unknown \
    && cargo install --locked trunk --version "${TRUNK_VERSION}"

COPY . .
RUN trunk build --release

FROM ${RUST_IMAGE} AS runtime
ARG PORT=8393
WORKDIR /app

ENV CARGO_TERM_COLOR=never \
    NO_COLOR=true \
    PORT=${PORT} \
    TRUNK_SERVE_ADDRESS=0.0.0.0 \
    TRUNK_SERVE_PORT=${PORT}

RUN rustup target add wasm32-unknown-unknown

COPY --from=build /usr/local/cargo/bin/trunk /usr/local/cargo/bin/trunk
COPY --from=build /app /app
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh

EXPOSE ${PORT}

ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
