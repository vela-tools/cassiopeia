FROM rust:1.99.1-trixie AS chef

RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake g++ git libclang-dev libopenjp2-7-dev \
    && rm -rf /var/lib/apt/lists/*
RUN cargo install cargo-chef --locked --version 0.1.78
WORKDIR /cassiopeia

FROM chef AS planner

COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder

ENV CARGO_INSTALL_ROOT=/usr/local
COPY --from=planner /cassiopeia/recipe.json recipe.json
RUN cargo chef cook --release --locked --recipe-path recipe.json
COPY . .
RUN cargo build --release --locked --bin cassiopeia

FROM debian:trixie-20261027-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libopenjp2-7 \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /etc/cassiopeia/mappings /var/lib/cassiopeia/schemas /var/log/cassiopeia/logs

COPY --from=builder /usr/local/lib/cassiopeia /usr/local/lib/cassiopeia
COPY --from=builder /cassiopeia/target/release/cassiopeia /usr/local/bin/cassiopeia

ENTRYPOINT ["/usr/local/bin/cassiopeia"]
