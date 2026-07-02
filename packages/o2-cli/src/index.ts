/**
 * `o2` — small CLI for inspecting the local OpenObserve dev
 * stack. Wraps the SQL/timestamp/auth dance in a few common
 * subcommands. Connection config is env-driven (`O2_ENDPOINT`,
 * `O2_ORG`, `O2_EMAIL`, `O2_PASSWORD`) with sensible defaults
 * matching `compose.dev.yml`.
 *
 * `dotenv/config` autoloads `./.env` from the cwd so the CLI
 * picks up creds from a repo-root `.env` without extra wiring.
 */

import "dotenv/config";
import { setTimeout as sleep } from "node:timers/promises";
import { Command } from "commander";
import { z } from "zod";
import {
	configFromEnv,
	O2Client,
	parseDuration,
	type SearchHit,
	type StreamType,
	windowEndingNow,
} from "./client.ts";

const program = new Command();
program
	.name("o2")
	.description("Inspect the local OpenObserve dev stack.")
	.option("--json", "emit raw JSON instead of human-formatted output", false);

program
	.command("streams")
	.description("List streams in the org, grouped by type.")
	.action(async () => {
		const cli = new O2Client(configFromEnv());
		const streams = await cli.listStreams();
		const byType = new Map<string, string[]>();
		for (const s of streams) {
			const list = byType.get(s.stream_type) ?? [];
			list.push(s.name);
			byType.set(s.stream_type, list);
		}
		if (program.opts().json) {
			console.log(JSON.stringify(streams, null, 2));
			return;
		}
		for (const [type, names] of [...byType].sort()) {
			console.log(`\n${type}`);
			for (const n of names.sort()) console.log(`  ${n}`);
		}
	});

const LogsOptionsSchema = z.object({
	service: z.string().optional(),
	since: z.string().default("1h"),
	limit: z.coerce.number().int().positive().default(50),
	follow: z.boolean().default(false),
});

const formatLog = (h: SearchHit): string => {
	const ts = formatMicros(h._timestamp);
	const svc = (h.service_name ?? "").toString().padEnd(20);
	const body = h.body ?? "";
	return `${ts}  ${svc}  ${body}`;
};

program
	.command("logs")
	.description("Recent log lines, optionally filtered by service.")
	.option(
		"-s, --service <name>",
		"filter by `service.name` (e.g. x11-web-backend)",
	)
	.option("--since <duration>", "lookback window (30s, 5m, 1h, 2d)", "1h")
	.option("-n, --limit <n>", "max rows", "50")
	.option(
		"-f, --follow",
		"tail new log lines as they arrive (poll every 2s)",
		false,
	)
	.action(async (raw) => {
		const opts = LogsOptionsSchema.parse(raw);
		const cli = new O2Client(configFromEnv());
		const serviceFilter = opts.service
			? ` AND service_name = '${opts.service.replace(/'/g, "''")}'`
			: "";

		// Initial backfill: most-recent N rows in the lookback
		// window, then reversed to chronological order so the next
		// follow poll picks up *after* the last printed row.
		const initialWindow = windowEndingNow(parseDuration(opts.since));
		const initialSql =
			`SELECT _timestamp, service_name, body FROM "default" ` +
			`WHERE _timestamp >= ${initialWindow.startTime}` +
			`${serviceFilter} ORDER BY _timestamp DESC LIMIT ${opts.limit}`;
		const initial = (
			await cli.search({
				sql: initialSql,
				...initialWindow,
				streamType: "logs",
			})
		)
			.slice()
			.reverse();
		emit(initial, formatLog);
		if (!opts.follow) return;

		// Poll loop: track the highest `_timestamp` we've printed,
		// and ask for everything strictly after it. End the window
		// at "now" each iteration so the server doesn't have to
		// scan the future. Ctrl+C is the exit.
		let lastTs = initial.reduce(
			(max, h) => Math.max(max, Number(h._timestamp ?? 0)),
			initialWindow.startTime,
		);
		while (true) {
			await sleep(2000);
			const endTime = Date.now() * 1000;
			const sql =
				`SELECT _timestamp, service_name, body FROM "default" ` +
				`WHERE _timestamp > ${lastTs}${serviceFilter} ORDER BY _timestamp ASC`;
			const hits = await cli.search({
				sql,
				startTime: lastTs + 1,
				endTime,
				streamType: "logs",
			});
			for (const h of hits) {
				console.log(formatLog(h));
				lastTs = Math.max(lastTs, Number(h._timestamp ?? 0));
			}
		}
	});

