import type { ContextPlanData } from "../types/context";

/** A fixture home with the two layers (personal, client-acme), the bare launch profile and the
 * shims file. The shapes are those of the golden envelopes in `ctl-envelopes/context-*.json`;
 * the rows of `layers` and `profiles` follow `plus/context/manage.rs`. */
export const DATA = "/fixture/data";
export const HOME = "/fixture/home";
export const SHIMS = `${DATA}/context-shims.zsh`;
export const BARE_DIR = `${HOME}/.config/toolport/claude-profiles/bare`;

export const personalLayer = {
  name: "personal",
  path: `${HOME}/.config/toolport/skills_repo/rules/personal/SKILL.md`,
  globs: [] as string[],
  description: "Personal rules and preferences",
};
export const clientLayer = {
  name: "client-acme",
  path: `${HOME}/.config/toolport/skills_repo/rules/client-acme/SKILL.md`,
  globs: ["**/clients/acme/**"],
  description: "Rules for the acme client",
};
export const bareProfile = {
  name: "bare",
  shim: "claude-bare",
  org: false,
  orgMode: "import",
  rules: "none",
  servers: "none",
  generated: true,
  dir: BARE_DIR,
};

export const statusData = {
  config: { exists: true, path: `${HOME}/.config/toolport/context.json` },
  layers: [personalLayer, clientLayer],
  legacyDupes: [],
  profiles: [bareProfile],
  shims: {
    exists: true,
    legacyExists: false,
    legacyPath: `${HOME}/.config/mcpm/context-shims.zsh`,
    path: SHIMS,
  },
  zshrc: {
    deadAliases: [
      {
        name: "toolup",
        file: `${HOME}/.config/mcpm/local-aliases.zsh`,
        line: 1,
        command: "cd ~/toolport && ./update.sh",
      },
    ],
    exists: true,
    legacyLines: [
      {
        line: 3,
        file: "context-shims.zsh",
        text: "source ~/.config/mcpm/context-shims.zsh",
      },
    ],
    path: `${HOME}/.zshrc`,
  },
};

export const emptyStatus = {
  ...statusData,
  config: { ...statusData.config, exists: false },
  layers: [],
  profiles: [],
  shims: { ...statusData.shims, exists: false },
  zshrc: { ...statusData.zshrc, deadAliases: [], legacyLines: [] },
};

export const checks: Array<[string, string]> = [
  ["ok", "no legacy MCP duplicates"],
  [
    "warn",
    "context-shims sourced BEFORE shell-wrapper.sh in ~/.zshrc — move our source line below it",
  ],
  ["ok", "corp-tools not installed — no coexistence constraints"],
];

export const zshrcPlan = (dry: boolean) => ({
  path: `${HOME}/.zshrc`,
  exists: true,
  dryRun: dry,
  changes: [
    {
      line: 3,
      before: "source ~/.config/mcpm/context-shims.zsh",
      after: `source ${SHIMS}`,
    },
  ],
  skipped: [],
  copies: [],
  backup: dry ? null : `${HOME}/.zshrc.toolport-backup-1`,
  order: { ok: false, problems: ["context-shims is sourced before shell-wrapper.sh"] },
  deadAliases: [],
  actions: [`would rewrite 1 line(s) of ${HOME}/.zshrc`],
  warnings: [],
});

export function deployData(dry: boolean, rewrite: boolean): ContextPlanData {
  const verb = dry ? "would write" : "wrote";
  return {
    actions: [
      `${verb} shims: ${SHIMS}`,
      ...(dry ? [] : ["saved config (1 profile(s))"]),
      ...(rewrite
        ? [`${dry ? "would rewrite" : "rewrote"} 1 line(s) of ${HOME}/.zshrc`]
        : []),
    ],
    checks,
    dryRun: dry,
    warnings: [],
    ...(rewrite ? { zshrc: zshrcPlan(dry) } : {}),
  } as ContextPlanData;
}

export const checkpointData = {
  at_checkpoint: false,
  checkpoint_at: 50000,
  checkpoint_point: 150000,
  remaining_to_checkpoint: 148500,
  remaining_to_compact: 198500,
  used_tokens: 1500,
  window: 200000,
  window_source: "model",
};

export const STATUSLINE = JSON.stringify({
  model: { id: "model-x" },
  context_window: { context_window_size: 200000, used_percentage: 0.75 },
});

const OPTIONS = [[], ["--rules"], ["--rewrite-zshrc"], ["--rules", "--rewrite-zshrc"]];

/** Every argv the Launch & shell tab can run against the fixture home, as `[argv, data]`. */
export function contextCtlFixtures(): Array<[string, unknown]> {
  const out: Array<[string, unknown]> = [
    ["context status", statusData],
    ["context profile list", { profiles: [bareProfile] }],
    ["context client list", { layers: [personalLayer, clientLayer] }],
    ["context checkpoint-status --checkpoint-at 50000", checkpointData],
  ];
  for (const flags of OPTIONS) {
    const rewrite = flags.includes("--rewrite-zshrc");
    const base = flags.join(" ");
    const suffix = base ? ` ${base}` : "";
    out.push([`context plan${suffix}`, deployData(true, rewrite)]);
    out.push([`context apply${suffix} --dry-run`, deployData(true, rewrite)]);
    out.push([`context apply${suffix}`, deployData(false, rewrite)]);
    out.push([
      `context apply${suffix} --no-persist --dry-run`,
      deployData(true, rewrite),
    ]);
    out.push([`context apply${suffix} --no-persist`, deployData(false, rewrite)]);
    out.push([
      `context sync${suffix} --dry-run`,
      { apply: null, dryRun: true, plan: deployData(true, rewrite) },
    ]);
    out.push([
      `context sync${suffix}`,
      {
        apply: deployData(false, rewrite),
        dryRun: false,
        plan: deployData(true, rewrite),
      },
    ]);
  }
  return out;
}
