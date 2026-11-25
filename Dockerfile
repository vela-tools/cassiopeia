FROM rust:1.100.0-trixie@sha256:e84068000361bf3d4a5ba2b7647f62893528b1eb9f847d7a23b493f5ce8ea254 AS toolchain

FROM toolchain AS chef

RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake g++ git libclang-dev libopenjp2-7-dev \
    && rm -rf /var/lib/apt/lists/*
RUN set -eu; \
    arch="$(uname -m)"; \
    case "$arch" in \
        x86_64) about=3ad1e94ea1e63d69d1961f4fb15ccbbe9575d906111f96d2cb4dce6df48f71b4; audit=b77993532b253fcbf550996d7716465ec3d734d4e74e7f87bfb65a19add0fb59; auditable=7676443367c5e33d9fb22acef63cccb5e9202de12dae1b0579995eb3e2bc7a18; chef=aca691abfbfbbe00d482e0ed2249eec3091b65e96a0ce92947fb5b254d48b16d ;; \
        aarch64) about=eb4d9ff6ebd2518152a8308729431a04b3836e71d2bd5ead7aea69f91a71162e; audit=00eff3158429b9629abe8748e971f29f99ee53d4cff54a25072cbb15167bdda8; auditable=c324af56990601354bbf40080f582ba46f8ec8b807a1388e25bb7e429d285c18; chef=cd59b90fce5fa84c4189648fec33404ec5491791eebdb812df5e7fca8336408d ;; \
        *) echo "no pinned cargo tools for $arch" >&2; exit 1 ;; \
    esac; \
    fetch() { \
        curl -fsSLo /tmp/tool "$1"; \
        echo "$2  /tmp/tool" | sha256sum -c -; \
        tar -xf /tmp/tool -C /usr/local/bin --no-same-owner --strip-components=1 --wildcards "*/$3"; \
        rm /tmp/tool; \
    }; \
    fetch "https://github.com/EmbarkStudios/cargo-about/releases/download/0.9.3/cargo-about-0.9.3-${arch}-unknown-linux-musl.tar.gz" "$about" cargo-about; \
    fetch "https://github.com/rustsec/rustsec/releases/download/cargo-audit/v0.23.0/cargo-audit-${arch}-unknown-linux-gnu-v0.23.0.tgz" "$audit" cargo-audit; \
    fetch "https://github.com/rust-secure-code/cargo-auditable/releases/download/v0.7.7/cargo-auditable-${arch}-unknown-linux-musl.tar.xz" "$auditable" cargo-auditable; \
    fetch "https://github.com/LukeMathWalker/cargo-chef/releases/download/v0.1.78/cargo-chef-${arch}-unknown-linux-musl.tar.xz" "$chef" cargo-chef
WORKDIR /cassiopeia

FROM chef AS planner

COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM toolchain AS sqlite

ARG SQLITE_VERSION=3540100
ARG SQLITE_SHA256=926ef8d3eab93fbf6e06011901273eec076b25594a4e1cdadd50c987f4f6d64e
WORKDIR /sqlite
RUN curl -fsSLo sqlite.tar.gz "https://sqlite.org/2026/sqlite-autoconf-${SQLITE_VERSION}.tar.gz" \
    && echo "${SQLITE_SHA256}  sqlite.tar.gz" | sha256sum -c - \
    && tar -xzf sqlite.tar.gz --strip-components=1 \
    && cc -O2 -fPIC \
        -DSQLITE_THREADSAFE=1 \
        -DSQLITE_DEFAULT_FOREIGN_KEYS=1 \
        -DSQLITE_ENABLE_API_ARMOR \
        -DSQLITE_ENABLE_COLUMN_METADATA \
        -DSQLITE_ENABLE_MEMORY_MANAGEMENT \
        -DSQLITE_ENABLE_RTREE \
        -DSQLITE_ENABLE_STAT4 \
        -DSQLITE_OMIT_LOAD_EXTENSION \
        -DHAVE_USLEEP=1 \
        -DHAVE_ISNAN \
        -c sqlite3.c -o sqlite3.o \
    && mkdir -p /opt/sqlite/include /opt/sqlite/lib /opt/sqlite/share/doc/sqlite \
    && ar rcs /opt/sqlite/lib/libsqlite3.a sqlite3.o \
    && cp sqlite3.h sqlite3ext.h /opt/sqlite/include/ \
    && sed -n '/^\*\{5,\}/q;s/^\*\* \{0,1\}//p' sqlite3.h > /opt/sqlite/share/doc/sqlite/copyright \
    && grep -q 'disclaims copyright' /opt/sqlite/share/doc/sqlite/copyright

