/** RGBA in 0..1, sRGB — passed raw into shaders so GL shapes match
 *  their CSS-colored DOM counterparts exactly. */
export interface Rgba {
	r: number;
	g: number;
	b: number;
	a: number;
}

export const TRANSPARENT: Rgba = { r: 0, g: 0, b: 0, a: 0 };

const NAMED: Record<string, Rgba> = {
	transparent: TRANSPARENT,
	white: { r: 1, g: 1, b: 1, a: 1 },
	black: { r: 0, g: 0, b: 0, a: 1 },
};

/** Parse the CSS color subset the canvas actually stores in docs:
 *  #rgb / #rgba / #rrggbb / #rrggbbaa, rgb()/rgba(), `transparent`,
 *  and a couple of names. Unknown input renders opaque magenta so
 *  bad data is visible instead of silently invisible. */
export function parseColor(input: string | undefined | null): Rgba {
	if (!input) return TRANSPARENT;
	const str = input.trim().toLowerCase();
	const named = NAMED[str];
	if (named) return named;

	if (str.startsWith("#")) {
		const hex = str.slice(1);
		if (hex.length === 3 || hex.length === 4) {
			const [r, g, b, a] = [...hex].map((c) => parseInt(c + c, 16) / 255);
			if ([r, g, b].every((v) => !Number.isNaN(v))) {
				return { r, g, b, a: hex.length === 4 && !Number.isNaN(a) ? a : 1 };
			}
		}
		if (hex.length === 6 || hex.length === 8) {
			const r = parseInt(hex.slice(0, 2), 16) / 255;
			const g = parseInt(hex.slice(2, 4), 16) / 255;
			const b = parseInt(hex.slice(4, 6), 16) / 255;
			const a = hex.length === 8 ? parseInt(hex.slice(6, 8), 16) / 255 : 1;
			if (![r, g, b, a].some((v) => Number.isNaN(v))) return { r, g, b, a };
		}
	}

	const fn = str.match(/^rgba?\(([^)]+)\)$/);
	if (fn) {
		const parts = fn[1].split(/[,/\s]+/).filter(Boolean);
		if (parts.length === 3 || parts.length === 4) {
			const [r, g, b] = parts.map((p) => Number.parseFloat(p) / 255);
			const a = parts[3] ? Number.parseFloat(parts[3]) : 1;
			if (![r, g, b, a].some((v) => Number.isNaN(v))) {
				return { r, g, b, a };
			}
		}
	}

	return { r: 1, g: 0, b: 1, a: 1 };
}
