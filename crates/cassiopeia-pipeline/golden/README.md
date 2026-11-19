# Golden corpus

The golden corpus records everything a mapping writes: every entity, in every representation, and every diagnostic the run reports. The test `end_to_end::golden::every_golden_case_writes_and_reports_what_its_snapshots_record` in `src/end_to_end/golden/` runs each case through the whole pipeline and checks what it wrote against the snapshots stored beside it. A change that alters what any mapping writes fails it, and a change meant to alter one output shows, in its regenerated snapshots, everything it altered.

## Cases

Each case is a directory `<group>/<case>/`, such as `values/json-property/`. It holds a `manifest.json5` that declares no `output`, the mappings and inputs that manifest names by relative path, and the snapshots of what the case writes and reports. The groups follow the mapping constructs: `identity`, `scope`, `expiry`, `values`, `qualifiers`, `structure`, `geometry`, `relationships`, `ranking`, `merging`, `collections` and `keys-without-effect`, and `sensorthings` holds the mappings that read SensorThings snapshots. Each mapping in a case directory opens with a comment saying what the case shows. The `sensorthings` cases read the mappings and snapshots in `tests/fixtures/sensorthings/` by relative path, so they run the same files as the SensorThings end-to-end tests, and keep beside them only the snapshots and mappings no fixture holds.

The corpus holds every mapping the end-to-end tests in `src/end_to_end/` run, with the records those tests feed it. A new end-to-end test that runs a mapping no case holds adds a case for it.

The test writes each case once per view: the current state and, when any of its mappings declares an `observedAt`, the series, each in the normalized, concise and simplified representations. A view writes its files with both entity stores, which have to write the same entities and report the same diagnostics, and its records are extracted on one thread, so every run writes the same files. The three representations of a history have to report the same diagnostics.

| Snapshot | Content |
| --- | --- |
| `current-normalized.snap`, `current-concise.snap`, `current-simplified.snap` | Every entity the current-state run wrote, file by file in the order of their names. |
| `series-normalized.snap`, `series-concise.snap`, `series-simplified.snap` | Every entity the series run wrote, for a case whose mappings declare an `observedAt`. |
| `current-diagnostics.snap`, `series-diagnostics.snap` | The failure that ended the run, if it failed, and every diagnostic it reported, with its code, its count and its message, ordered by code and message. `schema-absent` is left out, since a case declares no schema. |

Folders a message names are written as `[case]`, the case's directory, and `[run]`, the run's temporary folder.

To add a case, create its directory with the manifest, the mappings and the inputs, and regenerate the snapshots as described below.

## Running the corpus

The corpus needs `cargo-insta` (`cargo install cargo-insta`). Check it as CI does:

```sh
cargo insta test --check --unreferenced=reject -p cassiopeia-pipeline --all-features -- end_to_end::golden::
```

`cargo test --workspace` runs the corpus too. In CI it runs only in the `snapshots` workflow, as the command above, and the `test` workflow skips it with `--skip end_to_end::golden::`.

When a snapshot differs, the test prints, before insta's diff, the case, the snapshot, and where it first differs: the entity, by its `id` and its index, and its first member that differs, or the diagnostic, by its code, and its first member that differs, such as its `occurrences` or a line of its `message`:

```text
golden case `values/json-property`, snapshot `current-concise`: entity `urn:ngsi-ld:Sample:S1`, at index 0, differs first at `/asIs/json/no2`: the snapshot holds `3`, the run wrote `4`
```

## Regenerating the snapshots

Regenerate every snapshot with one command:

```sh
cargo insta test --accept --unreferenced=delete -p cassiopeia-pipeline --all-features -- end_to_end::golden::
```

It writes the snapshots of every view that changed or is new, and deletes the snapshots no view writes any more, such as those of a removed case or of a series a case no longer writes. Commit the regenerated files with the change that alters the output, so its review shows the difference.

To review each difference before accepting it, run `cargo insta test --review` with the same arguments instead of `--accept`. To regenerate only some cases, add `--glob-filter` with the path of their manifests, such as `--glob-filter 'values/json-property/manifest.json5'`, and leave out `--unreferenced=delete`, which would otherwise delete the snapshots of every case the filter skips.
