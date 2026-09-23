FROM rust:1.98.1-trixie AS chef

RUN apt-get update \
    && apt-get install -y --no-install-recommends libclang-dev libeccodes-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*
RUN cargo install cargo-chef --locked --version 0.1.78
WORKDIR /cassiopeia

FROM chef AS planner

COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder

COPY --from=planner /cassiopeia/recipe.json recipe.json
RUN cargo chef cook --release --locked --recipe-path recipe.json
COPY . .
RUN cargo build --release --locked --bin cassiopeia

FROM debian:trixie-20260824-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libeccodes0 \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /etc/cassiopeia/mappings /var/lib/cassiopeia/schemas /var/log/cassiopeia/logs

COPY --from=builder /cassiopeia/target/release/cassiopeia /usr/local/bin/cassiopeia

ENTRYPOINT ["/usr/local/bin/cassiopeia"]