const TraceArgsSchema = z.object({
	traceId: z
		.string()
		.regex(
			/^[0-9a-fA-F]{32}$/,
			"trace_id must be 32 hex chars (W3C Trace Context)",
		),
});

program
	.command("trace <trace_id>")
	.description("Fetch every span belonging to a trace.")
	.option(
		"--since <duration>",
		"lookback window (default 24h — traces age out)",
		"24h",
	)
	.action(async (traceId, raw) => {
		const args = TraceArgsSchema.parse({ traceId });
		const since = (raw.since ?? "24h") as string;
		const cli = new O2Client(configFromEnv());
		const window = windowEndingNow(parseDuration(since));
		// `SELECT *` rather than naming columns: OpenObserve's
		// traces schema is dynamic and `parent_span_id` only exists
		// once at least one span has a parent, so a fixed projection
		// 400s on single-span traces.
		const sql = `SELECT * FROM "default" WHERE trace_id = '${args.traceId}' ORDER BY _timestamp ASC`;
		const hits = await cli.search({
			sql,
			...window,
			streamType: "traces",
		});
		emit(hits, (h) => {
			const ts = formatMicros(h._timestamp);
			const svc = (h.service_name ?? "").toString().padEnd(20);
			const op = (h.operation_name ?? "").toString().padEnd(36);
			const dur =
				h.duration != null ? `${Math.round(Number(h.duration) / 1000)}ms` : "-";
			return `${ts}  ${svc}  ${op}  span=${h.span_id}  parent=${h.parent_span_id ?? "-"}  ${dur}`;
		});
	});

const TracesOptionsSchema = z.object({
	since: z.string().default("1h"),
	limit: z.coerce.number().int().positive().default(20),
});

program
	.command("traces")
	.description("Recent traces — one row per trace_id, root span shown.")
	.option("--since <duration>", "lookback window (default 1h)", "1h")
	.option("-n, --limit <n>", "max traces", "20")
	.action(async (raw) => {
		const opts = TracesOptionsSchema.parse(raw);
		const cli = new O2Client(configFromEnv());
		const window = windowEndingNow(parseDuration(opts.since));
		// OpenObserve doesn't have a "list traces" API; fan-out
		// from a span scan. We over-fetch (limit × 50, capped at
		// 1000) so a chatty trace doesn't crowd quieter traces out
		// of the result set, then group client-side.
		const scanLimit = Math.min(1000, opts.limit * 50);
		const sql =
			`SELECT trace_id, service_name, operation_name, start_time, duration ` +
			`FROM "default" ORDER BY _timestamp DESC LIMIT ${scanLimit}`;
		const hits = await cli.search({ sql, ...window, streamType: "traces" });
		// Pick the lowest start_time per trace_id as the root.
		// span_count is best-effort within the scan window.
		type Row = {
			traceId: string;
			service: string;
			op: string;
			start: number;
			duration: number;
			spanCount: number;
		};
		const byTrace = new Map<string, Row>();
		for (const h of hits) {
			const traceId = String(h.trace_id ?? "");
			if (!traceId) continue;
			const start = Number(h.start_time ?? h._timestamp ?? 0);
			const existing = byTrace.get(traceId);
			if (!existing) {
				byTrace.set(traceId, {
					traceId,
					service: String(h.service_name ?? ""),
					op: String(h.operation_name ?? ""),
					start,
					duration: Number(h.duration ?? 0),
					spanCount: 1,
				});
				continue;
			}
			existing.spanCount += 1;
			if (start < existing.start) {
				existing.start = start;
				existing.service = String(h.service_name ?? existing.service);
				existing.op = String(h.operation_name ?? existing.op);
				existing.duration = Number(h.duration ?? existing.duration);
			}
		}
		const traces = [...byTrace.values()]
			.sort((a, b) => b.start - a.start)
			.slice(0, opts.limit);
		if (program.opts().json) {
			console.log(JSON.stringify(traces, null, 2));
			return;
		}
		if (traces.length === 0) {
			console.error("(no results)");
			return;
		}
		for (const t of traces) {
			// `start_time` is nanoseconds in the OTel/OpenObserve
			// span schema; `_timestamp` is microseconds. Use start.
			const ts = new Date(Math.round(t.start / 1_000_000)).toISOString();
			const svc = t.service.padEnd(20);
			const op = t.op.padEnd(36);
			const dur = t.duration
				? `${Math.round(t.duration / 1000)}ms`.padStart(8)
				: "       -";
			console.log(
				`${ts}  ${t.traceId}  ${svc}  ${op}  ${dur}  spans=${t.spanCount}`,
			);
		}
	});

