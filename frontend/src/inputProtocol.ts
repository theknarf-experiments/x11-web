/**
 * Pure browser-event → X11 protocol translation. Lives standalone
 * so the table-driven mappings (key codes, button numbers, modifier
 * masks) can be unit-tested without mounting any React tree.
 *
 * Functions here take *structural* shapes, not `React.MouseEvent` /
 * `React.KeyboardEvent`, so tests can pass plain objects. The
 * shapes match the relevant subset of DOM event interfaces — every
 * call site in `WindowFrame` already satisfies them via React's
 * synthetic events.
 */

import type { InputEvent } from "./types";

/** Modifier-key subset of `KeyboardEvent` / `MouseEvent`. */
export interface ModifierLike {
	shiftKey: boolean;
	ctrlKey: boolean;
	altKey: boolean;
	metaKey: boolean;
}

/** Subset of `KeyboardEvent` we need for keycode translation. */
export interface KeyLike extends ModifierLike {
	code: string;
	keyCode: number;
	/** Printable-character key, used to infer shift when the
	 *  modifier wasn't explicitly pressed (e.g. Playwright's
	 *  keyboard.type(":")). Optional because legacy paths only need
	 *  code + keyCode. */
	key?: string;
}

/** Subset of `MouseEvent` we need for mouse / button translation. */
export interface MouseLike extends ModifierLike {
	button: number;
	buttons: number;
	clientX: number;
	clientY: number;
}

/** Browser `KeyboardEvent.code` → X11 keycode. The X server's
 *  modifier map then turns these into KeySyms; the keycode itself
 *  is just a hardware-position handle. */
const X11_KEYCODE_BY_CODE: Record<string, number> = {
	Escape: 9,
	Digit1: 10,
	Digit2: 11,
	Digit3: 12,
	Digit4: 13,
	Digit5: 14,
	Digit6: 15,
	Digit7: 16,
	Digit8: 17,
	Digit9: 18,
	Digit0: 19,
	Minus: 20,
	Equal: 21,
	Backspace: 22,
	Tab: 23,
	KeyQ: 24,
	KeyW: 25,
	KeyE: 26,
	KeyR: 27,
	KeyT: 28,
	KeyY: 29,
	KeyU: 30,
	KeyI: 31,
	KeyO: 32,
	KeyP: 33,
	BracketLeft: 34,
	BracketRight: 35,
	Enter: 36,
	ControlLeft: 37,
	KeyA: 38,
	KeyS: 39,
	KeyD: 40,
	KeyF: 41,
	KeyG: 42,
	KeyH: 43,
	KeyJ: 44,
	KeyK: 45,
	KeyL: 46,
	Semicolon: 47,
	Quote: 48,
	Backquote: 49,
	ShiftLeft: 50,
	Backslash: 51,
	KeyZ: 52,
	KeyX: 53,
	KeyC: 54,
	KeyV: 55,
	KeyB: 56,
	KeyN: 57,
	KeyM: 58,
	Comma: 59,
	Period: 60,
	Slash: 61,
	ShiftRight: 62,
	NumpadMultiply: 63,
	AltLeft: 64,
	Space: 65,
	CapsLock: 66,
	F1: 67,
	F2: 68,
	F3: 69,
	F4: 70,
	F5: 71,
	F6: 72,
	F7: 73,
	F8: 74,
	F9: 75,
	F10: 76,
	NumLock: 77,
	ScrollLock: 78,
	F11: 95,
	F12: 96,
	ControlRight: 105,
	AltRight: 108,
	Home: 110,
	ArrowUp: 111,
	PageUp: 112,
	ArrowLeft: 113,
	ArrowRight: 114,
	End: 115,
	ArrowDown: 116,
	PageDown: 117,
	Insert: 118,
	Delete: 119,
	MetaLeft: 133,
	MetaRight: 134,
};

/** Translate a `KeyboardEvent` into an X11 keycode. Returns `0`
 *  when no mapping is available (the caller should drop the
 *  event rather than send a bogus keycode). The legacy `keyCode`
 *  fallback adds 8 because X11 keycodes start at 8 to match the
 *  AT keyboard offset. */
export function browserKeyToX11Keycode(e: KeyLike): number {
	if (e.code && X11_KEYCODE_BY_CODE[e.code] !== undefined) {
		return X11_KEYCODE_BY_CODE[e.code];
	}
	if (e.keyCode > 0) {
		return e.keyCode + 8;
	}
	return 0;
}

/** Browser `MouseEvent.button` (left=0, middle=1, right=2) → X11
 *  button number (left=1, middle=2, right=3). Buttons 4+ are
 *  reserved for wheel / extra buttons in X11; we shift by +1 for
 *  any unknown mouse button so they remain distinct. */
export function x11Button(browserButton: number): number {
	switch (browserButton) {
		case 0:
			return 1;
		case 1:
			return 2;
		case 2:
			return 3;
		default:
			return browserButton + 1;
	}
}

/** Convert the DOM `MouseEvent.buttons` bitfield into the X11 KeyButMask
 *  bits the protocol carries on motion / button events. The DOM
 *  layout puts middle on bit 2; X11 uses 0x100 (left), 0x200
 *  (middle), 0x400 (right). */
export function mouseButtonMask(buttons: number): number {
	let mask = 0;
	if (buttons & 1) mask |= 0x100;
	if (buttons & 4) mask |= 0x200;
	if (buttons & 2) mask |= 0x400;
	return mask;
}

