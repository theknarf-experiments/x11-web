// Minimal ambient declarations for the W3C EditContext API.
// Chrome / Edge ship this as of 2024; Safari 18.4+ as of 2025;
// Firefox is still working on it. TypeScript's `lib.dom.d.ts`
// hasn't picked it up yet so we declare just the surface we use.
//
// Spec: https://www.w3.org/TR/edit-context/
// MDN:  https://developer.mozilla.org/en-US/docs/Web/API/EditContext

interface EditContextInit {
	text?: string;
	selectionStart?: number;
	selectionEnd?: number;
}

interface TextUpdateEvent extends Event {
	readonly updateRangeStart: number;
	readonly updateRangeEnd: number;
	readonly text: string;
	readonly selectionStart: number;
	readonly selectionEnd: number;
	readonly compositionStart: number;
	readonly compositionEnd: number;
}

interface TextFormatUpdateEvent extends Event {
	readonly compositionStart: number;
	readonly compositionEnd: number;
	getTextFormats(): Array<{
		rangeStart: number;
		rangeEnd: number;
		underlineStyle?: string;
		underlineThickness?: string;
	}>;
}

interface EditContextEventMap {
	textupdate: TextUpdateEvent;
	textformatupdate: TextFormatUpdateEvent;
	compositionstart: Event;
	compositionend: Event;
	characterboundsupdate: Event;
}

interface EditContext extends EventTarget {
	readonly text: string;
	readonly selectionStart: number;
	readonly selectionEnd: number;
	readonly characterBoundsRangeStart: number;

	updateText(rangeStart: number, rangeEnd: number, text: string): void;
	updateSelection(start: number, end: number): void;
	updateControlBounds(controlBounds: DOMRect): void;
	updateSelectionBounds(selectionBounds: DOMRect): void;
	updateCharacterBounds(rangeStart: number, characterBounds: DOMRect[]): void;
	attachedElements(): HTMLElement[];
	characterBounds(): DOMRect[];

	addEventListener<K extends keyof EditContextEventMap>(
		type: K,
		listener: (this: EditContext, ev: EditContextEventMap[K]) => unknown,
		options?: boolean | AddEventListenerOptions,
	): void;
	addEventListener(
		type: string,
		listener: EventListenerOrEventListenerObject,
		options?: boolean | AddEventListenerOptions,
	): void;
	removeEventListener<K extends keyof EditContextEventMap>(
		type: K,
		listener: (this: EditContext, ev: EditContextEventMap[K]) => unknown,
		options?: boolean | EventListenerOptions,
	): void;
	removeEventListener(
		type: string,
		listener: EventListenerOrEventListenerObject,
		options?: boolean | EventListenerOptions,
	): void;
}

declare const EditContext: {
	prototype: EditContext;
	new (init?: EditContextInit): EditContext;
};

interface HTMLElement {
	editContext: EditContext | null;
}
