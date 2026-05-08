import * as $ from "capnp-es";
export declare const _capnpFileId = 13889381496258476561n;
export declare const FrontendMsg_Payload_Which: {
  readonly NO_VARIANT: 0;
  readonly OPEN_WORKSPACE: 1;
  readonly SPAWN_PROCESS: 2;
  readonly KILL_PROCESS: 3;
  readonly INPUT_EVENT: 4;
  readonly RESIZE_WINDOW: 5;
  readonly RTC_OFFER: 6;
  readonly RTC_ICE_CANDIDATE: 7;
};
export type FrontendMsg_Payload_Which = (typeof FrontendMsg_Payload_Which)[keyof typeof FrontendMsg_Payload_Which];
/**
* Default for unknown / unset variants — gives readers a
* safe fallback when a future schema variant lands. Cap'n
* Proto unions need at least two members.
*
*/
export declare class FrontendMsg_Payload extends $.Struct {
  static readonly NO_VARIANT: 0;
  static readonly OPEN_WORKSPACE: 1;
  static readonly SPAWN_PROCESS: 2;
  static readonly KILL_PROCESS: 3;
  static readonly INPUT_EVENT: 4;
  static readonly RESIZE_WINDOW: 5;
  static readonly RTC_OFFER: 6;
  static readonly RTC_ICE_CANDIDATE: 7;
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get _isNoVariant(): boolean;
  set noVariant(_: true);
  _adoptOpenWorkspace(value: $.Orphan<OpenWorkspace>): void;
  _disownOpenWorkspace(): $.Orphan<OpenWorkspace>;
  get openWorkspace(): OpenWorkspace;
  _hasOpenWorkspace(): boolean;
  _initOpenWorkspace(): OpenWorkspace;
  get _isOpenWorkspace(): boolean;
  set openWorkspace(value: OpenWorkspace);
  _adoptSpawnProcess(value: $.Orphan<SpawnProcess>): void;
  _disownSpawnProcess(): $.Orphan<SpawnProcess>;
  get spawnProcess(): SpawnProcess;
  _hasSpawnProcess(): boolean;
  _initSpawnProcess(): SpawnProcess;
  get _isSpawnProcess(): boolean;
  set spawnProcess(value: SpawnProcess);
  _adoptKillProcess(value: $.Orphan<KillProcess>): void;
  _disownKillProcess(): $.Orphan<KillProcess>;
  get killProcess(): KillProcess;
  _hasKillProcess(): boolean;
  _initKillProcess(): KillProcess;
  get _isKillProcess(): boolean;
  set killProcess(value: KillProcess);
  _adoptInputEvent(value: $.Orphan<InputEventCmd>): void;
  _disownInputEvent(): $.Orphan<InputEventCmd>;
  get inputEvent(): InputEventCmd;
  _hasInputEvent(): boolean;
  _initInputEvent(): InputEventCmd;
  get _isInputEvent(): boolean;
  set inputEvent(value: InputEventCmd);
  _adoptResizeWindow(value: $.Orphan<ResizeWindowCmd>): void;
  _disownResizeWindow(): $.Orphan<ResizeWindowCmd>;
  get resizeWindow(): ResizeWindowCmd;
  _hasResizeWindow(): boolean;
  _initResizeWindow(): ResizeWindowCmd;
  get _isResizeWindow(): boolean;
  set resizeWindow(value: ResizeWindowCmd);
  _adoptRtcOffer(value: $.Orphan<RtcSdp>): void;
  _disownRtcOffer(): $.Orphan<RtcSdp>;
  get rtcOffer(): RtcSdp;
  _hasRtcOffer(): boolean;
  _initRtcOffer(): RtcSdp;
  get _isRtcOffer(): boolean;
  set rtcOffer(value: RtcSdp);
  _adoptRtcIceCandidate(value: $.Orphan<RtcIceCandidate>): void;
  _disownRtcIceCandidate(): $.Orphan<RtcIceCandidate>;
  get rtcIceCandidate(): RtcIceCandidate;
  _hasRtcIceCandidate(): boolean;
  _initRtcIceCandidate(): RtcIceCandidate;
  get _isRtcIceCandidate(): boolean;
  set rtcIceCandidate(value: RtcIceCandidate);
  toString(): string;
  which(): FrontendMsg_Payload_Which;
}
/**
* W3C Trace Context. Empty string when OTel is disabled or
* when there is no active span on the sender.
*
*/
export declare class FrontendMsg extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get traceparent(): string;
  set traceparent(value: string);
  /**
* Default for unknown / unset variants — gives readers a
* safe fallback when a future schema variant lands. Cap'n
* Proto unions need at least two members.
*
*/
  get payload(): FrontendMsg_Payload;
  _initPayload(): FrontendMsg_Payload;
  toString(): string;
}
export declare class OpenWorkspace extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get id(): string;
  set id(value: string);
  toString(): string;
}
export declare class SpawnProcess extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get requestId(): string;
  set requestId(value: string);
  get sidecarId(): string;
  set sidecarId(value: string);
  get workspaceId(): string;
  set workspaceId(value: string);
  get command(): string;
  set command(value: string);
  _adoptArgs(value: $.Orphan<$.List<string>>): void;
  _disownArgs(): $.Orphan<$.List<string>>;
  get args(): $.List<string>;
  _hasArgs(): boolean;
  _initArgs(length: number): $.List<string>;
  set args(value: $.List<string>);
  toString(): string;
}
export declare class KillProcess extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get requestId(): string;
  set requestId(value: string);
  get sidecarId(): string;
  set sidecarId(value: string);
  get pid(): number;
  set pid(value: number);
  toString(): string;
}
export declare class InputEventCmd extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get sidecarId(): string;
  set sidecarId(value: string);
  get windowId(): string;
  set windowId(value: string);
  _adoptEvent(value: $.Orphan<InputEvent>): void;
  _disownEvent(): $.Orphan<InputEvent>;
  get event(): InputEvent;
  _hasEvent(): boolean;
  _initEvent(): InputEvent;
  set event(value: InputEvent);
  toString(): string;
}
export declare class ResizeWindowCmd extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get sidecarId(): string;
  set sidecarId(value: string);
  get windowId(): string;
  set windowId(value: string);
  get width(): number;
  set width(value: number);
  get height(): number;
  set height(value: number);
  toString(): string;
}
export declare class RtcSdp extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get sdp(): string;
  set sdp(value: string);
  toString(): string;
}
export declare class RtcIceCandidate extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get candidate(): string;
  set candidate(value: string);
  get sdpMid(): string;
  set sdpMid(value: string);
  get sdpMlineIndexHas(): boolean;
  set sdpMlineIndexHas(value: boolean);
  get sdpMlineIndex(): number;
  set sdpMlineIndex(value: number);
  toString(): string;
}
export declare const BackendMsg_Payload_Which: {
  readonly NO_VARIANT: 0;
  readonly SIDECAR_LIST: 1;
  readonly WORKSPACE: 2;
  readonly COMMAND_RESULT: 3;
  readonly PROCESS_LIST: 4;
  readonly WINDOW_UPDATE: 5;
  readonly WINDOW_LIST: 6;
  readonly BELL: 7;
  readonly RTC_ANSWER: 8;
  readonly RTC_ICE_CANDIDATE: 9;
};
export type BackendMsg_Payload_Which = (typeof BackendMsg_Payload_Which)[keyof typeof BackendMsg_Payload_Which];
export declare class BackendMsg_Payload extends $.Struct {
  static readonly NO_VARIANT: 0;
  static readonly SIDECAR_LIST: 1;
  static readonly WORKSPACE: 2;
  static readonly COMMAND_RESULT: 3;
  static readonly PROCESS_LIST: 4;
  static readonly WINDOW_UPDATE: 5;
  static readonly WINDOW_LIST: 6;
  static readonly BELL: 7;
  static readonly RTC_ANSWER: 8;
  static readonly RTC_ICE_CANDIDATE: 9;
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get _isNoVariant(): boolean;
  set noVariant(_: true);
  _adoptSidecarList(value: $.Orphan<SidecarListMsg>): void;
  _disownSidecarList(): $.Orphan<SidecarListMsg>;
  get sidecarList(): SidecarListMsg;
  _hasSidecarList(): boolean;
  _initSidecarList(): SidecarListMsg;
  get _isSidecarList(): boolean;
  set sidecarList(value: SidecarListMsg);
  _adoptWorkspace(value: $.Orphan<WorkspaceMsg>): void;
  _disownWorkspace(): $.Orphan<WorkspaceMsg>;
  get workspace(): WorkspaceMsg;
  _hasWorkspace(): boolean;
  _initWorkspace(): WorkspaceMsg;
  get _isWorkspace(): boolean;
  set workspace(value: WorkspaceMsg);
  _adoptCommandResult(value: $.Orphan<CommandResult>): void;
  _disownCommandResult(): $.Orphan<CommandResult>;
  get commandResult(): CommandResult;
  _hasCommandResult(): boolean;
  _initCommandResult(): CommandResult;
  get _isCommandResult(): boolean;
  set commandResult(value: CommandResult);
  _adoptProcessList(value: $.Orphan<ProcessListMsg>): void;
  _disownProcessList(): $.Orphan<ProcessListMsg>;
  get processList(): ProcessListMsg;
  _hasProcessList(): boolean;
  _initProcessList(): ProcessListMsg;
  get _isProcessList(): boolean;
  set processList(value: ProcessListMsg);
  _adoptWindowUpdate(value: $.Orphan<WindowUpdateMsg>): void;
  _disownWindowUpdate(): $.Orphan<WindowUpdateMsg>;
  get windowUpdate(): WindowUpdateMsg;
  _hasWindowUpdate(): boolean;
  _initWindowUpdate(): WindowUpdateMsg;
  get _isWindowUpdate(): boolean;
  set windowUpdate(value: WindowUpdateMsg);
  _adoptWindowList(value: $.Orphan<WindowListMsg>): void;
  _disownWindowList(): $.Orphan<WindowListMsg>;
  get windowList(): WindowListMsg;
  _hasWindowList(): boolean;
  _initWindowList(): WindowListMsg;
  get _isWindowList(): boolean;
  set windowList(value: WindowListMsg);
  _adoptBell(value: $.Orphan<Bell>): void;
  _disownBell(): $.Orphan<Bell>;
  get bell(): Bell;
  _hasBell(): boolean;
  _initBell(): Bell;
  get _isBell(): boolean;
  set bell(value: Bell);
  _adoptRtcAnswer(value: $.Orphan<RtcSdp>): void;
  _disownRtcAnswer(): $.Orphan<RtcSdp>;
  get rtcAnswer(): RtcSdp;
  _hasRtcAnswer(): boolean;
  _initRtcAnswer(): RtcSdp;
  get _isRtcAnswer(): boolean;
  set rtcAnswer(value: RtcSdp);
  _adoptRtcIceCandidate(value: $.Orphan<RtcIceCandidate>): void;
  _disownRtcIceCandidate(): $.Orphan<RtcIceCandidate>;
  get rtcIceCandidate(): RtcIceCandidate;
  _hasRtcIceCandidate(): boolean;
  _initRtcIceCandidate(): RtcIceCandidate;
  get _isRtcIceCandidate(): boolean;
  set rtcIceCandidate(value: RtcIceCandidate);
  toString(): string;
  which(): BackendMsg_Payload_Which;
}
export declare class BackendMsg extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get traceparent(): string;
  set traceparent(value: string);
  get payload(): BackendMsg_Payload;
  _initPayload(): BackendMsg_Payload;
  toString(): string;
}
export declare class SidecarListMsg extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  static _Sidecars: $.ListCtor<SidecarInfo>;
  _adoptSidecars(value: $.Orphan<$.List<SidecarInfo>>): void;
  _disownSidecars(): $.Orphan<$.List<SidecarInfo>>;
  get sidecars(): $.List<SidecarInfo>;
  _hasSidecars(): boolean;
  _initSidecars(length: number): $.List<SidecarInfo>;
  set sidecars(value: $.List<SidecarInfo>);
  toString(): string;
}
export declare class SidecarInfo extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get id(): string;
  set id(value: string);
  get name(): string;
  set name(value: string);
  toString(): string;
}
export declare class WorkspaceMsg extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  _adoptWorkspace(value: $.Orphan<Workspace>): void;
  _disownWorkspace(): $.Orphan<Workspace>;
  get workspace(): Workspace;
  _hasWorkspace(): boolean;
  _initWorkspace(): Workspace;
  set workspace(value: Workspace);
  toString(): string;
}
export declare class Workspace extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get id(): string;
  set id(value: string);
  get name(): string;
  set name(value: string);
  toString(): string;
}
export declare class CommandResult extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get requestId(): string;
  set requestId(value: string);
  get success(): boolean;
  set success(value: boolean);
  get message(): string;
  set message(value: string);
  toString(): string;
}
export declare class ProcessListMsg extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  static _Processes: $.ListCtor<ProcessInfo>;
  get sidecarId(): string;
  set sidecarId(value: string);
  _adoptProcesses(value: $.Orphan<$.List<ProcessInfo>>): void;
  _disownProcesses(): $.Orphan<$.List<ProcessInfo>>;
  get processes(): $.List<ProcessInfo>;
  _hasProcesses(): boolean;
  _initProcesses(length: number): $.List<ProcessInfo>;
  set processes(value: $.List<ProcessInfo>);
  toString(): string;
}
export declare class ProcessInfo extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get pid(): number;
  set pid(value: number);
  get clientId(): string;
  set clientId(value: string);
  get command(): string;
  set command(value: string);
  toString(): string;
}
export declare class WindowUpdateMsg extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  _adoptUpdate(value: $.Orphan<WindowUpdate>): void;
  _disownUpdate(): $.Orphan<WindowUpdate>;
  get update(): WindowUpdate;
  _hasUpdate(): boolean;
  _initUpdate(): WindowUpdate;
  set update(value: WindowUpdate);
  toString(): string;
}
export declare const WindowUpdate_Which: {
  readonly NO_VARIANT: 0;
  readonly TITLE_CHANGED: 1;
  readonly STATE_CHANGED: 2;
  readonly FOCUSED: 3;
  readonly MENU_STRUCTURE: 4;
};
export type WindowUpdate_Which = (typeof WindowUpdate_Which)[keyof typeof WindowUpdate_Which];
export declare class WindowUpdate extends $.Struct {
  static readonly NO_VARIANT: 0;
  static readonly TITLE_CHANGED: 1;
  static readonly STATE_CHANGED: 2;
  static readonly FOCUSED: 3;
  static readonly MENU_STRUCTURE: 4;
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get _isNoVariant(): boolean;
  set noVariant(_: true);
  _adoptTitleChanged(value: $.Orphan<TitleChanged>): void;
  _disownTitleChanged(): $.Orphan<TitleChanged>;
  get titleChanged(): TitleChanged;
  _hasTitleChanged(): boolean;
  _initTitleChanged(): TitleChanged;
  get _isTitleChanged(): boolean;
  set titleChanged(value: TitleChanged);
  _adoptStateChanged(value: $.Orphan<StateChanged>): void;
  _disownStateChanged(): $.Orphan<StateChanged>;
  get stateChanged(): StateChanged;
  _hasStateChanged(): boolean;
  _initStateChanged(): StateChanged;
  get _isStateChanged(): boolean;
  set stateChanged(value: StateChanged);
  _adoptFocused(value: $.Orphan<Focused>): void;
  _disownFocused(): $.Orphan<Focused>;
  get focused(): Focused;
  _hasFocused(): boolean;
  _initFocused(): Focused;
  get _isFocused(): boolean;
  set focused(value: Focused);
  _adoptMenuStructure(value: $.Orphan<MenuStructure>): void;
  _disownMenuStructure(): $.Orphan<MenuStructure>;
  get menuStructure(): MenuStructure;
  _hasMenuStructure(): boolean;
  _initMenuStructure(): MenuStructure;
  get _isMenuStructure(): boolean;
  set menuStructure(value: MenuStructure);
  toString(): string;
  which(): WindowUpdate_Which;
}
export declare class TitleChanged extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get windowId(): string;
  set windowId(value: string);
  get title(): string;
  set title(value: string);
  toString(): string;
}
export declare class StateChanged extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get windowId(): string;
  set windowId(value: string);
  get state(): WindowWmState;
  set state(value: WindowWmState);
  toString(): string;
}
export declare class Focused extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get windowId(): string;
  set windowId(value: string);
  toString(): string;
}
export declare class MenuStructure extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  static _Items: $.ListCtor<MenuItem>;
  get windowId(): string;
  set windowId(value: string);
  _adoptItems(value: $.Orphan<$.List<MenuItem>>): void;
  _disownItems(): $.Orphan<$.List<MenuItem>>;
  get items(): $.List<MenuItem>;
  _hasItems(): boolean;
  _initItems(length: number): $.List<MenuItem>;
  set items(value: $.List<MenuItem>);
  toString(): string;
}
export declare const WindowWmState: {
  readonly NORMAL: 0;
  readonly MINIMIZED: 1;
  readonly MAXIMIZED: 2;
  readonly FULLSCREEN: 3;
  readonly CLOSE: 4;
};
export type WindowWmState = (typeof WindowWmState)[keyof typeof WindowWmState];
export declare class WindowListMsg extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  static _Windows: $.ListCtor<WindowDescriptor>;
  _adoptWindows(value: $.Orphan<$.List<WindowDescriptor>>): void;
  _disownWindows(): $.Orphan<$.List<WindowDescriptor>>;
  get windows(): $.List<WindowDescriptor>;
  _hasWindows(): boolean;
  _initWindows(length: number): $.List<WindowDescriptor>;
  set windows(value: $.List<WindowDescriptor>);
  toString(): string;
}
export declare class WindowDescriptor extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get windowId(): string;
  set windowId(value: string);
  get sidecarId(): string;
  set sidecarId(value: string);
  get pid(): number;
  set pid(value: number);
  get command(): string;
  set command(value: string);
  get x(): number;
  set x(value: number);
  get y(): number;
  set y(value: number);
  get width(): number;
  set width(value: number);
  get height(): number;
  set height(value: number);
  get borderWidth(): number;
  set borderWidth(value: number);
  get borderPixel(): number;
  set borderPixel(value: number);
  get overrideRedirect(): boolean;
  set overrideRedirect(value: boolean);
  get resizable(): boolean;
  set resizable(value: boolean);
  toString(): string;
}
export declare class Bell extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get percent(): number;
  set percent(value: number);
  toString(): string;
}
export declare class MenuItem extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  static _Children: $.ListCtor<MenuItem>;
  get id(): string;
  set id(value: string);
  get label(): string;
  set label(value: string);
  get kind(): MenuItemKind;
  set kind(value: MenuItemKind);
  get enabled(): boolean;
  set enabled(value: boolean);
  get visible(): boolean;
  set visible(value: boolean);
  get checked(): CheckState;
  set checked(value: CheckState);
  get accelerator(): string;
  set accelerator(value: string);
  get icon(): string;
  set icon(value: string);
  _adoptAction(value: $.Orphan<MenuAction>): void;
  _disownAction(): $.Orphan<MenuAction>;
  get action(): MenuAction;
  _hasAction(): boolean;
  _initAction(): MenuAction;
  set action(value: MenuAction);
  _adoptChildren(value: $.Orphan<$.List<MenuItem>>): void;
  _disownChildren(): $.Orphan<$.List<MenuItem>>;
  get children(): $.List<MenuItem>;
  _hasChildren(): boolean;
  _initChildren(length: number): $.List<MenuItem>;
  set children(value: $.List<MenuItem>);
  toString(): string;
}
export declare const MenuItemKind: {
  readonly NORMAL: 0;
  readonly SUBMENU: 1;
  readonly SEPARATOR: 2;
  readonly CHECKBOX: 3;
  readonly RADIO: 4;
};
export type MenuItemKind = (typeof MenuItemKind)[keyof typeof MenuItemKind];
export declare const CheckState: {
  readonly NOT_APPLICABLE: 0;
  readonly UNCHECKED: 1;
  readonly CHECKED: 2;
};
export type CheckState = (typeof CheckState)[keyof typeof CheckState];
export declare class MenuAction extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get name(): string;
  set name(value: string);
  _adoptTarget(value: $.Orphan<MenuActionTarget>): void;
  _disownTarget(): $.Orphan<MenuActionTarget>;
  /**
* absent via `has_target()`
*
*/
  get target(): MenuActionTarget;
  _hasTarget(): boolean;
  _initTarget(): MenuActionTarget;
  set target(value: MenuActionTarget);
  toString(): string;
}
export declare const MenuActionTarget_Which: {
  readonly STRING: 0;
  readonly BOOLEAN: 1;
  readonly INT32: 2;
  readonly U_INT32: 3;
  readonly INT64: 4;
  readonly FLOAT64: 5;
};
export type MenuActionTarget_Which = (typeof MenuActionTarget_Which)[keyof typeof MenuActionTarget_Which];
export declare class MenuActionTarget extends $.Struct {
  static readonly STRING: 0;
  static readonly BOOLEAN: 1;
  static readonly INT32: 2;
  static readonly U_INT32: 3;
  static readonly INT64: 4;
  static readonly FLOAT64: 5;
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get string(): string;
  get _isString(): boolean;
  set string(value: string);
  get boolean(): boolean;
  get _isBoolean(): boolean;
  set boolean(value: boolean);
  get int32(): number;
  get _isInt32(): boolean;
  set int32(value: number);
  get uInt32(): number;
  get _isUInt32(): boolean;
  set uInt32(value: number);
  get int64(): bigint;
  get _isInt64(): boolean;
  set int64(value: bigint);
  get float64(): number;
  get _isFloat64(): boolean;
  set float64(value: number);
  toString(): string;
  which(): MenuActionTarget_Which;
}
export declare const InputEvent_Payload_Which: {
  readonly NO_VARIANT: 0;
  readonly KEY_PRESS: 1;
  readonly KEY_RELEASE: 2;
  readonly BUTTON_PRESS: 3;
  readonly BUTTON_RELEASE: 4;
  readonly MOTION_NOTIFY: 5;
  readonly MENU_ACTIVATE: 6;
  readonly WINDOW_MANAGE: 7;
  readonly DND_BRIDGE: 8;
  readonly TOUCH_BEGIN: 9;
  readonly TOUCH_UPDATE: 10;
  readonly TOUCH_END: 11;
  readonly GESTURE_SWIPE: 12;
  readonly GESTURE_PINCH: 13;
  readonly COMPOSITION_EVENT: 14;
};
export type InputEvent_Payload_Which = (typeof InputEvent_Payload_Which)[keyof typeof InputEvent_Payload_Which];
export declare class InputEvent_Payload extends $.Struct {
  static readonly NO_VARIANT: 0;
  static readonly KEY_PRESS: 1;
  static readonly KEY_RELEASE: 2;
  static readonly BUTTON_PRESS: 3;
  static readonly BUTTON_RELEASE: 4;
  static readonly MOTION_NOTIFY: 5;
  static readonly MENU_ACTIVATE: 6;
  static readonly WINDOW_MANAGE: 7;
  static readonly DND_BRIDGE: 8;
  static readonly TOUCH_BEGIN: 9;
  static readonly TOUCH_UPDATE: 10;
  static readonly TOUCH_END: 11;
  static readonly GESTURE_SWIPE: 12;
  static readonly GESTURE_PINCH: 13;
  static readonly COMPOSITION_EVENT: 14;
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get _isNoVariant(): boolean;
  set noVariant(_: true);
  _adoptKeyPress(value: $.Orphan<KeyEvent>): void;
  _disownKeyPress(): $.Orphan<KeyEvent>;
  get keyPress(): KeyEvent;
  _hasKeyPress(): boolean;
  _initKeyPress(): KeyEvent;
  get _isKeyPress(): boolean;
  set keyPress(value: KeyEvent);
  _adoptKeyRelease(value: $.Orphan<KeyEvent>): void;
  _disownKeyRelease(): $.Orphan<KeyEvent>;
  get keyRelease(): KeyEvent;
  _hasKeyRelease(): boolean;
  _initKeyRelease(): KeyEvent;
  get _isKeyRelease(): boolean;
  set keyRelease(value: KeyEvent);
  _adoptButtonPress(value: $.Orphan<ButtonEvent>): void;
  _disownButtonPress(): $.Orphan<ButtonEvent>;
  get buttonPress(): ButtonEvent;
  _hasButtonPress(): boolean;
  _initButtonPress(): ButtonEvent;
  get _isButtonPress(): boolean;
  set buttonPress(value: ButtonEvent);
  _adoptButtonRelease(value: $.Orphan<ButtonEvent>): void;
  _disownButtonRelease(): $.Orphan<ButtonEvent>;
  get buttonRelease(): ButtonEvent;
  _hasButtonRelease(): boolean;
  _initButtonRelease(): ButtonEvent;
  get _isButtonRelease(): boolean;
  set buttonRelease(value: ButtonEvent);
  _adoptMotionNotify(value: $.Orphan<MotionEvent>): void;
  _disownMotionNotify(): $.Orphan<MotionEvent>;
  get motionNotify(): MotionEvent;
  _hasMotionNotify(): boolean;
  _initMotionNotify(): MotionEvent;
  get _isMotionNotify(): boolean;
  set motionNotify(value: MotionEvent);
  _adoptMenuActivate(value: $.Orphan<MenuActivateEvt>): void;
  _disownMenuActivate(): $.Orphan<MenuActivateEvt>;
  get menuActivate(): MenuActivateEvt;
  _hasMenuActivate(): boolean;
  _initMenuActivate(): MenuActivateEvt;
  get _isMenuActivate(): boolean;
  set menuActivate(value: MenuActivateEvt);
  _adoptWindowManage(value: $.Orphan<WindowManageEvt>): void;
  _disownWindowManage(): $.Orphan<WindowManageEvt>;
  get windowManage(): WindowManageEvt;
  _hasWindowManage(): boolean;
  _initWindowManage(): WindowManageEvt;
  get _isWindowManage(): boolean;
  set windowManage(value: WindowManageEvt);
  _adoptDndBridge(value: $.Orphan<DndBridgeEvt>): void;
  _disownDndBridge(): $.Orphan<DndBridgeEvt>;
  get dndBridge(): DndBridgeEvt;
  _hasDndBridge(): boolean;
  _initDndBridge(): DndBridgeEvt;
  get _isDndBridge(): boolean;
  set dndBridge(value: DndBridgeEvt);
  _adoptTouchBegin(value: $.Orphan<TouchEvent>): void;
  _disownTouchBegin(): $.Orphan<TouchEvent>;
  get touchBegin(): TouchEvent;
  _hasTouchBegin(): boolean;
  _initTouchBegin(): TouchEvent;
  get _isTouchBegin(): boolean;
  set touchBegin(value: TouchEvent);
  _adoptTouchUpdate(value: $.Orphan<TouchEvent>): void;
  _disownTouchUpdate(): $.Orphan<TouchEvent>;
  get touchUpdate(): TouchEvent;
  _hasTouchUpdate(): boolean;
  _initTouchUpdate(): TouchEvent;
  get _isTouchUpdate(): boolean;
  set touchUpdate(value: TouchEvent);
  _adoptTouchEnd(value: $.Orphan<TouchEvent>): void;
  _disownTouchEnd(): $.Orphan<TouchEvent>;
  get touchEnd(): TouchEvent;
  _hasTouchEnd(): boolean;
  _initTouchEnd(): TouchEvent;
  get _isTouchEnd(): boolean;
  set touchEnd(value: TouchEvent);
  _adoptGestureSwipe(value: $.Orphan<GestureSwipeEvt>): void;
  _disownGestureSwipe(): $.Orphan<GestureSwipeEvt>;
  get gestureSwipe(): GestureSwipeEvt;
  _hasGestureSwipe(): boolean;
  _initGestureSwipe(): GestureSwipeEvt;
  get _isGestureSwipe(): boolean;
  set gestureSwipe(value: GestureSwipeEvt);
  _adoptGesturePinch(value: $.Orphan<GesturePinchEvt>): void;
  _disownGesturePinch(): $.Orphan<GesturePinchEvt>;
  get gesturePinch(): GesturePinchEvt;
  _hasGesturePinch(): boolean;
  _initGesturePinch(): GesturePinchEvt;
  get _isGesturePinch(): boolean;
  set gesturePinch(value: GesturePinchEvt);
  _adoptCompositionEvent(value: $.Orphan<CompositionEvt>): void;
  _disownCompositionEvent(): $.Orphan<CompositionEvt>;
  get compositionEvent(): CompositionEvt;
  _hasCompositionEvent(): boolean;
  _initCompositionEvent(): CompositionEvt;
  get _isCompositionEvent(): boolean;
  set compositionEvent(value: CompositionEvt);
  toString(): string;
  which(): InputEvent_Payload_Which;
}
export declare class InputEvent extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get payload(): InputEvent_Payload;
  _initPayload(): InputEvent_Payload;
  toString(): string;
}
export declare class KeyEvent extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get keycode(): number;
  set keycode(value: number);
  get state(): number;
  set state(value: number);
  toString(): string;
}
export declare class ButtonEvent extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get button(): number;
  set button(value: number);
  get x(): number;
  set x(value: number);
  get y(): number;
  set y(value: number);
  get state(): number;
  set state(value: number);
  toString(): string;
}
export declare class MotionEvent extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get x(): number;
  set x(value: number);
  get y(): number;
  set y(value: number);
  get state(): number;
  set state(value: number);
  toString(): string;
}
export declare class MenuActivateEvt extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  _adoptAction(value: $.Orphan<MenuAction>): void;
  _disownAction(): $.Orphan<MenuAction>;
  get action(): MenuAction;
  _hasAction(): boolean;
  _initAction(): MenuAction;
  set action(value: MenuAction);
  toString(): string;
}
export declare class WindowManageEvt extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get action(): WindowWmState;
  set action(value: WindowWmState);
  toString(): string;
}
export declare class DndBridgeEvt extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  _adoptEvent(value: $.Orphan<DndEvent>): void;
  _disownEvent(): $.Orphan<DndEvent>;
  get event(): DndEvent;
  _hasEvent(): boolean;
  _initEvent(): DndEvent;
  set event(value: DndEvent);
  toString(): string;
}
export declare const DndEvent_Payload_Which: {
  readonly NO_VARIANT: 0;
  readonly ENTER: 1;
  readonly POSITION: 2;
  readonly DROP: 3;
  readonly LEAVE: 4;
};
export type DndEvent_Payload_Which = (typeof DndEvent_Payload_Which)[keyof typeof DndEvent_Payload_Which];
export declare class DndEvent_Payload extends $.Struct {
  static readonly NO_VARIANT: 0;
  static readonly ENTER: 1;
  static readonly POSITION: 2;
  static readonly DROP: 3;
  static readonly LEAVE: 4;
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get _isNoVariant(): boolean;
  set noVariant(_: true);
  _adoptEnter(value: $.Orphan<DndEnter>): void;
  _disownEnter(): $.Orphan<DndEnter>;
  get enter(): DndEnter;
  _hasEnter(): boolean;
  _initEnter(): DndEnter;
  get _isEnter(): boolean;
  set enter(value: DndEnter);
  _adoptPosition(value: $.Orphan<DndPosition>): void;
  _disownPosition(): $.Orphan<DndPosition>;
  get position(): DndPosition;
  _hasPosition(): boolean;
  _initPosition(): DndPosition;
  get _isPosition(): boolean;
  set position(value: DndPosition);
  _adoptDrop(value: $.Orphan<DndDrop>): void;
  _disownDrop(): $.Orphan<DndDrop>;
  get drop(): DndDrop;
  _hasDrop(): boolean;
  _initDrop(): DndDrop;
  get _isDrop(): boolean;
  set drop(value: DndDrop);
  get _isLeave(): boolean;
  set leave(_: true);
  toString(): string;
  which(): DndEvent_Payload_Which;
}
export declare class DndEvent extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get payload(): DndEvent_Payload;
  _initPayload(): DndEvent_Payload;
  toString(): string;
}
export declare class DndEnter extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  _adoptMimeTypes(value: $.Orphan<$.List<string>>): void;
  _disownMimeTypes(): $.Orphan<$.List<string>>;
  get mimeTypes(): $.List<string>;
  _hasMimeTypes(): boolean;
  _initMimeTypes(length: number): $.List<string>;
  set mimeTypes(value: $.List<string>);
  toString(): string;
}
export declare class DndPosition extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get x(): number;
  set x(value: number);
  get y(): number;
  set y(value: number);
  toString(): string;
}
export declare class DndDrop extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get mimeType(): string;
  set mimeType(value: string);
  _adoptData(value: $.Orphan<$.Data>): void;
  _disownData(): $.Orphan<$.Data>;
  get data(): $.Data;
  _hasData(): boolean;
  _initData(length: number): $.Data;
  set data(value: $.Data);
  toString(): string;
}
export declare class TouchEvent extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get touchId(): number;
  set touchId(value: number);
  get x(): number;
  set x(value: number);
  get y(): number;
  set y(value: number);
  get state(): number;
  set state(value: number);
  toString(): string;
}
export declare const GesturePhase: {
  readonly BEGIN: 0;
  readonly UPDATE: 1;
  readonly END: 2;
};
export type GesturePhase = (typeof GesturePhase)[keyof typeof GesturePhase];
export declare class GestureSwipeEvt extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get phase(): GesturePhase;
  set phase(value: GesturePhase);
  get fingers(): number;
  set fingers(value: number);
  get dx(): number;
  set dx(value: number);
  get dy(): number;
  set dy(value: number);
  toString(): string;
}
export declare class GesturePinchEvt extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get phase(): GesturePhase;
  set phase(value: GesturePhase);
  get fingers(): number;
  set fingers(value: number);
  get dx(): number;
  set dx(value: number);
  get dy(): number;
  set dy(value: number);
  get scale(): number;
  set scale(value: number);
  get rotation(): number;
  set rotation(value: number);
  toString(): string;
}
export declare class CompositionEvt extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get phase(): string;
  set phase(value: string);
  get text(): string;
  set text(value: string);
  toString(): string;
}
