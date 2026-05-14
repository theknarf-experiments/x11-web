import { describe, expect, test } from "vitest";
import {
	browserKeyToX11Keycode,
	type CanvasGeometry,
	clientToCanvas,
	keyDownToInput,
	keyUpToInput,
	mouseButtonMask,
	mouseDownToInput,
	mouseMoveToInput,
	mouseUpToInput,
	modifierMask,
	x11Button,
} from "./inputProtocol";

const noMods = { shiftKey: false, ctrlKey: false, altKey: false, metaKey: false };

describe("browserKeyToX11Keycode", () => {
	test.each([
		["KeyA", 38],
		["Space", 65],
		["Escape", 9],
		["Enter", 36],
		["F12", 96],
		["MetaLeft", 133],
		["ArrowDown", 116],
	])("known code %s → keycode %i", (code, expected) => {
		expect(browserKeyToX11Keycode({ ...noMods, code, keyCode: 0 })).toBe(
			expected,
		);
	});

	test("falls back to keyCode + 8 when code is unmapped", () => {
		expect(
			browserKeyToX11Keycode({ ...noMods, code: "Unknown", keyCode: 65 }),
		).toBe(73);
	});

	test("returns 0 when both code and keyCode are missing", () => {
		expect(
			browserKeyToX11Keycode({ ...noMods, code: "", keyCode: 0 }),
		).toBe(0);
	});

	test("prefers code mapping over keyCode fallback", () => {
		// `code` is mapped to KeyA → 38; `keyCode` would yield 73
		// via the +8 fallback. The code path must win.
		expect(
			browserKeyToX11Keycode({ ...noMods, code: "KeyA", keyCode: 65 }),
		).toBe(38);
	});
});

describe("x11Button", () => {
	test.each([
		[0, 1],
		[1, 2],
		[2, 3],
		[3, 4],
		[4, 5],
	])("browser button %i → X11 button %i", (browser, x11) => {
		expect(x11Button(browser)).toBe(x11);
	});
});

describe("mouseButtonMask", () => {
	test.each([
		[0, 0],
		// DOM bit 1 (left)   → X11 0x100
		[1, 0x100],
		// DOM bit 4 (middle) → X11 0x200
		[4, 0x200],
		// DOM bit 2 (right)  → X11 0x400
		[2, 0x400],
		// All three pressed
		[1 | 2 | 4, 0x100 | 0x200 | 0x400],
		// Left + right
		[1 | 2, 0x100 | 0x400],
	])("DOM buttons %i → X11 mask %i", (buttons, expected) => {
		expect(mouseButtonMask(buttons)).toBe(expected);
	});
});

describe("modifierMask", () => {
	test("no modifiers", () => {
		expect(modifierMask(noMods)).toBe(0);
	});

	test("shift only", () => {
		expect(modifierMask({ ...noMods, shiftKey: true })).toBe(0x01);
	});

	test("ctrl + alt + meta", () => {
		expect(
			modifierMask({
				...noMods,
				ctrlKey: true,
				altKey: true,
				metaKey: true,
			}),
		).toBe(0x04 | 0x08 | 0x40);
	});
});

describe("clientToCanvas", () => {
	const g: CanvasGeometry = {
		rect: { left: 100, top: 50, width: 400, height: 300 },
		width: 800, // canvas is 2× rect → scaleX = 2
		height: 600,
	};

	test("translates client coords into canvas pixels at 2× scale", () => {
		// click at rect-relative (50, 100) → canvas (100, 200)
		expect(clientToCanvas(150, 150, g)).toEqual({
			x: 100,
			y: 200,
			scaleX: 2,
			scaleY: 2,
		});
	});

	test("rounds fractional pixels", () => {
		// rect-relative (1.5, 1.5) at scale 2 → (3, 3)
		expect(clientToCanvas(101.5, 51.5, g)).toMatchObject({ x: 3, y: 3 });
	});
});

describe("mouseMoveToInput", () => {
	const g: CanvasGeometry = {
		rect: { left: 0, top: 0, width: 100, height: 100 },
		width: 100,
		height: 100,
	};

	test("encodes the live button mask onto MotionNotify", () => {
		expect(
			mouseMoveToInput(
				{ ...noMods, button: 0, buttons: 1, clientX: 30, clientY: 40 },
				g,
			),
		).toEqual({ kind: "MotionNotify", x: 30, y: 40, state: 0x100 });
	});
});

