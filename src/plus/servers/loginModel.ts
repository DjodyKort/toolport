import type { AuthRow, AuthSection, LoginState } from "../bridge/data";

export type LoginVariant = "success" | "warning" | "destructive" | "off";

export interface LoginView {
  label: string;
  variant: LoginVariant;
  tone?: "danger" | "warning";
}

export const LOGIN_VIEWS: Record<LoginState, LoginView> = {
  ok: { label: "Signed in", variant: "success" },
  expiring: { label: "Expiring", variant: "warning", tone: "warning" },
  needs_reauth: { label: "Sign-in needed", variant: "destructive", tone: "danger" },
  revoked: { label: "Access revoked", variant: "destructive", tone: "danger" },
  misconfigured: { label: "Misconfigured", variant: "destructive", tone: "danger" },
  unreachable: { label: "Cannot check", variant: "warning", tone: "warning" },
  unknown: { label: "Not checked yet", variant: "off" },
};

/** A check that came back with a refusal. An unreachable server or a login that is only about to
 * expire is a warning, not a failure. */
export function loginFailed(row: AuthRow): boolean {
  return LOGIN_VIEWS[row.state].tone === "danger";
}

/** What a person has to act on: a failure, or a login that stops working soon. */
export function loginNeedsYou(row: AuthRow): boolean {
  return loginFailed(row) || row.state === "expiring";
}

/** The reason the check gave, unless it is only a restatement of the state. */
export function loginReason(row: AuthRow): string | null {
  const reason = row.reason.trim();
  return reason === "" || reason.toLowerCase() === "ok" ? null : reason;
}

export function loginRowOf(
  auth: AuthSection | null,
  serverId: string,
): AuthRow | undefined {
  return auth?.servers.find((row) => row.server === serverId);
}

/** Whether the fix of a row is a sign-in, so the row offers Sign in again first. */
export function fixIsSignIn(row: AuthRow): boolean {
  return row.fix?.command?.includes("auth login") === true;
}