const FieldsArgsSchema = z.object({
	stream: z.string().min(1),
	type: z.enum(["logs", "traces", "metrics"]).default("logs"),
});

program
	.command("fields <stream>")
	.description(
		"Show columns of a stream's schema (handy because OpenObserve grows the schema lazily).",
	)
	.option("-t, --type <kind>", "stream type: logs | traces | metrics", "logs")
	.action(async (stream, raw) => {
		const args = FieldsArgsSchema.parse({ stream, type: raw.type });
		const cli = new O2Client(configFromEnv());
		const fields = await cli.getStreamSchema(
			args.stream,
			args.type as StreamType,
		);
		if (program.opts().json) {
			console.log(JSON.stringify(fields, null, 2));
			return;
		}
		if (fields.length === 0) {
			console.error("(no schema — stream may be empty or not yet ingested)");
			return;
		}
		const width = fields.reduce((w, f) => Math.max(w, f.name.length), 0);
		for (const f of [...fields].sort((a, b) => a.name.localeCompare(b.name))) {
			console.log(`${f.name.padEnd(width)}  ${f.type}`);
		}
	});

const MetricArgsSchema = z.object({
	name: z
		.string()
		.regex(
			/^[a-zA-Z][a-zA-Z0-9_.]*$/,
			"metric name should be a valid OTel instrument name (letters / digits / underscores / dots)",
		),
});

program
	.command("metric <name>")
	.description(
		"Recent samples for a metric. Stream name is derived from the OTel instrument name (dots → underscores).",
	)
	.option("--since <duration>", "lookback window (default 1h)", "1h")
	.option("-n, --limit <n>", "max rows", "50")
	.action(async (name, raw) => {
		const args = MetricArgsSchema.parse({ name });
		const since = (raw.since ?? "1h") as string;
		const limit = Number(raw.limit ?? 50);
		const cli = new O2Client(configFromEnv());
		const window = windowEndingNow(parseDuration(since));
		// OpenObserve replaces `.` with `_` when materialising
		// metric instruments into stream names.
		const stream = args.name.replace(/\./g, "_");
		const sql = `SELECT * FROM "${stream}" ORDER BY _timestamp DESC LIMIT ${limit}`;
		const hits = await cli.search({
			sql,
			...window,
			streamType: "metrics",
		});
		emit(hits, (h) => {
			const ts = formatMicros(h._timestamp);
			const value = h.value ?? "-";
			// Strip the timestamp / value / well-known fields out
			// of the rendered tag list so the user only sees the
			// dimension columns they care about.
			const skip = new Set([
				"_timestamp",
				"value",
				"start_time",
				"flags",
				"service_name",
				"service_instance_id",
				"otel_scope_name",
				"otel_scope_version",
			]);
			const tags = Object.entries(h)
				.filter(([k, v]) => !skip.has(k) && v !== null && v !== "")
				.map(([k, v]) => `${k}=${v}`)
				.join(" ");
			return `${ts}  value=${value}  ${tags}`;
		});
	});

function formatMicros(v: unknown): string {
	const n = Number(v);
	if (!Number.isFinite(n)) return "?";
	return new Date(Math.round(n / 1000)).toISOString();
}

function emit(hits: SearchHit[], format: (h: SearchHit) => string): void {
	if (program.opts().json) {
		console.log(JSON.stringify(hits, null, 2));
		return;
	}
	if (hits.length === 0) {
		console.error("(no results)");
		return;
	}
	for (const h of hits) console.log(format(h));
}

program.parseAsync(process.argv).catch((err) => {
	console.error(err instanceof Error ? err.message : String(err));
	process.exit(1);
});
