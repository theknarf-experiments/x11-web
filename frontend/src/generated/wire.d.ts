import * as $ from "capnp-es";
export declare const _capnpFileId = 18310840293120370807n;
export declare const Frame_Which: {
  readonly NO_VARIANT: 0;
  readonly PUT_IMAGE: 1;
  readonly WINDOW_THUMBNAIL: 2;
  readonly WORKSPACE_SYNC: 3;
};
export type Frame_Which = (typeof Frame_Which)[keyof typeof Frame_Which];
export declare class Frame extends $.Struct {
  static readonly NO_VARIANT: 0;
  static readonly PUT_IMAGE: 1;
  static readonly WINDOW_THUMBNAIL: 2;
  static readonly WORKSPACE_SYNC: 3;
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get _isNoVariant(): boolean;
  set noVariant(_: true);
  _adoptPutImage(value: $.Orphan<PutImage>): void;
  _disownPutImage(): $.Orphan<PutImage>;
  get putImage(): PutImage;
  _hasPutImage(): boolean;
  _initPutImage(): PutImage;
  get _isPutImage(): boolean;
  set putImage(value: PutImage);
  _adoptWindowThumbnail(value: $.Orphan<WindowThumbnail>): void;
  _disownWindowThumbnail(): $.Orphan<WindowThumbnail>;
  get windowThumbnail(): WindowThumbnail;
  _hasWindowThumbnail(): boolean;
  _initWindowThumbnail(): WindowThumbnail;
  get _isWindowThumbnail(): boolean;
  set windowThumbnail(value: WindowThumbnail);
  _adoptWorkspaceSync(value: $.Orphan<WorkspaceSync>): void;
  _disownWorkspaceSync(): $.Orphan<WorkspaceSync>;
  get workspaceSync(): WorkspaceSync;
  _hasWorkspaceSync(): boolean;
  _initWorkspaceSync(): WorkspaceSync;
  get _isWorkspaceSync(): boolean;
  set workspaceSync(value: WorkspaceSync);
  toString(): string;
  which(): Frame_Which;
}
export declare class PutImage extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get windowId(): string;
  set windowId(value: string);
  get x(): number;
  set x(value: number);
  get y(): number;
  set y(value: number);
  get width(): number;
  set width(value: number);
  get height(): number;
  set height(value: number);
  _adoptData(value: $.Orphan<$.Data>): void;
  _disownData(): $.Orphan<$.Data>;
  get data(): $.Data;
  _hasData(): boolean;
  _initData(length: number): $.Data;
  set data(value: $.Data);
  toString(): string;
}
export declare class WindowThumbnail extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get windowId(): string;
  set windowId(value: string);
  get width(): number;
  set width(value: number);
  get height(): number;
  set height(value: number);
  _adoptData(value: $.Orphan<$.Data>): void;
  _disownData(): $.Orphan<$.Data>;
  get data(): $.Data;
  _hasData(): boolean;
  _initData(length: number): $.Data;
  set data(value: $.Data);
  toString(): string;
}
export declare class WorkspaceSync extends $.Struct {
  static readonly _capnp: {
    displayName: string;
    id: string;
    size: $.ObjectSize;
  };
  get workspaceId(): string;
  set workspaceId(value: string);
  _adoptMessage(value: $.Orphan<$.Data>): void;
  _disownMessage(): $.Orphan<$.Data>;
  get message(): $.Data;
  _hasMessage(): boolean;
  _initMessage(length: number): $.Data;
  set message(value: $.Data);
  toString(): string;
}