describe("mouseDownToInput", () => {
	const g: CanvasGeometry = {
		rect: { left: 0, top: 0, width: 100, height: 100 },
		width: 100,
		height: 100,
	};

	test("strips the just-pressed bit so state reflects pre-press", () => {
		// Browser reports buttons=1 *after* a left-click — the X11
		// state on a ButtonPress should report the button set as it
		// was *before* the press (so 0).
		expect(
			mouseDownToInput(
				{ ...noMods, button: 0, buttons: 1, clientX: 10, clientY: 10 },
				g,
			),
		).toEqual({
			kind: "ButtonPress",
			button: 1,
			x: 10,
			y: 10,
			state: 0,
		});
	});

	test("preserves other already-held buttons + modifiers", () => {
		// Right button held, then user presses left. Pre-press state
		// is just right (X11 0x400), and shift is down.
		expect(
			mouseDownToInput(
				{
					...noMods,
					shiftKey: true,
					button: 0,
					buttons: 1 | 2, // left + right
					clientX: 5,
					clientY: 5,
				},
				g,
			),
		).toEqual({
			kind: "ButtonPress",
			button: 1,
			x: 5,
			y: 5,
			state: 0x400 | 0x01,
		});
	});
});

describe("mouseUpToInput", () => {
	const g: CanvasGeometry = {
		rect: { left: 0, top: 0, width: 100, height: 100 },
		width: 100,
		height: 100,
	};

	test("re-adds the just-released bit so state reflects pre-release", () => {
		// Browser reports buttons=0 *after* the release. Pre-release
		// the left button was held → X11 0x100.
		expect(
			mouseUpToInput(
				{ ...noMods, button: 0, buttons: 0, clientX: 0, clientY: 0 },
				g,
			),
		).toEqual({
			kind: "ButtonRelease",
			button: 1,
			x: 0,
			y: 0,
			state: 0x100,
		});
	});
});

describe("keyDownToInput / keyUpToInput", () => {
	test("emits KeyPress for a mapped code", () => {
		expect(
			keyDownToInput({ ...noMods, code: "KeyA", keyCode: 65 }),
		).toEqual({ kind: "KeyPress", keycode: 38, state: 0 });
	});

	test("packs modifier mask onto KeyPress", () => {
		expect(
			keyDownToInput({
				...noMods,
				ctrlKey: true,
				code: "KeyC",
				keyCode: 67,
			}),
		).toEqual({ kind: "KeyPress", keycode: 54, state: 0x04 });
	});

	test("emits KeyRelease for a mapped code", () => {
		expect(
			keyUpToInput({ ...noMods, code: "Enter", keyCode: 13 }),
		).toEqual({ kind: "KeyRelease", keycode: 36, state: 0 });
	});

	test("returns null when the key is unmappable", () => {
		expect(
			keyDownToInput({ ...noMods, code: "", keyCode: 0 }),
		).toBeNull();
		expect(keyUpToInput({ ...noMods, code: "", keyCode: 0 })).toBeNull();
	});

	// Playwright's `keyboard.type(":")` dispatches the colon character
	// with `code="Semicolon"` and `key=":"` but does NOT set shiftKey,
	// so a literal modifierMask read would lose the shift bit and the
	// X server would see `;` instead of `:`. impliedShiftMask() compensates.
	test("infers shift bit from shifted key character even when shiftKey=false", () => {
		expect(
			keyDownToInput({
				...noMods,
				code: "Semicolon",
				key: ":",
				keyCode: 186,
			}),
		).toEqual({ kind: "KeyPress", keycode: 47, state: 0x01 });
	});

	test("does not infer shift when key character is the unshifted variant", () => {
		expect(
			keyDownToInput({
				...noMods,
				code: "Semicolon",
				key: ";",
				keyCode: 186,
			}),
		).toEqual({ kind: "KeyPress", keycode: 47, state: 0 });
	});

	test("infers shift for uppercase letter typed without an explicit Shift event", () => {
		expect(
			keyDownToInput({ ...noMods, code: "KeyA", key: "A", keyCode: 65 }),
		).toEqual({ kind: "KeyPress", keycode: 38, state: 0x01 });
	});
});
