// React canary features not yet in @types/react
declare namespace React {
	const ViewTransition: React.ExoticComponent<{
		children: React.ReactNode;
		name?: string;
	}>;
}
