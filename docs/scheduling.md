# Scheduling

A schedule makes a run repeat instead of exiting after one pass. It turns a one-off transformation into a poller that fetches a live feed, maps it, writes the entities, waits, and repeats. This keeps a file or context broker current as new data arrives. [Example 23](examples/23-json-scheduling/example.md) shows a complete air-quality poller. This page explains trigger behavior, including the timezone rules that are easiest to get wrong.

Every scheduling setting has two equivalent forms: a field inside a manifest's `schedule` block and a command-line flag on `cassiopeia map`. This page gives both forms. The manifest field table is in the [manifest guide](manifest.md#schedule).

Whichever form you use, the first run fires immediately. Every later run waits for the trigger. With no bound, the command stays alive and keeps firing until you interrupt it (Ctrl-C).

## The three triggers

A schedule has exactly one trigger. In a manifest, the trigger is a `mode` and a `value`. On the command line, it is one of three mutually exclusive flags.

### `every`: a fixed interval

Runs on a repeating interval written in the format understood by [humantime](https://docs.rs/humantime), such as `30s`, `5m`, or `2h`.

~~~json5
schedule: {
    mode: "every",
    value: "5m",
}
~~~

~~~bash
cassiopeia map -i feed.json -m feed.json5 -o out --every 5m
~~~

An interval is independent of timezones. It measures elapsed time from one run to the next. The first run fires at once, and each later run waits one interval.

### `cron`: a cron expression

Runs at the instants named by a cron expression.

~~~json5
schedule: {
    mode: "cron",
    value: "0 */5 * * * *",
}
~~~

~~~bash
cassiopeia map -i feed.json -m feed.json5 -o out --cron "0 */5 * * * *"
~~~

The expression has **six fields with seconds precision**: `second minute hour day-of-month month day-of-week`. An optional seventh `year` field is also supported, as are the `@` shorthands (`@hourly`, `@daily`, `@weekly`, `@monthly`, `@yearly`). A conventional five-field crontab line has no seconds field and will not parse. The example `0 */5 * * * *` means "at second 0 of every fifth minute", not "every five hours".

A cron expression is **evaluated in UTC**. `0 0 9 * * *` fires at 09:00 UTC regardless of the host's local timezone.

### `at`: fixed times of day

Runs at one or more wall-clock times written as `HH:MM` with minute resolution.

~~~json5
schedule: {
    mode: "at",
    value: [
        "06:00",
        "18:00",
    ],
}
~~~

~~~bash
cassiopeia map -i feed.json -m feed.json5 -o out --at 06:00,18:00
~~~

Unlike `cron`, `at` times are **evaluated in local time**. `06:00` means 06:00 in the host's timezone. This UTC-versus-local distinction is easy to miss: a `cron` expression and an `at` time that show the same clock time will fire at different instants when the host is not on UTC. When the timezone matters, use `at` for local wall-clock times and `cron` when you need the instant pinned to UTC.

## Bound and harden the schedule

The remaining settings limit how long the schedule runs and control how it handles failures. All are optional and independent of the trigger.

| Purpose | Manifest field | CLI flag | Notes |
| --- | --- | --- | --- |
| Cap the number of runs | `repeat` | `--repeat` | At least 1. Unbounded when omitted. |
| Cap the total running time | `duration` | `--schedule-duration` | A duration such as `24h`. Stops after this regardless of `repeat`. |
| Stagger the start | `jitter` | `--jitter` | A random extra delay of up to this duration before each run, so deployments sharing one schedule do not all start at once. |
| Retry a failed run | `retry` | `--retry` + `--retry-backoff` | See below. |
| React to a failure for good | `onFailure` (top-level, not under `schedule`) | `--on-failure` | `abort` (the default) ends the schedule at once; `continue` logs the failure and waits for the next run, exiting non-zero when the schedule finishes; `ignore` logs it, waits, and exits zero. |

`retry` re-attempts a single failed run before the schedule treats it as failed. In a manifest, it is an object with a required `maxAttempts` of at least 1 and an optional `backoff` duration between attempts. The default backoff is `5s`:

~~~json5
schedule: {
    mode: "every",
    value: "5m",
    retry: {
        maxAttempts: 3,
        backoff: "15s",
    },
}
~~~

On the command line, the same policy is `--retry 3 --retry-backoff 15s`. `--retry-backoff` requires `--retry` and defaults to `5s` when omitted. Once retries are exhausted, the top-level `onFailure` decides the outcome. `abort` (the default) stops the schedule and exits non-zero. `continue` logs the failure and waits for the next scheduled run, exiting non-zero when the schedule finishes. `ignore` does the same but exits zero, which suits a long-lived poller that should continue after a transient bad poll. See [Handle failures](running.md#handle-failures) for the full policy and its exit-status guarantee.

## A worked example

This is the schedule from [Example 23](examples/23-json-scheduling/example.md). It polls a live air-quality feed every five minutes and upserts each reading to a broker. Retries handle dropped connections, and a bad poll does not stop the schedule.

~~~json5
{
    version: "v1",
    inputs: [
        {
            source: "https://data.sensor.community/airrohr/v1/filter/area=46.05,14.51,12",
            mapping: "air-quality.json5",
            format: "json",
        },
    ],
    // Tolerate a bad poll: keep the schedule running and exit zero even if a cycle fails for good.
    onFailure: "ignore",
    output: {
        target: "context-broker",
        url: "http://localhost:9090/",
        operation: "upsert",
        context: "default",
        validation: {
            mode: "fail-when-schema",
        },
    },
    schedule: {
        mode: "every",
        value: "5m",
        jitter: "20s",
        retry: {
            maxAttempts: 3,
            backoff: "15s",
        },
    },
}
~~~

~~~bash
cassiopeia map --manifest manifest.json5
~~~

The same run can be expressed entirely inline, without a manifest. The input, output, schedule, and failure policy move to flags. The default is `abort`, so tolerating a bad poll requires an explicit `--on-failure ignore`:

~~~bash
cassiopeia map \
    --input "https://data.sensor.community/airrohr/v1/filter/area=46.05,14.51,12" \
    --mapping air-quality.json5 \
    --type json \
    --writer context-broker \
    --broker-url http://localhost:9090/ \
    --broker-operation upsert \
    --context default \
    --validation-mode fail-when-schema \
    --on-failure ignore \
    --every 5m \
    --jitter 20s \
    --retry 3 \
    --retry-backoff 15s
~~~

The first poll runs at once. Every five minutes, plus up to 20 seconds of jitter, the schedule fires again, fetches the next window, and upserts each sensor's fresh reading. A failed poll is retried three times, 15 seconds apart. If it still fails, Cassiopeia logs the failure and waits for the next run.

## Next steps

- [Examples](examples/index.md): apply each idea in these guides to a real dataset.
- [Manifests](manifest.md#schedule): see the `schedule` field table.
- [Running Cassiopeia](running.md): use the full command-line guide for schedule flags.
- [Example 23](examples/23-json-scheduling/example.md): follow a live feed polled on a schedule from beginning to end.
