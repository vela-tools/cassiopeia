FROM rust:1.99.1-trixie AS chef

RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake g++ git libclang-dev libopenjp2-7-dev \
    && rm -rf /var/lib/apt/lists/*
RUN cargo install cargo-chef --locked --version 0.1.78
WORKDIR /cassiopeia

FROM chef AS planner

COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM rust:1.99.1-trixie AS sqlite

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
    && mkdir -p /opt/sqlite/include /opt/sqlite/lib \
    && ar rcs /opt/sqlite/lib/libsqlite3.a sqlite3.o \
    && cp sqlite3.h sqlite3ext.h /opt/sqlite/include/

FROM chef AS builder

ENV CARGO_INSTALL_ROOT=/usr/local \
    SQLITE3_INCLUDE_DIR=/opt/sqlite/include \
    SQLITE3_LIB_DIR=/opt/sqlite/lib \
    SQLITE3_NO_PKG_CONFIG=1 \
    SQLITE3_STATIC=1
COPY --from=sqlite /opt/sqlite /opt/sqlite
COPY --from=planner /cassiopeia/recipe.json recipe.json
RUN cargo chef cook --release --locked --no-default-features --features geopackage,grib1,grib2-full --recipe-path recipe.json
COPY . .
RUN cargo build --release --locked --no-default-features --features geopackage,grib1,grib2-full --bin cassiopeia
RUN features="$(target/release/cassiopeia --version | grep '^features ')" \
    && echo "$features" \
    && case " $features " in *" +geopackage "*" -geopackage-bundled "*" +grib1 "*" +grib2-full "*) ;; *) exit 1 ;; esac \
    && ! ldd target/release/cassiopeia | grep -F libsqlite3

FROM chef AS rootfs

RUN mkdir -p /rootfs/etc/cassiopeia/mappings /rootfs/var/lib/cassiopeia/schemas /rootfs/var/log/cassiopeia/logs /rootfs/var/lib/dpkg/status.d \
    && dpkg -L libopenjp2-7 | grep -E '\.so|/copyright$' | xargs cp -a --parents -t /rootfs \
    && dpkg-query -s libopenjp2-7 > /rootfs/var/lib/dpkg/status.d/libopenjp2-7

FROM gcr.io/distroless/cc-debian13:latest@sha256:e1f5543d45a2cd7c02a5b43dda5f0ff30729b4832113759d01ed39b0b152cddb AS runtime

COPY --from=rootfs /rootfs/ /
COPY --from=builder /usr/local/lib/cassiopeia /usr/local/lib/cassiopeia
COPY --from=builder /cassiopeia/target/release/cassiopeia /usr/local/bin/cassiopeia

ENTRYPOINT ["/usr/local/bin/cassiopeia"]
