import { useCallback, useEffect, useState } from "react";

/** What the backend's `/auth/me` returns. `null` means no session
 *  (anonymous user); the frontend still works in that mode for
 *  every feature that doesn't gate on identity. */
export interface AuthenticatedUser {
	sub: string;
	email: string | null;
}

interface UseAuthResult {
	user: AuthenticatedUser | null;
	/** True until the initial `/auth/me` fetch completes. Lets
	 *  components avoid flashing "Sign in" while we don't know yet. */
	loading: boolean;
	signIn: () => void;
	signOut: () => Promise<void>;
}

/** Track the current auth state by polling `/auth/me` on mount.
 *  Sign-in / sign-out trigger a redirect through the backend
 *  (which runs the OIDC dance and sets a session cookie); on the
 *  bounce-back to `/`, this hook re-fetches and surfaces the new
 *  user. */
export function useAuth(): UseAuthResult {
	const [user, setUser] = useState<AuthenticatedUser | null>(null);
	const [loading, setLoading] = useState(true);

	useEffect(() => {
		let cancelled = false;
		fetch("/auth/me", { credentials: "include" })
			.then((r) => (r.ok ? r.json() : null))
			.then((data: AuthenticatedUser | null) => {
				if (!cancelled) {
					setUser(data);
					setLoading(false);
				}
			})
			.catch(() => {
				if (!cancelled) setLoading(false);
			});
		return () => {
			cancelled = true;
		};
	}, []);

	const signIn = useCallback(() => {
		// Full-page navigation — the OIDC flow needs to leave the
		// SPA so the IdP can render its own login UI. After the
		// callback completes the server bounces us back to `/` and
		// the hook re-fetches.
		window.location.assign("/auth/login");
	}, []);

	const signOut = useCallback(async () => {
		await fetch("/auth/logout", {
			method: "POST",
			credentials: "include",
		});
		setUser(null);
	}, []);

	return { user, loading, signIn, signOut };
}
