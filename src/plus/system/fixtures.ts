import { CtlReplyFailure } from "../fixtures/ctlReply";
import golden from "../../../src-tauri/tests/fixtures/ctl-envelopes/commands.json";

/** Envelope `data` of the System commands per argv (joined with spaces): the real golden
 * envelopes of `src-tauri/tests/fixtures/ctl-envelopes`, a fresh machine that has not set sync
 * up, a council that is not installed and a self-management server that is not either. The
 * names are made up. Dynamic argv (a form the user fills in) get their replies in the tests. */

interface Golden {
  argv: string[];
  envelope: {
    ok: boolean;
    data?: unknown;
    error?: { code: string; message: string };
  };
}

const files = import.meta.glob<Golden>(
  "../../../src-tauri/tests/fixtures/ctl-envelopes/{sync,update,cc,council,import,mcp}[-.]*.json",
  { eager: true, import: "default" },
);

const byStem = new Map(
  Object.entries(files).map(([path, file]) => [
    path.replace(/^.*\/(.*)\.json$/, "$1"),
    file,
  ]),
);

export function goldenFile(stem: string): Golden {
  const file = byStem.get(stem);
  if (!file) throw new Error(`no golden envelope ${stem}`);
  return file;
}

export const goldenData = (stem: string): unknown => goldenFile(stem).envelope.data;

/** What the fake bridge answers for a golden: its data, or the failure it recorded. */
export function goldenReply(stem: string): unknown {
  const { envelope } = goldenFile(stem);
  if (envelope.ok) return envelope.data;
  return new CtlReplyFailure(
    envelope.error?.code ?? "failed",
    envelope.error?.message ?? "failed",
    envelope.data,
  );
}

export const commandsData = (golden as { envelope: { data: unknown } }).envelope.data;

/** A server list with every state of the Updates tab: a git server two commits behind with an
 * update command, a release that is current, an npx server that follows latest and one whose
 * source is not known yet. The fields are those of `update --check` in `plus/update`. */
export const updateWorld = {
  mode: "check",
  counts: { "up-to-date": 1, "update-available": 1, auto: 1, skipped: 1 },
  servers: [
    {
      id: "srv-git",
      kind: "git",
      status: "update-available",
      message: "2 commit(s) behind origin/main",
      detected: true,
      current: "a1b2c3d4e5f6",
      latest: "f6e5d4c3b2a1",
      behind: 2,
      ahead: 0,
      plan: [
        "git merge --ff-only origin/main",
        "post_update: ./build.sh (not run without --allow-commands)",
      ],
    },
    {
      id: "srv-release",
      kind: "github-release",
      status: "up-to-date",
      message: "up to date with v1.4.0",
      detected: true,
      current: "v1.4.0",
      latest: "v1.4.0",
    },
    {
      id: "srv-npx",
      kind: "npx",
      status: "auto",
      message: "follows the latest version on each start",
      detected: true,
    },
    {
      id: "srv-new",
      kind: "unknown",
      status: "skipped",
      message: "unknown source; run update --init",
      detected: false,
    },
  ],
};

export const systemCtlFixtures: Array<[string, unknown]> = [
  ["commands", commandsData],
  ["sync status", goldenReply("sync-status")],
  ["sync diff", goldenReply("sync-diff.unconfigured")],
  ["sync git-sync --status", goldenReply("sync-git-sync.status")],
  ["update --check", goldenReply("update.check")],
  ["cc list", goldenReply("cc-list")],
  ["cc update --dry-run", goldenReply("cc-update.preview")],
  ["cc update demo-plugin --dry-run", goldenReply("cc-update.preview")],
  ["cc update demo-plugin", goldenReply("cc-update.apply")],
  ["council doctor", goldenReply("council-doctor")],
  ["council tools", goldenReply("council-tools")],
  ["mcp doctor", goldenReply("mcp-doctor")],
  ["mcp tools", goldenReply("mcp-tools")],
  ["mcp call where_am_i --args-stdin", goldenReply("mcp-call.where_am_i")],
  ["mcp call flow_diagram --args-stdin", goldenReply("mcp-call.flow_diagram")],
];
