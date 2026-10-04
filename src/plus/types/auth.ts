import {
  any,
  arr,
  bool,
  nullable,
  num,
  obj,
  str,
  type Infer,
  type Shape,
} from "../bridge/shape";

/** `data` of the auth commands, checked against the golden envelopes by `data.test.ts`. */

export const authHookData = obj({
  auth: obj({
    expiring: num,
    misconfigured: num,
    needs_reauth: num,
    ok: num,
    revoked: num,
    text: str,
    unreachable: num,
    worst: arr(any),
  }),
});
export type AuthHookData = Infer<typeof authHookData>;

export const authLoginData = obj({
  consentUrl: nullable(any),
  flow: str,
  message: str,
  name: str,
  probe: nullable(any),
  server: str,
  servers: arr(any),
  signedIn: bool,
});
export type AuthLoginData = Infer<typeof authLoginData>;

export const authProbeData = obj({
  counts: obj({
    expiring: num,
    misconfigured: num,
    needs_reauth: num,
    ok: num,
    revoked: num,
    unknown: num,
    unreachable: num,
  }),
  failures: arr(any),
  mode: str,
  probes: arr(
    obj({
      nextDueAt: num,
      ran: bool,
      server: str,
      skipped: nullable(num),
      tracked: obj({
        reason: str,
        since: num,
        state: str,
        transient: nullable(any),
      }),
    }),
  ),
  servers: arr(
    obj({
      expiresAt: nullable(any),
      fix: obj({
        action: str,
        command: str,
        ipc: nullable(any),
        label: str,
        server: str,
      }),
      lastProbe: num,
      reason: str,
      server: str,
      since: num,
      state: str,
      ttlSecs: nullable(any),
    }),
  ),
});
export type AuthProbeData = Infer<typeof authProbeData>;

/** Golden file stem to the shape of its envelope `data`. */
export const authShapes: Record<string, Shape<unknown>> = {
  "auth-hook": authHookData,
  "auth-login.signed-in": authLoginData,
  "auth-probe.apply": authProbeData,
};
