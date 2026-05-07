---
name: o2-cli
description: How to use the `o2` CLI (packages/o2-cli) to inspect the local OpenObserve dev stack — listing streams, browsing logs, fetching traces, sampling metrics, and introspecting stream schemas. Invoke when the user asks about logs, traces, metrics, OpenObserve, or observability for this project.
---

# Using the `o2` CLI

`o2` is the project's CLI for inspecting the local OpenObserve dev stack
(`compose.dev.yml` runs OpenObserve on `:5080` UI / `:5081` OTLP gRPC).

It lives in `packages/o2-cli` and runs through tsx — no build step. From the
repo root, invoke it with:

```
node packages/o2-cli/bin/o2.mjs <subcommand> [flags]
```

A repo-root `.env` is autoloaded (`O2_ENDPOINT`, `O2_ORG`, `O2_EMAIL`,
`O2_PASSWORD` — all default to the dev container).

A global `--json` flag emits raw JSON for any subcommand instead of the
human-formatted output.

## Subcommands

### `streams` — list streams grouped by type
```
node packages/o2-cli/bin/o2.mjs streams
```
First call when poking around — confirms which streams exist (logs / traces /
metrics / metadata) and surfaces metric stream names.

### `fields <stream> [--type logs|traces|metrics]` — show a stream's columns
```
node packages/o2-cli/bin/o2.mjs fields default --type traces
```
**Run this first** before writing any custom SQL against a stream. OpenObserve
grows the schema lazily — fields only exist after something has populated
them, so naming a column blindly in a SELECT 400s with `Schema error: No field
named X`. `--type` defaults to `logs`.

### `logs [-s service] [--since DUR] [-n LIMIT] [-f]` — recent log lines
```
node packages/o2-cli/bin/o2.mjs logs --service x11-web-backend --since 10m
node packages/o2-cli/bin/o2.mjs logs -f --since 1m   # tail -f style
```
`--since` accepts `30s` / `5m` / `1h` / `2d`. With `-f / --follow`, the CLI
backfills via `--since`, then polls every 2s for new rows. Ctrl+C exits.

### `trace <trace_id> [--since DUR]` — every span of one trace
```
node packages/o2-cli/bin/o2.mjs trace cbbc7f0f78c63b3eae5cc489207d8026
```
`trace_id` must be 32 hex chars (W3C Trace Context). Lookback defaults to 24h
because traces age out fast in the dev volume. Use `traces` (below) to find a
trace_id without bouncing to the web UI.

### `traces [--since DUR] [-n LIMIT]` — recent traces, root span shown
```
node packages/o2-cli/bin/o2.mjs traces --since 1h -n 10
```
Groups a span scan client-side (OpenObserve has no list-traces API), so the
limit is approximate when traces have very different span counts.

### `metric <name> [--since DUR] [-n LIMIT]` — recent metric samples
```
node packages/o2-cli/bin/o2.mjs metric x11web.frame_count --since 30m
```
Pass the OTel instrument name verbatim (`x11web.frame_count`); the CLI
translates dots to underscores for the OpenObserve stream name
(`x11web_frame_count`).

## Common flows

- **"Why isn't my log showing up?"** → `logs --since 5m` (start broad), then
  narrow with `--service`. If nothing appears, check the OTel SDK is exporting
  (look for `OTel enabled` in the backend's stdout).
- **"What's a chatty trace doing?"** → `traces -n 5` to grab a `trace_id`,
  then `trace <id>` to see every span.
- **"My SQL is 400ing"** → `fields <stream> --type <kind>` to see what columns
  actually exist; OpenObserve's lazy schema is the usual culprit.
- **"Tail logs while reproducing a bug"** → `logs --service <svc> -f --since 30s`.

## Troubleshooting

- `fetch failed` → OpenObserve isn't running. `docker compose -f compose.dev.yml up -d openobserve`, or start `pnpm dev` (mprocs brings it up).
- `401` → bad creds. Defaults match `compose.dev.yml` (`admin@admin.com / admin`); check your `.env` if you've overridden them.
- `Schema error: No field named X` → run `fields` to see the real schema; the field probably hasn't been ingested yet (single-span traces don't have `parent_span_id`, logs without a `severity` attribute don't have `severity_text`, etc.).
