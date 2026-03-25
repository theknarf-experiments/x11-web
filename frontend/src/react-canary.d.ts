// Augment React types with canary features not yet in @types/react
import "react";

declare module "react" {
	const ViewTransition: React.ExoticComponent<{
		children?: React.ReactNode;
		name?: string;
		enter?: string;
		exit?: string;
	}>;
}