FROM chef AS builder

ENV CARGO_INSTALL_ROOT=/usr/local \
    SQLITE3_INCLUDE_DIR=/opt/sqlite/include \
    SQLITE3_LIB_DIR=/opt/sqlite/lib \
    SQLITE3_NO_PKG_CONFIG=1 \
    SQLITE3_STATIC=1
COPY --from=sqlite /opt/sqlite /opt/sqlite
COPY --from=planner /cassiopeia/recipe.json recipe.json
RUN cargo chef cook --release --locked --no-default-features --features geopackage,grib1,grib2-full --recipe-path recipe.json
RUN cargo fetch --locked
COPY . .
RUN cargo auditable build --release --locked --no-default-features --features geopackage,grib1,grib2-full --bin cassiopeia
RUN features="$(target/release/cassiopeia --version | grep '^features ')" \
    && echo "$features" \
    && case " $features " in *" +geopackage "*" -geopackage-bundled "*" +grib1 "*" +grib2-full "*) ;; *) exit 1 ;; esac \
    && ! ldd target/release/cassiopeia | grep -F libsqlite3
RUN cargo audit --ignore RUSTSEC-2026-0459 bin target/release/cassiopeia
RUN doc=/usr/share/doc/cassiopeia \
    && mkdir -p "$doc" \
    && third-party-licenses/generate.sh "$(uname -m)-unknown-linux-gnu" "$doc/third-party-licenses.md" --no-default-features --features geopackage,grib1,grib2-full \
    && install -m 0644 -t "$doc" LICENSE.md NOTICE.md \
    && install -D -m 0644 -t "$doc/eccodes" target/release/build/eccodes-sys/*/out/src/eccodes/LICENSE target/release/build/eccodes-sys/*/out/src/eccodes/NOTICE \
    && install -D -m 0644 -t "$doc/eckit" target/release/build/eckit-sys/*/out/src/eckit/LICENSE target/release/build/eckit-sys/*/out/src/eckit/NOTICE \
    && install -D -m 0644 -t "$doc/libaec" target/release/build/eccodes-sys/*/out/src/libaec/LICENSE.txt \
    && install -D -m 0644 -t "$doc/sqlite" /opt/sqlite/share/doc/sqlite/copyright

FROM chef AS rootfs

RUN mkdir -p /rootfs/etc/cassiopeia/mappings /rootfs/var/lib/cassiopeia/schemas /rootfs/var/lib/cassiopeia/cursors /rootfs/var/log/cassiopeia/logs /rootfs/var/lib/dpkg/status.d \
    && dpkg -L libopenjp2-7 | grep -E '\.so|/copyright$' | xargs cp -a --parents -t /rootfs \
    && dpkg-query -s libopenjp2-7 > /rootfs/var/lib/dpkg/status.d/libopenjp2-7

FROM gcr.io/distroless/cc-debian13:latest@sha256:dac90a25ffe03ebaace90cda886b081e61ab6cd77ac47eda51f7bcc15c320765 AS runtime

COPY --from=rootfs /rootfs/ /
COPY --from=builder /usr/local/lib/cassiopeia /usr/local/lib/cassiopeia
COPY --from=builder /cassiopeia/target/release/cassiopeia /usr/local/bin/cassiopeia
COPY --from=builder /usr/share/doc/cassiopeia /usr/share/doc/cassiopeia

LABEL org.opencontainers.image.title="Cassiopeia" \
      org.opencontainers.image.description="Adaptive pipeline that maps any source to validated and standards-compliant NGSI-LD entities." \
      org.opencontainers.image.url="https://vela-tools.github.io/cassiopeia/" \
      org.opencontainers.image.documentation="https://vela-tools.github.io/cassiopeia/docs/reference/docker" \
      org.opencontainers.image.source="https://github.com/vela-tools/cassiopeia" \
      org.opencontainers.image.licenses="EUPL-1.2"

ENTRYPOINT ["/usr/local/bin/cassiopeia"]
