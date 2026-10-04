import type {
  SkillsBundleData,
  SkillsInitData,
  SkillsInstallData,
  SkillsSearchData,
  SkillsTapAddData,
  SkillsTapLsData,
  SkillsTapRemoveData,
  SkillsTapUpdateData,
  SkillsUnbundleData,
} from "../types/skills";
import { REPO } from "./fixtures";

/** The world of the taps, search, install, bundle and init panels: one GitHub tap that is
 * cloned, one local tap whose spec cannot be derived, and a risky skill the audit blocks. */
export const TAPS_ROOT = "/fixture/data/taps";

export const tapLs: SkillsTapLsData = {
  tapsRoot: TAPS_ROOT,
  taps: [
    {
      cloned: true,
      name: "acme-skills",
      path: `${TAPS_ROOT}/acme-skills`,
      repo: "acme/skills",
      url: "https://github.com/acme/skills.git",
    },
    {
      cloned: false,
      name: "local-notes",
      path: `${TAPS_ROOT}/local-notes`,
      repo: "/fixture/tap-src",
      url: "/fixture/tap-src",
    },
  ],
};

export const searchData: SkillsSearchData = {
  discoveryWarnings: [],
  query: "review",
  tapCount: 2,
  results: [
    {
      description: "Review a pull request for risky changes",
      name: "code-review",
      repo: "acme/skills",
      tap: "acme-skills",
      type: "skill",
    },
    {
      description: "Review notes from a local folder",
      name: "note-review",
      repo: "/fixture/tap-src",
      tap: "local-notes",
      type: "skill",
    },
  ],
};

export const emptySearch = (query: string): SkillsSearchData => ({
  discoveryWarnings: [],
  query,
  results: [],
  tapCount: 0,
});

export const tapAddData = (dryRun: boolean): SkillsTapAddData => ({
  cloned: !dryRun,
  dryRun,
  head: dryRun ? null : "a1b2c3d",
  name: "tools",
  path: `${TAPS_ROOT}/tools`,
  repo: "acme/tools",
  url: "https://github.com/acme/tools.git",
});

export const tapRemoveData = (dryRun: boolean): SkillsTapRemoveData => ({
  dryRun,
  hadClone: true,
  name: "acme-skills",
  path: `${TAPS_ROOT}/acme-skills`,
  removed: !dryRun,
});

export const tapUpdateData = (dryRun: boolean, failing = false): SkillsTapUpdateData => ({
  dryRun,
  failed: failing ? 1 : 0,
  results: tapLs.taps.map((tap) => ({
    error: failing && tap.name === "local-notes" ? "could not reach the remote" : null,
    head: dryRun ? null : "a1b2c3d",
    name: tap.name,
    ok: !(failing && tap.name === "local-notes"),
  })),
});

export const installData = (
  spec: string,
  options: { dryRun: boolean; blocked?: boolean; audit?: boolean; clean?: boolean },
): SkillsInstallData => {
  const risky = options.blocked && options.audit !== false;
  const medium = !options.clean && !risky && spec.includes("code-review");
  return {
    audit: {
      findings: risky
        ? [
            {
              severity: "high",
              skill: "deploy-risky",
              line: 7,
              message: "Instructs the model to pipe a download into a shell",
            },
          ]
        : medium
          ? [
              {
                severity: "medium",
                skill: "code-review",
                line: 4,
                message: "Suspicious: sudo usage in skill instructions",
              },
            ]
          : [],
      high: risky ? 1 : 0,
      low: 0,
      medium: medium ? 1 : 0,
      ran: options.audit !== false,
    },
    blocked: !!risky,
    cloneUrl: "https://github.com/acme/skills.git",
    discoveryWarnings: [],
    dryRun: options.dryRun,
    foundCount: 1,
    installedCount: risky ? 0 : 1,
    skills: risky
      ? []
      : [{ name: spec.split("/").pop(), type: "skill", status: "installed" }],
    skippedCount: 0,
    spec,
    symlinksSkipped: [],
    tap: "acme-skills",
    tapAdded: !options.dryRun,
    tapMissing: options.dryRun,
    target: REPO,
    version: null,
    versionIgnored: false,
  };
};

export const bundleData = (dryRun: boolean): SkillsBundleData => ({
  bundleBytes: dryRun ? null : 2048,
  dryRun,
  fileCount: 2,
  output: "/fixture/out/team.zip",
  repo: REPO,
  skills: ["api-review", "deploy-helper"].map((name) => ({
    files: 1,
    name,
    type: "skill",
  })),
  sourceBytes: 4096,
});

export const unbundleData = (dryRun: boolean): SkillsUnbundleData => ({
  bundle: "/fixture/in/team.zip",
  dryRun,
  files: ["skills/api-review/SKILL.md", "skills/deploy-helper/SKILL.md"],
  names: ["api-review", "deploy-helper"],
  overwritten: ["skills/deploy-helper/SKILL.md"],
  skipped: [],
  target: "/fixture/fresh",
});

export const initData = (dryRun: boolean): SkillsInitData => ({
  alreadyExists: false,
  created: ["mcpm-skills.yaml", "skills/", "rules/", "agents/", "styles/", "profiles/"],
  dryRun,
  name: "team",
  repo: "/fixture/new",
});

const pair = (
  argv: string,
  preview: unknown,
  apply: unknown,
): Array<[string, unknown]> => [
  [`${argv} --dry-run`, preview],
  [argv, apply],
];

/** Replies of the fake bridge for the panels that came after the first phase. */
export const tapsCtlFixtures: Array<[string, unknown]> = [
  ["skills tap ls", tapLs],
  ["skills search review", searchData],
  ["skills search nothing", emptySearch("nothing")],
  ...pair("skills tap add acme/tools --name tools", tapAddData(true), tapAddData(false)),
  ...pair("skills tap remove acme-skills", tapRemoveData(true), tapRemoveData(false)),
  ...pair("skills tap update acme-skills", tapUpdateData(true), tapUpdateData(false)),
  ...pair("skills tap update", tapUpdateData(true), tapUpdateData(false)),
  ...pair(
    "skills install @acme/skills/code-review",
    installData("@acme/skills/code-review", { dryRun: true }),
    installData("@acme/skills/code-review", { dryRun: false }),
  ),
  ...pair(
    "skills install @acme/risky",
    installData("@acme/risky", { dryRun: true, blocked: true }),
    installData("@acme/risky", { dryRun: false, blocked: true }),
  ),
  ...pair(
    "skills install @acme/risky --no-audit",
    installData("@acme/risky", { dryRun: true, audit: false }),
    installData("@acme/risky", { dryRun: false, audit: false }),
  ),
  ...pair(
    "skills bundle --skills api-review,deploy-helper --output /fixture/out/team.zip",
    bundleData(true),
    bundleData(false),
  ),
  ...pair(
    "skills unbundle /fixture/in/team.zip --path /fixture/fresh",
    unbundleData(true),
    unbundleData(false),
  ),
  ...pair("skills init --path /fixture/new --name team", initData(true), initData(false)),
];
