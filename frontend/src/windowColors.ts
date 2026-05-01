const PASTEL_COLORS = [
	"#fce4ec", // pink
	"#e8eaf6", // indigo
	"#e0f2f1", // teal
	"#fff9c4", // yellow
	"#f3e5f5", // purple
	"#e8f5e9", // green
	"#fff3e0", // orange
	"#e1f5fe", // light blue
	"#fbe9e7", // deep orange
	"#f1f8e9", // light green
	"#ede7f6", // deep purple
	"#e0f7fa", // cyan
];

/**
 * Pick a tint from `PASTEL_COLORS` deterministically from a window UUID.
 * Same `window_id` → same colour across browser tabs / reloads, so no
 * cross-frontend syncing is required.
 */
export function colorForWindowId(windowId: string): string {
	let hash = 0;
	for (let i = 0; i < windowId.length; i++) {
		hash = ((hash << 5) - hash + windowId.charCodeAt(i)) | 0;
	}
	const idx = Math.abs(hash) % PASTEL_COLORS.length;
	return PASTEL_COLORS[idx];
}
