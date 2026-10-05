import { describe, expect, it } from "vitest";
import { ctlShapes } from "../bridge/data";
import { check } from "../bridge/shape";
import { ctlTypeShapes } from "../types";
import { skillsCtlFixtures } from "./fixtures";
import { tapsCtlFixtures } from "./fixturesTaps";
import { Failure, createSkillsWorld } from "./world";

const shapes = { ...ctlShapes, ...ctlTypeShapes };

/** The golden envelope a world row has to look like. */
function stem(argv: string): string | null {
  const words = argv.split(" ");
  if (words[0] === "sources") return null;
  if (words[0] === "library") {
    const fetch = words.includes("--fetch") ? "fetch" : "behind";
    const dry = words.includes("--dry-run");
    return {
      status: `library-status.${fetch}`,
      pull: `library-pull.${dry ? "preview" : "apply"}`,
      push: `library-push.${dry ? "dry-run" : "apply"}`,
    }[words[1]] as string;
  }
  const kind = words.includes("--dry-run") ? "preview" : "apply";
  const verb = words[1] === "tap" ? `tap-${words[2]}` : words[1];
  switch (verb) {
    case "ls":
      return words.includes("--source") ? "skills-ls.source" : "skills-ls.library";
    case "status":
    case "lint":
    case "audit":
    case "diff":
    case "tap-ls":
      return `skills-${verb}`;
    case "search":
      return argv.endsWith("nothing") ? "skills-search.empty" : "skills-search.hit";
    case "install":
      return "skills-install.preview";
    default:
      return `skills-${verb}.${kind}`;
  }
}

const dataOf = (reply: unknown) => (reply instanceof Failure ? reply.data : reply);
const answer = (world: Map<string, () => unknown>, argv: string) => {
  const reply = world.get(argv);
  expect(reply, `no row for ${argv}`).toBeDefined();
  return reply!();
};
const read = (world: Map<string, () => unknown>, argv: string) =>
  dataOf(answer(world, argv)) as Record<string, any>; // eslint-disable-line @typescript-eslint/no-explicit-any

const READS = [
  "skills ls",
  "skills status",
  "skills lint",
  "skills audit",
  "skills diff",
  "skills sync --dry-run",
  "skills resolve --dry-run",
  "skills tap ls",
  "skills search review",
];
const isRead = (argv: string) =>
  argv.endsWith("--dry-run") ||
  /^skills (ls|status|lint|audit|diff|tap ls|search)/.test(argv) ||
  argv.startsWith("sources ") ||
  argv.startsWith("library status");

