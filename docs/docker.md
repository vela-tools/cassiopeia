# Running Cassiopeia with Docker

Cassiopeia publishes a Docker image, so you can run the pipeline without installing Rust or system libraries on the host. This guide explains the image, its paths inside the container, and the volumes that keep the catalog, inputs, and outputs between runs.

For a quick check, use the one-line command in [Getting started](getting-started.md#run-with-docker). This page covers the rest.

## The image

The image is published to the GitHub Container Registry as `ghcr.io/vela-tools/cassiopeia`. It supports both `linux/amd64` and `linux/arm64`, so it runs natively on Intel, AMD, Apple silicon, and other ARM hosts.

The tags match the releases:

| Tag | Points at |
| --- | --- |
| `vX.Y.Z` | A specific release, for example `v1.0.0`. Pin to this for reproducible runs. |
| `latest` | The most recent stable release. Never a prerelease. |
| `vX.Y.Z-alpha.N`, `-beta.N`, `-rc.N` | A prerelease. These do not move `latest`. |

Pull the image:

```bash
docker pull ghcr.io/vela-tools/cassiopeia:latest
```

Unlike the prebuilt release binaries, the image is a **default build**. Its runtime layer links ecCodes, so it decodes both GRIB2 and GRIB1. The host does not need any additional system dependency for GRIB1. See [GRIB1 and ecCodes](getting-started.md#grib1-and-eccodes) for background.

## Verifying the image

Every published image is signed with [cosign](https://docs.sigstore.dev/) using keyless signing, so there is no public key to distribute. The signature carries a short-lived certificate that names the repository, the workflow, and the git ref the image was built from, and cosign records it in the public Sigstore transparency log. Install cosign, then verify a release:

```bash
cosign verify ghcr.io/vela-tools/cassiopeia:v1.0.0 \
    --certificate-identity-regexp '^https://github\.com/vela-tools/cassiopeia/\.github/workflows/docker\.yaml@refs/tags/v' \
    --certificate-oidc-issuer https://token.actions.githubusercontent.com
```

Both flags are necessary. Without them cosign confirms only that somebody signed the image; with them it confirms the signature came from this repository's `docker.yaml` workflow running on a release tag. Builds from other refs are signed too, but their certificate names a branch, so the expression above rejects them.

The signature sits on the multi-platform manifest list, which is what a tag resolves to, so one check covers `linux/amd64` and `linux/arm64`. Cosign signs each per-platform manifest inside the list as well, so a single architecture copied into another registry still verifies once the list is gone. To check the exact image you are about to run, pass a digest instead of a tag:

```bash
cosign verify ghcr.io/vela-tools/cassiopeia@sha256:... \
    --certificate-identity-regexp '^https://github\.com/vela-tools/cassiopeia/\.github/workflows/docker\.yaml@refs/tags/v' \
    --certificate-oidc-issuer https://token.actions.githubusercontent.com
```

## Running the container

The image's entrypoint is the `cassiopeia` binary. Docker passes everything after the image name to that command. Check that it runs:

```bash
docker run --rm ghcr.io/vela-tools/cassiopeia:latest --version
```

`--rm` removes the container when the command exits. Cassiopeia runs a batch job, not a service, so the examples below use `--rm`. Put anything you want to keep on a mounted volume; files inside the container disappear with it.

The same pattern works for every subcommand. To see the flags for `map`:

```bash
docker run --rm ghcr.io/vela-tools/cassiopeia:latest map --help
```

## How the container lays out its files

Inside a container, Cassiopeia switches from its per-user host directories to the system paths defined by the Filesystem Hierarchy Standard. These are the paths you mount volumes onto, and the image creates them in advance.

| What | Container path | Kind |
| --- | --- | --- |
| Configuration file | `/etc/cassiopeia/config.toml` | Auto-discovered on every run |
| Named mappings | `/etc/cassiopeia/mappings` | User-authored configuration |
| Smart Data Models catalog | `/var/lib/cassiopeia/schemas` | Downloaded application data |
| Log files | `/var/log/cassiopeia/logs` | Volatile run state |

Cassiopeia detects the container from the markers provided by Docker. If the runtime is not recognised, it falls back to the host-style layout under `/root`. You can also set these paths explicitly in configuration or through the environment, as described in [Configuration and environment](#configuration-and-environment).

Inputs and outputs are not fixed paths. Mount them wherever you like, then refer to them with `--input` and `--output` using paths inside the container. The examples below use `/data` for inputs and `/out` for outputs.

## Persisting the Smart Data Models catalog

`cassiopeia sdm download` fetches the schema catalog used by validation and stores it at `/var/lib/cassiopeia/schemas` inside the container. Because `--rm` removes that directory with the container, a catalog downloaded in one `docker run` is gone before the next run. Download it once into a volume instead.

Mount a volume at the schemas directory so the catalog persists. A named volume is the simplest option:

```bash
# Download the catalog once into a named volume.
docker run --rm \
    -v cassiopeia-schemas:/var/lib/cassiopeia/schemas \
    ghcr.io/vela-tools/cassiopeia:latest sdm download
```

Mount the same volume on later runs and the catalog will already be there. To inspect it:

```bash
docker run --rm \
    -v cassiopeia-schemas:/var/lib/cassiopeia/schemas \
    ghcr.io/vela-tools/cassiopeia:latest sdm list
```

Use a host directory instead if you want to share the catalog with a non-Docker install:

```bash
docker run --rm \
    -v "$HOME/.cassiopeia/schemas:/var/lib/cassiopeia/schemas" \
    ghcr.io/vela-tools/cassiopeia:latest sdm download
```

The catalog is optional. You do not need it when a run uses only a custom schema or skips validation. The terminal interfaces (`explorer`, `wizard`) and examples that use a published Smart Data Model do expect it to be present.

## A full run with volumes

A file run reads a source and a mapping, uses the catalog if validation is enabled, and writes entities to an output directory. Mount the source and mapping read-only at `/data`, mount a writable directory at `/out`, and mount the schemas volume:

```bash
docker run --rm \
    -v "$PWD:/data:ro" \
    -v "$PWD/out:/out" \
    -v cassiopeia-schemas:/var/lib/cassiopeia/schemas \
    ghcr.io/vela-tools/cassiopeia:latest \
    map \
      --input /data/stations.csv \
      --mapping /data/station.json5 \
      --output /out
```

The paths in this command are paths inside the container:

- `--input` and `--output` are **container** paths. `/data/stations.csv` is the source as the container sees it, not the host path. The file writer creates the output directory if it is missing, so an empty `/out` is fine.
- Mount inputs read-only (`:ro`) when the run only reads them. Cassiopeia never writes to its input.
- Cassiopeia writes one file per entity type to the output directory. On the host, those files appear in `./out`.

If a manifest refers to mappings by name instead of by path, also mount the mapping folder at `/etc/cassiopeia/mappings`. Cassiopeia resolves the names from that directory.

## Sending entities to a context broker

To send entities to a broker, the container must be able to reach it over the network. Remember that `localhost` inside the container refers to the container, not the host. Choose the address that matches your setup:

- **The broker is another container in the same Docker network.** Address it by its service name, for example `http://scorpio:9090/`.
- **The broker runs on the host** (Docker Desktop on macOS or Windows). Use `http://host.docker.internal:1026/`.
- **The broker runs on the host** (Docker Engine on Linux). Add `--network host` to the `docker run` command and address the broker as `http://localhost:1026/`, or use the host's LAN address.

```bash
docker run --rm \
    -v "$PWD:/data:ro" \
    -v cassiopeia-schemas:/var/lib/cassiopeia/schemas \
    ghcr.io/vela-tools/cassiopeia:latest \
    map \
      --input /data/readings.json \
      --mapping /data/reading.json5 \
      --writer context-broker \
      --broker-url http://host.docker.internal:1026/ \
      --header "Authorization: Bearer replace-me"
```

Pass broker credentials with `--header`, just as you would on the host. The [output guide](output.md#send-to-a-context-broker) covers broker operations and delivery options.

## Configuration and environment

A configuration file controls engine settings such as batch sizes, resolver stores, and logging. Cassiopeia looks for it at `/etc/cassiopeia/config.toml` inside the container, so mount your file there:

```bash
docker run --rm \
    -v "$PWD/config.toml:/etc/cassiopeia/config.toml:ro" \
    -v "$PWD:/data:ro" -v "$PWD/out:/out" \
    ghcr.io/vela-tools/cassiopeia:latest \
    map -i /data/stations.csv -m /data/station.json5 -o /out
```

Generate a starting file with `docker run --rm -v "$PWD:/out" ghcr.io/vela-tools/cassiopeia:latest config generate -o /out/config.toml`, edit it, and mount it as above.

Every configuration setting also has an environment variable. Build its name from `CASSIOPEIA_`, the section, `__` (two underscores), and the upper-case key. This works well in Compose, where environment variables live alongside the service definition. These settings are especially useful in a container because they change the default file locations:

| Variable | Overrides |
| --- | --- |
| `CASSIOPEIA_SCHEMAS__FOLDER` | Where the catalog is read and written |
| `CASSIOPEIA_MAPPINGS__FOLDER` | Where named mappings are resolved |
| `CASSIOPEIA_PIPELINE__BATCH_SIZE` | Records per batch |

To store the catalog at another path, point the variable at a mounted directory and mount the volume there:

```bash
docker run --rm \
    -e CASSIOPEIA_SCHEMAS__FOLDER=/catalog \
    -v cassiopeia-schemas:/catalog \
    ghcr.io/vela-tools/cassiopeia:latest sdm download
```

The [manifest guide](manifest.md) and [running guide](running.md) describe the settings. Only the way you provide them changes in a container.

## File ownership

The container runs as `root` by default, so files written to a mounted host directory belong to `root`. To use your own user, pass `--user "$(id -u):$(id -g)"`. Make sure that user can write to every output and schema directory. If the run downloads the catalog, point `CASSIOPEIA_SCHEMAS__FOLDER` at a writable directory because the image's `/var/lib/cassiopeia/schemas` directory belongs to `root`.

## A scheduled container

Commands that use a schedule flag (`--every`, `--cron`, or `--at`) keep running after the first cycle. Run them as a long-lived container: omit `--rm`, add a restart policy, and mount the volumes the schedule needs:

```bash
docker run -d --name cassiopeia-hourly \
    --restart unless-stopped \
    -v "$PWD:/data:ro" -v "$PWD/out:/out" \
    -v cassiopeia-schemas:/var/lib/cassiopeia/schemas \
    ghcr.io/vela-tools/cassiopeia:latest \
    map -i /data/stations.csv -m /data/station.json5 -o /out --every 1h
```

The [scheduling guide](scheduling.md) explains timezone handling for `--cron` and `--at`. To set the container's timezone, add `-e TZ=Europe/Ljubljana`.

## Next steps

- [Getting started](getting-started.md): install Cassiopeia and read about GRIB1.
- [Running Cassiopeia](running.md): the complete `map` reference, with or without Docker.
- [Output](output.md): file framing, representations, and broker delivery.
- [Manifests](manifest.md): package a complete run in a file you can mount and reuse.
