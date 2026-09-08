<p align="center">
  <img src="docs/assets/vela.png" alt="" width="88" height="88">
</p>

<h1 align="center">Cassiopeia</h1>

<p align="center">
  <strong>Turn data from common formats into <a href="https://ngsi-ld.org/">NGSI-LD</a> entities.</strong><br>
  Convert CSV, JSON, GeoJSON, KML/KMZ, XML, Shapefiles, and GRIB into validated, standards-compliant context data.
</p>

<p align="center">
  <a href="#quick-start">Quick start</a> ·
  <a href="docs/index.md">Documentation</a> ·
  <a href="#editions">Editions</a> ·
  <a href="#sovereignty">Sovereignty</a> ·
  <a href="#community">Community</a> ·
  <a href="#license">License</a>
</p>

<p align="center">
  <a href="https://github.com/vela-tools/cassiopeia/actions/workflows/test.yaml"><img alt="tests" src="https://github.com/vela-tools/cassiopeia/actions/workflows/test.yaml/badge.svg"></a>
  <a href="LICENSE.md"><img alt="license: EUPL-1.2" src="https://img.shields.io/badge/license-EUPL--1.2-blue"></a>
  <img alt="rust 1.96+" src="https://img.shields.io/badge/rust-1.96%2B-orange">
</p>

<p align="center">
  <sub>Part of <a href="https://github.com/vela-tools">Vela Tools</a> · Based on EU open standards 🇪🇺 · Built in Ljubljana, Slovenia 🇸🇮 by SenLab d.o.o.</sub>
</p>

---

## How it works

Give Cassiopeia a source such as a CSV, JSON file, GeoJSON layer, or XML feed, then describe how its data maps to an entity type. Cassiopeia produces standards-compliant NGSI-LD entities, then either writes them to files or sends them to a context broker. It supports CSV, JSON, GeoJSON, KML/KMZ, XML, ESRI Shapefiles, and GRIB.

NGSI-LD is the linked-data model used by many smart-city, IoT, and dataspace platforms across Europe. Cassiopeia prepares data for these systems, but does not store or serve context itself.

## Quick start

