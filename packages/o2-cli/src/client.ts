import { z } from "zod";

/** OpenObserve connection config. Driven by env so the same
 *  binary works against the dev container, a staging instance,
 *  or a production OpenObserve without code changes. */
export interface Config {
	endpoint: string;
	org: string;
	authHeader: string;
}

export function configFromEnv(): Config {
	const endpoint = (
		process.env.O2_ENDPOINT ?? "http://localhost:5080"
	).replace(/\/+$/, "");
	const org = process.env.O2_ORG ?? "default";
	const email = process.env.O2_EMAIL ?? "admin@admin.com";
	const password = process.env.O2_PASSWORD ?? "admin";
	const token = Buffer.from(`${email}:${password}`).toString("base64");
	return { endpoint, org, authHeader: `Basic ${token}` };
}

/** Stream listing — narrow to the bits we actually print. */
const StreamSchema = z.object({
	name: z.string(),
	stream_type: z.string(),
});
const StreamListSchema = z.object({
	list: z.array(StreamSchema),
});
export type Stream = z.infer<typeof StreamSchema>;

/** Search hit shape. OpenObserve returns whatever columns the
 *  query selected; everything's optional from our POV. */
const SearchHitSchema = z.record(z.string(), z.unknown());
const SearchResponseSchema = z.object({
	hits: z.array(SearchHitSchema),
	total: z.number().optional(),
	took: z.number().optional(),
});
export type SearchHit = z.infer<typeof SearchHitSchema>;

export class O2Client {
	constructor(private readonly cfg: Config) {}

	private async request(
		method: "GET" | "POST",
		path: string,
		body?: unknown,
	): Promise<unknown> {
		const url = `${this.cfg.endpoint}${path}`;
		const res = await fetch(url, {
			method,
			headers: {
				Authorization: this.cfg.authHeader,
				...(body ? { "Content-Type": "application/json" } : {}),
			},
			body: body ? JSON.stringify(body) : undefined,
		});
		if (!res.ok) {
			const text = await res.text();
			throw new Error(
				`OpenObserve ${method} ${path} → ${res.status}: ${text || res.statusText}`,
			);
		}
		return res.json();
	}

	async listStreams(): Promise<Stream[]> {
		const raw = await this.request("GET", `/api/${this.cfg.org}/streams`);
		const parsed = StreamListSchema.parse(raw);
		return parsed.list;
	}

	/** Run a SQL search against a stream. `sql` should already
	 *  reference the target stream by name (the SDK doesn't
	 *  prefix-rewrite, so `FROM "default"` etc. is on the caller). */
	async search(opts: {
		sql: string;
		startTime: number; // µs since epoch
		endTime: number; // µs since epoch
		streamType?: "logs" | "traces" | "metrics";
	}): Promise<SearchHit[]> {
		const params = new URLSearchParams();
		if (opts.streamType) params.set("type", opts.streamType);
		const qs = params.toString() ? `?${params.toString()}` : "";
		const raw = await this.request(
			"POST",
			`/api/${this.cfg.org}/_search${qs}`,
			{
				query: {
					sql: opts.sql,
					start_time: opts.startTime,
					end_time: opts.endTime,
				},
			},
		);
		const parsed = SearchResponseSchema.parse(raw);
		return parsed.hits;
	}
}

/** Parse "5m" / "1h" / "30s" / "2d" → microseconds. The CLI
 *  takes durations in this Loki-style shorthand because nobody
 *  wants to type microseconds. */
export function parseDuration(s: string): number {
	const m = /^(\d+)(s|m|h|d)$/.exec(s);
	if (!m) {
		throw new Error(`bad duration "${s}" (expected e.g. 30s, 5m, 1h, 2d)`);
	}
	const n = Number(m[1]);
	const factor = { s: 1, m: 60, h: 3600, d: 86400 }[m[2] as "s" | "m" | "h" | "d"];
	return n * factor * 1_000_000; // µs
}

/** Wall-clock window ending now. */
export function windowEndingNow(durationMicros: number): {
	startTime: number;
	endTime: number;
} {
	const endTime = Date.now() * 1_000;
	return { startTime: endTime - durationMicros, endTime };
}
