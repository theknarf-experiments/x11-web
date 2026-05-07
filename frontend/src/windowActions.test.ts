// @vitest-environment jsdom
import {
	afterEach,
	beforeEach,
	describe,
	expect,
	test,
	vi,
} from "vitest";
import { act, createElement, useImperativeHandle } from "react";
import { createRoot, type Root } from "react-dom/client";

// biome-ignore lint/suspicious/noExplicitAny: test-only flag
(globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;

// `useWindowActions` calls into `db` and `workspaceSync` for its
// side effects; mock both so we can assert exactly what each
// callback triggers without bringing up the actual collections /
// Automerge docs. `vi.hoisted(...)` lets us define the mock state
// in the same hoisted bucket as `vi.mock`'s factory closure, so
// the test body can still reach the `vi.fn()`s afterwards.
const { dbMocks, workspaceMocks } = vi.hoisted(() => ({
	dbMocks: {
		patchWindow: vi.fn(),
		setFocusedWindow: vi.fn(),
		unminimizeWindow: vi.fn(),
		windowsCollection: { state: new Map() as Map<string, unknown> },
		windowsForProcess: vi.fn<(sidecarId: string, pid: number) => string[]>(),
	},
	workspaceMocks: {
		raiseOcifNode: vi.fn(),
		setOcifNodePosition: vi.fn(),
	},
}));
vi.mock("./db", () => dbMocks);
vi.mock("./workspaceSync", () => workspaceMocks);

// Imported AFTER `vi.mock` so the hook resolves to the stubbed
// modules.
import { useWindowActions } from "./windowActions";
import type { OcifNode } from "./workspaceSync";

interface ActionsHandle {
	actions: ReturnType<typeof useWindowActions>;
}

// biome-ignore lint/suspicious/noExplicitAny: matches the hook's
// `send: (message: any) => void` signature and the `Dispatch`
// shape exactly so the test passes the same types the host does.
interface HostArgs {
	activeWorkspaceId: string | null;
	ocifNodes: Map<string, OcifNode>;
	focusPolicy: "click-to-focus" | "focus-follows-mouse";
	send: (message: any) => void;
	setClosedWindowIds: React.Dispatch<React.SetStateAction<Set<string>>>;
	ref: React.Ref<ActionsHandle>;
}

function Host({ ref, ...args }: HostArgs) {
	const actions = useWindowActions(args);
	useImperativeHandle(ref, () => ({ actions }));
	return null;
}

let root: Root;
let container: HTMLDivElement;
// `vi.fn()` is callable + constructable, which TS sees as a
// union; spread a narrower function type so the prop bag in
// `mount()` matches `Host`'s expected signatures.
// biome-ignore lint/suspicious/noExplicitAny: matches the hook's API
let send: ((m: any) => void) & ReturnType<typeof vi.fn>;
let setClosedWindowIds: React.Dispatch<React.SetStateAction<Set<string>>> &
	ReturnType<typeof vi.fn>;
const handleRef: { current: ActionsHandle | null } = { current: null };

function mount(over: Partial<HostArgs> = {}) {
	send = vi.fn() as typeof send;
	setClosedWindowIds = vi.fn() as typeof setClosedWindowIds;
	container = document.createElement("div");
	document.body.appendChild(container);
	act(() => {
		root = createRoot(container);
		root.render(
			createElement(Host, {
				activeWorkspaceId: "ws-1",
				ocifNodes: new Map(),
				focusPolicy: "click-to-focus",
				send,
				setClosedWindowIds,
				ref: handleRef,
				...over,
			}),
		);
	});
	if (!handleRef.current) throw new Error("ref not set");
	return handleRef.current.actions;
}

beforeEach(() => {
	dbMocks.patchWindow.mockClear();
	dbMocks.setFocusedWindow.mockClear();
	dbMocks.unminimizeWindow.mockClear();
	dbMocks.windowsForProcess.mockClear();
	dbMocks.windowsForProcess.mockReturnValue([]);
	dbMocks.windowsCollection.state.clear();
	workspaceMocks.raiseOcifNode.mockClear();
	workspaceMocks.setOcifNodePosition.mockClear();
});
afterEach(() => {
	act(() => root.unmount());
	document.body.removeChild(container);
	vi.useRealTimers();
});

describe("handleMove", () => {
	test("moves a top-level window via setOcifNodePosition", () => {
		const a = mount();
		dbMocks.windowsCollection.state.set("w1", { overrideRedirect: false });
		act(() => a.handleMove("w1", 100, 200));
		expect(workspaceMocks.setOcifNodePosition).toHaveBeenCalledWith(
			"ws-1",
			"w1",
			100,
			200,
		);
	});

	test("ignores override-redirect (popup) windows", () => {
		const a = mount();
		dbMocks.windowsCollection.state.set("popup", { overrideRedirect: true });
		act(() => a.handleMove("popup", 5, 6));
		expect(workspaceMocks.setOcifNodePosition).not.toHaveBeenCalled();
	});

	test("no-op when no active workspace", () => {
		const a = mount({ activeWorkspaceId: null });
		act(() => a.handleMove("w1", 0, 0));
		expect(workspaceMocks.setOcifNodePosition).not.toHaveBeenCalled();
	});
});

describe("handleResize", () => {
	test("debounces rapid resize events into a single ResizeWindow send", () => {
		vi.useFakeTimers();
		const a = mount();
		act(() => {
			a.handleResize("w1", "x11", 100, 100);
			a.handleResize("w1", "x11", 200, 200);
			a.handleResize("w1", "x11", 300, 300);
		});
		expect(send).not.toHaveBeenCalled();
		vi.advanceTimersByTime(100);
		expect(send).toHaveBeenCalledTimes(1);
		expect(send).toHaveBeenCalledWith({
			type: "ResizeWindow",
			sidecar_id: "x11",
			window_id: "w1",
			width: 300,
			height: 300,
		});
	});
});

describe("handleFocus", () => {
	test("raises the node, unminimizes, and sets local focus", () => {
		const a = mount();
		act(() => a.handleFocus("w1"));
		expect(workspaceMocks.raiseOcifNode).toHaveBeenCalledWith("ws-1", "w1");
		expect(dbMocks.unminimizeWindow).toHaveBeenCalledWith("w1");
		expect(dbMocks.setFocusedWindow).toHaveBeenCalledWith("w1");
	});

	test("still sets local focus when there's no workspace (macOS sidecar path)", () => {
		const a = mount({ activeWorkspaceId: null });
		act(() => a.handleFocus("w1"));
		expect(workspaceMocks.raiseOcifNode).not.toHaveBeenCalled();
		expect(dbMocks.setFocusedWindow).toHaveBeenCalledWith("w1");
	});
});

describe("handleInput", () => {
	test("envelopes the InputEvent and forwards via send", () => {
		const a = mount();
		const event = { kind: "KeyPress" as const, keycode: 38, state: 0 };
		act(() => a.handleInput("w1", "x11", event));
		expect(send).toHaveBeenCalledWith({
			type: "InputEvent",
			sidecar_id: "x11",
			window_id: "w1",
			event,
		});
	});
});

describe("handleCloseProcess", () => {
	test("locally hides matching window ids and sends KillProcess", () => {
		const a = mount();
		dbMocks.windowsForProcess.mockReturnValue(["w1", "w2"]);
		act(() => a.handleCloseProcess("x11", 42));
		// setClosedWindowIds was called with an updater fn — invoke
		// it against an empty set to verify it merges in `w1` + `w2`.
		const updater = setClosedWindowIds.mock.calls[0][0] as (
			prev: Set<string>,
		) => Set<string>;
		expect([...updater(new Set())]).toEqual(["w1", "w2"]);
		expect(send).toHaveBeenCalledWith(
			expect.objectContaining({
				type: "KillProcess",
				sidecar_id: "x11",
				pid: 42,
			}),
		);
	});

	test("doesn't touch closedWindowIds when there are no matching windows", () => {
		const a = mount();
		dbMocks.windowsForProcess.mockReturnValue([]);
		act(() => a.handleCloseProcess("x11", 7));
		expect(setClosedWindowIds).not.toHaveBeenCalled();
		expect(send).toHaveBeenCalledWith(
			expect.objectContaining({ type: "KillProcess" }),
		);
	});
});

describe("handleMinimize / handleMaximize / handleCloseWindow", () => {
	test("Minimize patches state + sends WindowManage 'minimized'", () => {
		const a = mount();
		act(() => a.handleMinimize("w1", "x11"));
		expect(dbMocks.patchWindow).toHaveBeenCalledWith("w1", {
			wmState: "minimized",
		});
		expect(send).toHaveBeenCalledWith(
			expect.objectContaining({
				event: { kind: "WindowManage", action: "minimized" },
			}),
		);
	});

	test("Maximize stashes the pre-maximize OcifNode position", () => {
		const ocifNodes = new Map<string, OcifNode>();
		ocifNodes.set("w1", { x: 50, y: 60, z: 1, width: 100, height: 80 });
		const a = mount({ ocifNodes });
		act(() => a.handleMaximize("w1", "x11"));
		expect(dbMocks.patchWindow).toHaveBeenCalledWith("w1", {
			wmState: "maximized",
			savedPosition: { x: 50, y: 60 },
		});
	});

	test("Maximize without an OcifNode skips savedPosition", () => {
		const a = mount();
		act(() => a.handleMaximize("w1", "x11"));
		expect(dbMocks.patchWindow).toHaveBeenCalledWith("w1", {
			wmState: "maximized",
		});
	});

	test("CloseWindow sends WindowManage 'close' without local mutation", () => {
		const a = mount();
		act(() => a.handleCloseWindow("w1", "x11"));
		expect(dbMocks.patchWindow).not.toHaveBeenCalled();
		expect(send).toHaveBeenCalledWith(
			expect.objectContaining({
				event: { kind: "WindowManage", action: "close" },
			}),
		);
	});
});

describe("handleRestore", () => {
	test("normalizes + restores savedPosition + sends WindowManage 'normal'", () => {
		const a = mount();
		dbMocks.windowsCollection.state.set("w1", {
			savedPosition: { x: 11, y: 22 },
		});
		act(() => a.handleRestore("w1", "x11"));
		expect(dbMocks.patchWindow).toHaveBeenCalledWith("w1", {
			wmState: "normal",
		});
		expect(workspaceMocks.setOcifNodePosition).toHaveBeenCalledWith(
			"ws-1",
			"w1",
			11,
			22,
		);
		expect(send).toHaveBeenCalledWith(
			expect.objectContaining({
				event: { kind: "WindowManage", action: "normal" },
			}),
		);
	});

	test("skips position restore when savedPosition is missing", () => {
		const a = mount();
		dbMocks.windowsCollection.state.set("w1", {});
		act(() => a.handleRestore("w1", "x11"));
		expect(workspaceMocks.setOcifNodePosition).not.toHaveBeenCalled();
		// Still sends WindowManage normal.
		expect(send).toHaveBeenCalledWith(
			expect.objectContaining({
				event: { kind: "WindowManage", action: "normal" },
			}),
		);
	});

	test("no-op when no active workspace", () => {
		const a = mount({ activeWorkspaceId: null });
		act(() => a.handleRestore("w1", "x11"));
		expect(dbMocks.patchWindow).not.toHaveBeenCalled();
		expect(send).not.toHaveBeenCalled();
	});
});

describe("handleMouseEnterWindow", () => {
	test("under click-to-focus policy: no-op", () => {
		const a = mount({ focusPolicy: "click-to-focus" });
		act(() => a.handleMouseEnterWindow("w1"));
		expect(workspaceMocks.raiseOcifNode).not.toHaveBeenCalled();
		expect(dbMocks.setFocusedWindow).not.toHaveBeenCalled();
	});

	test("under focus-follows-mouse policy: raises + focuses", () => {
		const a = mount({ focusPolicy: "focus-follows-mouse" });
		act(() => a.handleMouseEnterWindow("w1"));
		expect(workspaceMocks.raiseOcifNode).toHaveBeenCalledWith("ws-1", "w1");
		expect(dbMocks.setFocusedWindow).toHaveBeenCalledWith("w1");
	});
});
