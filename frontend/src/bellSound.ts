import { useEffect } from "react";

type BellHandler = ((percent: number) => void) | null;

interface UseBellSoundArgs {
	onBell: (cb: BellHandler) => void;
}

/** Side-effect hook: register a `Bell` listener that beeps via
 *  `AudioContext` (volume scales with the X11 bell percent), or
 *  flashes the document body white for 100 ms when audio isn't
 *  available — i.e. the user hasn't gestured on the page yet, so
 *  Chromium refuses to start the audio context. */
export function useBellSound({ onBell }: UseBellSoundArgs): void {
	useEffect(() => {
		onBell((percent) => {
			try {
				const ctx = new AudioContext();
				const osc = ctx.createOscillator();
				const gain = ctx.createGain();
				osc.connect(gain);
				gain.connect(ctx.destination);
				osc.frequency.value = 800;
				gain.gain.value = Math.max(0.01, percent / 100);
				osc.start();
				osc.stop(ctx.currentTime + 0.1);
			} catch {
				document.body.style.backgroundColor = "#fff";
				setTimeout(() => {
					document.body.style.backgroundColor = "";
				}, 100);
			}
		});
		return () => onBell(null);
	}, [onBell]);
}
