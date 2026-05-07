/**
 * `o2` — small CLI for inspecting the local OpenObserve dev
 * stack. Wraps the SQL/timestamp/auth dance in a few common
 * subcommands. Connection config is env-driven (`O2_ENDPOINT`,
 * `O2_ORG`, `O2_EMAIL`, `O2_PASSWORD`) with sensible defaults
 * matching `compose.dev.yml`.
 */

import { Command } from "commander";
import { z } from "zod";
import {
	configFromEnv,
	O2Client,
	parseDuration,
	type SearchHit,
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
});

program
	.command("logs")
	.description("Recent log lines, optionally filtered by service.")
	.option("-s, --service <name>", "filter by `service.name` (e.g. x11-web-backend)")
	.option("--since <duration>", "lookback window (30s, 5m, 1h, 2d)", "1h")
	.option("-n, --limit <n>", "max rows", "50")
	.action(async (raw) => {
		const opts = LogsOptionsSchema.parse(raw);
		const cli = new O2Client(configFromEnv());
		const window = windowEndingNow(parseDuration(opts.since));
		const where = opts.service
			? `WHERE service_name = '${opts.service.replace(/'/g, "''")}'`
			: "";
		const sql = `SELECT _timestamp, service_name, body FROM "default" ${where} ORDER BY _timestamp DESC LIMIT ${opts.limit}`;
		const hits = await cli.search({
			sql,
			...window,
			streamType: "logs",
		});
		emit(hits, (h) => {
			const ts = formatMicros(h._timestamp);
			const svc = (h.service_name ?? "").toString().padEnd(20);
			const body = h.body ?? "";
			return `${ts}  ${svc}  ${body}`;
		});
	});

const TraceArgsSchema = z.object({
	traceId: z
		.string()
		.regex(/^[0-9a-fA-F]{32}$/, "trace_id must be 32 hex chars (W3C Trace Context)"),
});

program
	.command("trace <trace_id>")
	.description("Fetch every span belonging to a trace.")
	.option("--since <duration>", "lookback window (default 24h — traces age out)", "24h")
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
			const dur = h.duration != null ? `${Math.round(Number(h.duration) / 1000)}ms` : "-";
			return `${ts}  ${svc}  ${op}  span=${h.span_id}  parent=${h.parent_span_id ?? "-"}  ${dur}`;
		});
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
	.description("Recent samples for a metric. Stream name is derived from the OTel instrument name (dots → underscores).")
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