/** Modifier flags packed as X11 KeyButMask bits. Used on both
 *  mouse and keyboard events: the protocol carries the keyboard
 *  modifier state alongside button state on every input event. */
export function modifierMask(e: ModifierLike): number {
	let mask = 0;
	if (e.shiftKey) mask |= 0x01;
	if (e.ctrlKey) mask |= 0x04;
	if (e.altKey) mask |= 0x08;
	if (e.metaKey) mask |= 0x40;
	return mask;
}

/** Shifted printable characters (`:`, `?`, `<`, `>`, `_`, `+`, `!`, `@`,
 *  `#`, `$`, `%`, `^`, `&`, `*`, `(`, `)`, `{`, `}`, `|`, `~`, `"`, A-Z)
 *  whose corresponding `code` value would otherwise pair with shiftKey
 *  unset. Playwright's `keyboard.type(":")` dispatches the colon char
 *  with `code="Semicolon"` but does NOT fire a Shift modifier, so a
 *  literal modifierMask read would lose the shift bit and the X server
 *  would see `;` instead of `:`. When `e.key` is one of these characters
 *  we set the shift bit unconditionally. */
const SHIFTED_KEY_CHARS: ReadonlySet<string> = new Set([
	":",
	"<",
	">",
	"?",
	"~",
	'"',
	"{",
	"}",
	"|",
	"!",
	"@",
	"#",
	"$",
	"%",
	"^",
	"&",
	"*",
	"(",
	")",
	"_",
	"+",
	// Uppercase letters are also shifted versions of their lowercase keys.
	..."ABCDEFGHIJKLMNOPQRSTUVWXYZ",
]);

function impliedShiftMask(e: KeyLike): number {
	return e.key && SHIFTED_KEY_CHARS.has(e.key) ? 0x01 : 0;
}

/** Resolve the bit in `MouseEvent.buttons` corresponding to
 *  `MouseEvent.button`. Browsers report `buttons` AFTER the press
 *  for a `mousedown` and BEFORE the release for a `mouseup`, so
 *  the caller has to reconstruct the alternate state by toggling
 *  this bit. Index 0=left, 1=middle, 2=right. */
function buttonBitFor(button: number): number {
	return [1, 4, 2][button] ?? 0;
}

/** Translate canvas-space coords from a `clientX/Y` pair using
 *  the canvas's bounding rect and intrinsic dimensions. */
export interface CanvasGeometry {
	rect: { left: number; top: number; width: number; height: number };
	width: number;
	height: number;
}

export function clientToCanvas(
	clientX: number,
	clientY: number,
	g: CanvasGeometry,
): { x: number; y: number; scaleX: number; scaleY: number } {
	const scaleX = g.width / g.rect.width;
	const scaleY = g.height / g.rect.height;
	return {
		x: Math.round((clientX - g.rect.left) * scaleX),
		y: Math.round((clientY - g.rect.top) * scaleY),
		scaleX,
		scaleY,
	};
}

/** `mousemove` in canvas space → `MotionNotify`. */
export function mouseMoveToInput(
	e: MouseLike,
	g: CanvasGeometry,
): InputEvent {
	const { x, y } = clientToCanvas(e.clientX, e.clientY, g);
	return {
		kind: "MotionNotify",
		x,
		y,
		state: mouseButtonMask(e.buttons),
	};
}

/** `mousedown` → `ButtonPress`. The browser reports `buttons` as
 *  the post-press state, so we strip the just-pressed bit before
 *  encoding the X11 state. */
export function mouseDownToInput(
	e: MouseLike,
	g: CanvasGeometry,
): InputEvent {
	const { x, y } = clientToCanvas(e.clientX, e.clientY, g);
	const prePress = e.buttons & ~buttonBitFor(e.button);
	return {
		kind: "ButtonPress",
		button: x11Button(e.button),
		x,
		y,
		state: mouseButtonMask(prePress) | modifierMask(e),
	};
}

/** `mouseup` → `ButtonRelease`. The browser reports `buttons` as
 *  the post-release state; reconstruct the pre-release set by
 *  re-adding the just-released bit so the X11 state matches what
 *  was true at the instant of the release. */
export function mouseUpToInput(
	e: MouseLike,
	g: CanvasGeometry,
): InputEvent {
	const { x, y } = clientToCanvas(e.clientX, e.clientY, g);
	const preRelease = e.buttons | buttonBitFor(e.button);
	return {
		kind: "ButtonRelease",
		button: x11Button(e.button),
		x,
		y,
		state: mouseButtonMask(preRelease) | modifierMask(e),
	};
}

/** `keydown` → `KeyPress`, or `null` when the keycode is
 *  unmappable (caller drops the event rather than sending a zero
 *  keycode the X server will reject). */
export function keyDownToInput(e: KeyLike): InputEvent | null {
	const keycode = browserKeyToX11Keycode(e);
	if (keycode <= 0) return null;
	return {
		kind: "KeyPress",
		keycode,
		state: modifierMask(e) | impliedShiftMask(e),
	};
}

/** `keyup` → `KeyRelease`, or `null` when unmappable. */
export function keyUpToInput(e: KeyLike): InputEvent | null {
	const keycode = browserKeyToX11Keycode(e);
	if (keycode <= 0) return null;
	return {
		kind: "KeyRelease",
		keycode,
		state: modifierMask(e) | impliedShiftMask(e),
	};
}
