// Augment React types with canary features not yet in @types/react.
// Mirrors `frontend/src/react-canary.d.ts` — needed here because
// `Dock.tsx` references `ViewTransition` and the components
// package's `tsc -b` build resolves types independently.
import "react";

declare module "react" {
	const ViewTransition: React.ExoticComponent<{
		children?: React.ReactNode;
		name?: string;
		enter?: string;
		exit?: string;
	}>;
}
