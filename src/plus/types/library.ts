import {
  any,
  arr,
  bool,
  nullable,
  num,
  obj,
  opt,
  str,
  type Infer,
  type Shape,
} from "../bridge/shape";
import { planV1, resultV1 } from "../bridge/data";

/** `data` of `library status|pull|push` and the results of the self-MCP tools `library_status` and
 * `library_pull`, checked against the golden envelopes by `data.test.ts` and `selfmcp.test.ts`.
 * The network is touched only with `--fetch`; the remote URL never carries its credentials. */

const cloneRow = obj({
  path: str,
  skills: num,
  ahead: num,
  behind: num,
  sameRemote: bool,
});

/** Where the library clone stands against its remote: ahead/behind come from the refs git has,
 * `fetch.requested` says whether this call contacted the remote, and `duplicateClones` lists the
 * other clones of the same remote under the configured source roots (never deleted). */
export const libraryStatusData = obj({
  repo: str,
  remote: nullable(str),
  branch: nullable(str),
  upstream: nullable(str),
  ahead: num,
  behind: num,
  uncommitted: num,
  lastFetch: nullable(str),
  fetch: obj({ requested: bool, ok: nullable(bool), error: nullable(str) }),
  auth: obj({ method: str, ok: bool, checked: bool }),
  duplicateClones: arr(cloneRow),
});
export type LibraryStatusData = Infer<typeof libraryStatusData>;

/** A dry run answers with `plan`; an apply with `pulled` and `result` (`commits` when it moved). */
export const libraryPullData = obj({
  dryRun: bool,
  repo: str,
  plan: opt(planV1),
  pulled: opt(bool),
  commits: opt(num),
  result: opt(resultV1),
});
export type LibraryPullData = Infer<typeof libraryPullData>;

const finding = obj({ rule: str, file: str, line: num, commit: opt(str) });

/** What `library push` checked first; a secret finding blocks the push, an audit finding warns. */
export const libraryChecks = obj({
  audit: obj({
    ran: bool,
    skills: opt(num),
    high: opt(num),
    findings: opt(arr(any)),
    error: opt(str),
  }),
  gitleaks: obj({
    available: bool,
    ran: bool,
    error: nullable(str),
    count: num,
    findings: arr(finding),
  }),
  builtinScan: obj({ count: num, findings: arr(finding) }),
  blocked: bool,
});
export type LibraryChecks = Infer<typeof libraryChecks>;

/** A dry run answers with `plan`, `commits` and `branch`; an apply with `pushed`, `commitSha`
 * and `result` (`message` instead of a sha when there was nothing to commit). */
export const libraryPushData = obj({
  dryRun: bool,
  repo: str,
  branch: opt(nullable(str)),
  pushed: bool,
  plan: opt(planV1),
  commits: opt(arr(obj({ sha: str, subject: str }))),
  commitSha: opt(str),
  message: opt(str),
  checks: libraryChecks,
  result: opt(resultV1),
});
export type LibraryPushData = Infer<typeof libraryPushData>;

/** Golden file stem to the shape of its envelope `data`. */
export const libraryShapes: Record<string, Shape<unknown>> = {
  "library-status.ahead": libraryStatusData,
  "library-status.duplicate": libraryStatusData,
  "library-status.behind": libraryStatusData,
  "library-status.fetch": libraryStatusData,
  "library-status.dirty": libraryStatusData,
  "library-status.no-remote": libraryStatusData,
  "library-pull.preview": libraryPullData,
  "library-pull.apply": libraryPullData,
  "library-pull.current": libraryPullData,
  "library-push.dry-run": libraryPushData,
  "library-push.secret-preview": libraryPushData,
  "library-push.apply": libraryPushData,
  "library-push.current": libraryPushData,
};

/** Tool name to the shape of its `structuredContent`; both answer like the CLI. */
export const libraryToolShapes: Record<string, Shape<unknown>> = {
  library_status: libraryStatusData,
  library_pull: libraryPullData,
};
