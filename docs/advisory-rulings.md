# Advisory rulings

`deny.toml` ignores four advisories because they don't affect the Cassiopeia image, and `.vex/cassiopeia.openvex.json` tells scanners the same thing as an [OpenVEX](https://openvex.dev/) document. This page explains how the two are kept in step and holds the proof behind each ruling. The proofs hold for the lockfile they were made against, which locks libsqlite3-sys 0.38.2, proc-macro-error 1.0.4, humantime 2.4.0 and chrono 0.4.45.

## How the files fit together

Each ignore in `deny.toml` names the advisory's id and a reason, and the reason is the impact statement the VEX document repeats, so it says why the image is not affected.

`.vex/rulings.toml` holds one table per ignored advisory, named by the id the ignore names:

- `aliases` lists the advisory's other identifiers, such as its CVE and GHSA ids, as the advisory gives them.
- `justification` is the OpenVEX justification that fits. `vulnerable_code_not_present` says the image holds none of the affected code, as for an advisory that reports an unmaintained crate. `component_not_present` says none of the crate's code is in the image, as for a crate only procedural macros use.
- `crates` names the crate versions the ruling is about as `<name>@<version>`, at the version `Cargo.lock` holds, such as `libsqlite3-sys@0.38.2`.

`.vex/cassiopeia.openvex.json` is generated from both. An advisory gets one statement under each of its identifiers, with the others as its aliases, because a scanner may apply only a statement named by the identifier it reports. Every statement names the image `ghcr.io/vela-tools/cassiopeia` in the two package URL forms scanners name a pulled image by, `pkg:oci/cassiopeia?repository_url=ghcr.io/vela-tools/cassiopeia` and `pkg:docker/vela-tools/cassiopeia?repository_url=ghcr.io`, each without a version so that it covers every digest, with the ruled crate versions as its subcomponents.

## Keeping them in step

After changing an ignore or a ruling, or after a dependency update moves a ruled crate, regenerate the document and commit it:

```bash
cargo run -p cassiopeia-vex-generator -- generate
```

The document's `version` rises and its `last_updated` moves only when a statement changes, so generating it again without a change leaves it as it is.

The `vex` workflow runs `cargo run -p cassiopeia-vex-generator -- check` on every pull request and every push to `main`. The check fails when the committed document differs from the one the rulings generate, when `deny.toml` ignores an advisory without a ruling or without a reason, when a ruling names an advisory `deny.toml` doesn't ignore, and when a ruling names a crate version `Cargo.lock` doesn't hold.

When the check reports a crate version the lockfile no longer holds, the ruling has to be proven again for the version it now holds. If the new version is no longer affected, remove the ignore and the ruling together.

## Using the document

Give the document to a scanner along with the image, for example `trivy image --vex .vex/cassiopeia.openvex.json ghcr.io/vela-tools/cassiopeia:latest` or `grype ghcr.io/vela-tools/cassiopeia:latest --vex .vex/cassiopeia.openvex.json`. The document covers the image only. The release binaries are built without default features, so they hold neither SQLite nor libsqlite3-sys, and `cargo audit bin` reads no VEX document.

## RUSTSEC-2026-0459: SQLite write-ahead logs

Also CVE-2026-119375 and GHSA-72w2-rjgg-q729. SQLite before 3.54.0 can return uninitialized page-cache memory when it opens a database together with a write-ahead log whose page size differs from the database's. libsqlite3-sys 0.6.0 to 0.38.2 are affected when they compile the SQLite they bundle, which they do with their `bundled` feature.

The image holds none of that code. The image build compiles Cassiopeia with `--no-default-features --features geopackage,grib1,grib2-full`, without `geopackage-bundled`, so libsqlite3-sys 0.38.2 is built without `bundled`: `cargo tree -p cassiopeia --no-default-features --features geopackage,grib1,grib2-full --target x86_64-unknown-linux-gnu -e features -i libsqlite3-sys --locked` lists only its `default`, `min_sqlite_version_3_34_1`, `pkg-config` and `vcpkg` features. The image build instead compiles SQLite 3.54.1, which carries the fix, from the official source archive it pins by checksum, and links it statically. The build fails unless `cassiopeia --version` shows `+geopackage` with `-geopackage-bundled` and `ldd` finds no `libsqlite3` beside the binary. The justification is `vulnerable_code_not_present`.

A build from source with the default features does compile the affected SQLite. Its GeoPackage ingestor refuses a GeoPackage with a write-ahead log beside it, as [Write-ahead logs](source-formats.md#write-ahead-logs) describes, which is what the flaw needs.

## RUSTSEC-2024-0370: proc-macro-error is unmaintained

The advisory reports that proc-macro-error is unmaintained and names no flaw in it. `cargo tree -p cassiopeia --no-default-features --features geopackage,grib1,grib2-full --target x86_64-unknown-linux-gnu -e normal,build -i proc-macro-error --locked` lists static-automata-macros 1.1.0, static-regular-grammar 2.0.2 and str-newtype-derive 2.0.0 as the only crates that depend on proc-macro-error 1.0.4, and all three are procedural macros. A procedural macro runs inside the compiler while Cassiopeia compiles, so neither it nor proc-macro-error is linked into the binary. The justification is `component_not_present`.

## RUSTSEC-2026-0542: humantime is unmaintained

The advisory reports that humantime is unmaintained, on the grounds that it wraps chrono, and names no flaw in it. humantime 2.4.0 is in the binary, through Cassiopeia's duration parsing, but no vulnerable code is. The justification is `vulnerable_code_not_present`.

## RUSTSEC-2026-0543: chrono is unmaintained

The advisory reports that chrono is unmaintained and names no flaw in it. chrono 0.4.45 is in the binary, but no vulnerable code is. The justification is `vulnerable_code_not_present`.