describe("the stateful skills world", () => {
  it("has every row of the static maps, and its first answers equal the static ones", () => {
    const world = createSkillsWorld();
    const stat = new Map([...skillsCtlFixtures, ...tapsCtlFixtures]);
    for (const [argv, data] of stat) {
      if (argv.startsWith("skills sync") && !argv.includes("--dry-run")) continue;
      expect(world.has(argv) || argv === "commands", argv).toBe(true);
      if (!world.has(argv)) continue;
      const first = dataOf(world.get(argv)!());
      if (/^skills (status|diff|tap update \S)|^sources /.test(argv)) continue;
      expect(first, argv).toEqual(dataOf(data));
    }
  });

  it("answers every row in the shape of its golden envelope, before and after a full walk", () => {
    const world = createSkillsWorld();
    const verify = (argv: string) => {
      const name = stem(argv);
      if (!name) return;
      const shape = shapes[name];
      expect(shape, `no shape for ${name} (${argv})`).toBeDefined();
      expect(check(shape, read(world, argv)), argv).toEqual([]);
    };
    const reads = [...world.keys()].filter(isRead);
    reads.forEach(verify);
    const walk = [
      "skills sync --client claude-code --client cursor",
      "skills clean",
      "skills resolve --migrate",
      "skills add reviewer --type skill",
      "skills uninstall api-review",
      "skills tap add acme/tools --name tools",
      "skills tap remove acme-skills",
      "skills install @acme/skills/code-review",
      "skills install @acme/risky --no-audit",
    ];
    for (const argv of walk) {
      verify(argv);
      reads.forEach(verify);
    }
    for (const argv of world.keys()) verify(argv);
  });

  it("changes nothing on a preview: every --dry-run row leaves the reads as they were", () => {
    const world = createSkillsWorld();
    const before = READS.map((argv) => JSON.stringify(read(world, argv)));
    for (const argv of world.keys()) if (argv.endsWith("--dry-run")) answer(world, argv);
    expect(READS.map((argv) => JSON.stringify(read(world, argv)))).toEqual(before);
  });

  it("changes what the next read answers only after the write is applied", () => {
    const world = createSkillsWorld();
    expect(read(world, "skills ls").skills).toHaveLength(35);
    expect(read(world, "skills status")).toMatchObject({
      drift: true,
      lockfilePresent: true,
    });
    expect(answer(world, "skills diff")).toBeInstanceOf(Failure);

    answer(world, "skills sync --client claude-code --client cursor");
    expect(read(world, "skills status")).toMatchObject({ drift: false, lockedCount: 35 });
    expect(answer(world, "skills diff")).toMatchObject({ clean: true, unchanged: 35 });

    answer(world, "skills uninstall api-review");
    expect(read(world, "skills ls").skills).toHaveLength(34);
    expect(read(world, "skills status").entries).toHaveLength(34);

    answer(world, "skills add reviewer --type skill");
    expect(
      read(world, "skills ls").skills.map((s: { name: string }) => s.name),
    ).toContain("reviewer");
    expect(read(world, "skills diff")).toMatchObject({ new: ["reviewer"], clean: false });

    expect(read(world, "skills resolve --dry-run").collisions).toHaveLength(1);
    answer(world, "skills resolve --migrate");
    expect(read(world, "skills resolve --dry-run").collisions).toHaveLength(0);

    answer(world, "skills clean");
    expect(read(world, "skills status")).toMatchObject({
      lockfilePresent: false,
      outputs: [],
      drift: false,
    });
    expect(read(world, "skills sync --dry-run")).toMatchObject({
      clientSource: "default",
      targetedClients: ["claude-code"],
    });
    expect(read(world, "skills diff")).toMatchObject({ noLockfile: true });
  });

  it("changes the taps and the library with a tap add, a tap remove and an install", () => {
    const world = createSkillsWorld();
    expect(read(world, "skills tap ls").taps).toHaveLength(2);
    answer(world, "skills tap add acme/tools --name tools");
    expect(read(world, "skills tap ls").taps).toHaveLength(3);
    answer(world, "skills tap remove acme-skills");
    expect(
      read(world, "skills tap ls").taps.map((t: { name: string }) => t.name),
    ).toEqual(["local-notes", "tools"]);
    expect(read(world, "skills search review").results).toHaveLength(1);
    answer(world, "skills install @acme/skills/code-review");
    expect(read(world, "skills ls").skills).toHaveLength(36);
    answer(world, "skills install @acme/risky");
    expect(read(world, "skills ls").skills).toHaveLength(36);
    answer(world, "skills install @acme/risky --no-audit");
    expect(read(world, "skills ls").skills).toHaveLength(37);
  });

  it("starts without a repository on a fresh Mac and has one after init", () => {
    const world = createSkillsWorld({ repo: false });
    const first = answer(world, "skills ls");
    expect(first).toBeInstanceOf(Failure);
    expect((first as Failure).message).toMatch(/no skills repository/);
    answer(world, "skills init --path /fixture/new --name team --dry-run");
    expect(answer(world, "skills ls")).toBeInstanceOf(Failure);
    answer(world, "skills init --path /fixture/new --name team");
    expect(read(world, "skills ls").skills).toEqual([]);
    expect(read(world, "skills status")).toMatchObject({ lockfilePresent: false });
  });

  it("answers a project write with the project and leaves the user-level state alone", () => {
    const world = createSkillsWorld();
    const before = JSON.stringify(read(world, "skills status"));
    expect(read(world, "skills clean --project --repo /fixture/proj")).toMatchObject({
      scope: "project",
      cleanRoot: "/fixture/proj",
    });
    expect(
      read(world, "skills sync --project --repo /fixture/proj --client claude-code"),
    ).toMatchObject({ globalMode: false, outputRoot: "/fixture/proj" });
    expect(JSON.stringify(read(world, "skills status"))).toEqual(before);
    expect(read(world, "skills ls").skills).toHaveLength(35);
  });

  it("shows a check that exits 1 as its data in the browser, which has no exit codes", () => {
    const world = createSkillsWorld({ browser: true });
    expect(answer(world, "skills diff")).toMatchObject({
      clean: false,
      new: ["feature-spec"],
    });
  });
});
