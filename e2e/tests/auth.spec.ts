import { expect, test } from "./fixtures";

/** End-to-end OIDC sign-in flow against the per-worker
 *  `mock-oauth2-server`:
 *
 *    1. Open the SPA — auth UI shows "Sign in" (anonymous).
 *    2. Click "Sign in" → backend's `/auth/login` → 302 to mock IdP.
 *    3. Mock IdP renders a tiny form; we fill in a subject + email.
 *    4. IdP redirects back to the backend's `/auth/callback`,
 *       which exchanges the code, validates the ID token, sets a
 *       session cookie, and 302s to the SPA.
 *    5. SPA reloads, `useAuth` re-fetches `/auth/me`, "Sign in"
 *       flips to "Sign out (alice@example.com)".
 *
 *  The mock server's authorize endpoint defaults to interactive
 *  mode — it shows a single-input form (`#username`) where the
 *  value becomes the `sub` claim, plus a hidden field for email.
 *  We drive the form via Playwright; a slim wrapper keeps the
 *  selectors out of the test body. */
test("OIDC sign in surfaces the user's email in the menu bar", async ({
	page,
	frontendUrl,
}) => {
	await page.goto(frontendUrl);

	// Anonymous state — Sign in button shown.
	const signInBtn = page.locator('[data-testid="auth-sign-in"]');
	await expect(signInBtn).toBeVisible({ timeout: 15_000 });

	// Kick off the OIDC dance. The redirect goes off-origin to the
	// mock IdP, so we wait for the form to render before driving it.
	await Promise.all([
		page.waitForURL(/\/x11-web\/(authorize|login)/, { timeout: 15_000 }),
		signInBtn.click(),
	]);

	// mock-oauth2-server's default login form: a "Enter any
	// user/subject" textbox (becomes the `sub` claim) plus an
	// optional JSON-claims textarea. We fill both so the resulting
	// ID token carries an `email` claim that the SPA can render.
	const userInput = page.getByPlaceholder(/Enter any user\/subject/i);
	await expect(userInput).toBeVisible({ timeout: 10_000 });
	await userInput.fill("alice@example.com");
	const claimsInput = page.getByPlaceholder(/Optional claims JSON/i);
	await claimsInput.fill('{"email": "alice@example.com"}');
	const submit = page.getByRole("button", { name: "Sign-in" });
	await Promise.all([
		page.waitForURL(/^http:\/\/localhost:\d+\/(\?|$)/, {
			timeout: 15_000,
		}),
		submit.click(),
	]);

	// Back on the SPA, `useAuth` re-fetches `/auth/me` and the bar
	// flips to the signed-in state.
	await expect(page.locator('[data-testid="auth-sign-out"]')).toBeVisible({
		timeout: 15_000,
	});
	await expect(page.locator('[data-testid="auth-email"]')).toHaveText(
		"alice@example.com",
	);

	// `/auth/me` directly returns the same user — guarantees the
	// session cookie is wired up end-to-end.
	const me = await page.evaluate(async () => {
		const r = await fetch("/auth/me", { credentials: "include" });
		return r.ok ? r.json() : null;
	});
	expect(me).toMatchObject({
		sub: "alice@example.com",
		email: "alice@example.com",
	});
});