Download the archive for your platform from the [latest release](https://github.com/vela-tools/cassiopeia/releases/latest). Linux and macOS builds ship as `cassiopeia-<platform>.tar.xz` for `linux-x86_64`, `linux-aarch64`, and `macos-aarch64`; the Windows build ships as `cassiopeia-windows-x86_64.zip`. Unpack the archive, then run:

```bash
wget https://github.com/vela-tools/cassiopeia/releases/latest/download/cassiopeia-linux-x86_64.tar.xz
tar -xf cassiopeia-linux-x86_64.tar.xz
./cassiopeia --version
```

Every release also publishes a `.sha256` checksum beside each archive. Download it too and run `sha256sum -c cassiopeia-linux-x86_64.tar.xz.sha256` before unpacking.

Try a complete mapping with a real dataset. The example needs no context broker, schema catalog, or configuration beyond the mapping itself:

```bash
# 118 chemical elements as a JSON array, one object per element.
wget -O element.json https://raw.githubusercontent.com/andrejewski/periodic-table/master/data.json

# The mapping: which model to produce, what identifies an entity, which fields to carry across.
cat > element.json5 <<'MAPPING'
{
    version: "v4",
    dataModel: "ChemicalElement",
    identity: {
        // The symbol is unique in this dataset, so it is enough to identify the entity.
        entityName: "{{ symbol }}",
    },
    attributes: {
        name: {
            source: "{{ name }}",
        },
        // A direct copy stays text, so this transformation makes the atomic number a real number.
        atomicNumber: {
            source: "{{ atomicNumber }}",
            transformation: "integer",
        },
        // Source and target names need not match: this reads groupBlock and writes group.
        group: {
            source: "{{ groupBlock }}",
        },
        standardState: {
            source: "{{ standardState }}",
        },
    },
}
MAPPING

./cassiopeia map --input element.json --mapping element.json5 --output out --context none
```

Cassiopeia detects the input format from the file, so `--type` is not needed. It writes one file per entity type and prints a summary of the run. `out/ChemicalElement.json` now holds 118 NGSI-LD entities:

```json
{
    "id": "urn:ngsi-ld:ChemicalElement:H",
    "type": "ChemicalElement",
    "name": {
        "type": "Property",
        "value": "Hydrogen"
    },
    "atomicNumber": {
        "type": "Property",
        "value": 1
    },
    "group": {
        "type": "Property",
        "value": "nonmetal"
    },
    "standardState": {
        "type": "Property",
        "value": "gas"
    }
}
```

The command also warns that the entities were not validated. That is expected here: `--context none` skips the JSON-LD context, and no Smart Data Model or custom data model schema was given. This keeps the first run entirely offline. [Validation](docs/validation.md) and [Output](docs/output.md) explain how to turn both on.

The [worked examples](docs/examples/index.md) continue with 31 datasets. Each adds one idea, such as composite identities, geometry, conditionals, relationships, synthetic entities, or scheduling. Every mapping is in this repository, so you can run the examples in place:

```bash
git clone https://github.com/vela-tools/cassiopeia.git
cd cassiopeia/docs/examples/01-json-field-mapping
```

The prebuilt binaries do not include ecCodes. They decode GRIB2 with the bundled pure-Rust reader, but they do not support GRIB1. To build from source with GRIB1 support, see [Getting started](docs/getting-started.md).

## Why?

Sensor vendors, city departments, and other data feeds often use different formats. Without a transformation layer, each new source needs custom integration code, and that code can break when a vendor changes its feed. Cassiopeia replaces those one-off integrations with a single transformation description. You describe the output you want, and Cassiopeia runs the mapping the same way each time.

The transformation is described as data. Its sources, validation rules, output destination, and schedule live in files with sensible defaults. The same manifest can run unchanged on your laptop, in a container, or on a schedule.

## What it does

Mappings can build entity attributes with templates, conditional logic, nested objects, and relationships. They can also create synthetic entities when a source carries only a partial reference to them. Cassiopeia can emit the result in any NGSI-LD representation (normalized, concise, or simplified), then validate it against Smart Data Model schemas before it leaves the pipeline. This catches bad data before it reaches your platform. Finished entities are written to files or sent directly to a context broker over HTTP.

Cassiopeia splits the work into small stages. These stages stream data, apply back pressure, and run in parallel. This means large inputs do not need to fit in memory. Cassiopeia detects formats from their content, so most inputs need no configuration. The terminal also includes tools for browsing schemas and authoring mappings interactively.

## Documentation

The [documentation](docs/index.md) covers installation and running Cassiopeia. More product information is available at [velacontext.com/cassiopeia-ngsi-ld-mapper](https://velacontext.com/cassiopeia-ngsi-ld-mapper).

## Editions

Cassiopeia is open core. This repository contains the FOSS edition, which is complete on its own: it maps, validates, and delivers NGSI-LD without a licence key, account, or call-home check. Pro and Hub add operational tooling around the same engine rather than unlocking parts of it.

Everything listed under FOSS remains in that edition permanently. Features released here will not later move to Pro or Hub, and they will not be withdrawn to create an upgrade path. This boundary is permanent; it does not change with a new release.

### FOSS

The FOSS edition includes the transformation engine and everything needed to run it in production. It reads CSV, JSON, GeoJSON, KML/KMZ, XML, ESRI Shapefiles, and GRIB, detecting the format from the content. Mappings build entity attributes with templates, conditional logic, nested objects, relationships, and synthetic entities. The result can be emitted in any NGSI-LD representation and validated against Smart Data Model schemas before it leaves the pipeline. Entities go to files or straight to a context broker over HTTP.

You can repeat runs without extra machinery: a manifest or command-line flag turns a one-off transformation into a poller on a fixed interval, a cron expression, or fixed times of day. The process stays in the foreground and keeps running until you stop it. That works for a container, a systemd unit, or a cron entry that owns the process lifecycle.

The FOSS edition also includes two terminal interfaces. The Explorer browses a Smart Data Model schema, and the Wizard walks through authoring a mapping against a chosen model. You can start a first mapping without creating an empty file by hand.

### Pro

Pro is for running many transformations as managed infrastructure instead of individual commands. You self-host it on your own machines, the same as the FOSS edition.

Server mode runs Cassiopeia as a long-lived background service. It manages multiple jobs, schedules them, and exposes an HTTP API to submit, inspect, and control runs. The FOSS scheduler keeps one manifest alive in the foreground; server mode manages a fleet of manifests and tracks their state.

A web-based mapping configurator covers the same ground as the terminal Wizard in a browser. This is useful when the person writing the mapping does not have shell access to the server.

Pro also adds transit and mobility feeds, including GTFS, GTFS-RT, and GBFS, along with Excel workbooks. For Excel, it locates the tables inside a sheet automatically, so most workbooks map without being rearranged first.

### Hub

Hub delivers Cassiopeia as part of [Vela Context Data Hub](https://velacontext.com). It is fully managed, integrated with the rest of the Vela suite, and includes support and consulting. You do not self-host it.

For Pro and Hub, see [velacontext.com](https://velacontext.com).

## Sovereignty

The output format determines how easily you can move your data later. Cassiopeia writes ETSI NGSI-LD, an EU open standard, rather than a vendor-specific format. The mappings are plain JSON5 files in your own repository, and the output goes to files or to the NGSI-LD context broker you choose, including Scorpio, Orion-LD, and Stellio. The FOSS edition runs on your own machines under the EUPL-1.2, without an account, licence key, or telemetry.

SenLab's full position on ownership, export, and reversibility is at [velacontext.com/sovereignty](https://velacontext.com/sovereignty).

## Contributing

Cassiopeia is not accepting external source-code contributions right now. Bug reports, feature requests, and documentation feedback are welcome. A contribution policy will follow. See [CONTRIBUTING.md](CONTRIBUTING.md).

Never report a security vulnerability through a public issue. Follow the [Cassiopeia security policy](https://github.com/vela-tools/cassiopeia/security/policy) instead.

## Community

- Issues: [GitHub Issues](../../issues)
- Discussions: [GitHub Discussions](../../discussions)
- Newsletter: [velacontext.com/newsletter](https://velacontext.com/newsletter)
- General contact: info@velacontext.com
- Security: security@velacontext.com, see the [security policy](https://github.com/vela-tools/cassiopeia/security/policy)
- Code of conduct: [Contributor Covenant](https://github.com/vela-tools/cassiopeia?tab=coc-ov-file)

## License

Cassiopeia is licensed under the [European Union Public Licence v. 1.2](LICENSE.md) (EUPL-1.2). The full licence text in all EU languages and additional information are available at [eupl.eu](https://eupl.eu/).

## About

Cassiopeia is part of Vela Tools, an open-core infrastructure project for the NGSI-LD ecosystem. SenLab d.o.o. builds it in Ljubljana, Slovenia. Learn more at [velacontext.com](https://velacontext.com).
