import { describe, expect, it } from "vitest";
import { check } from "../bridge/shape";
import { loadsData, measureData } from "../bridge/data";
import { contextBundleShapes } from "../types/context-bundle";
import { contextLayerShapes } from "../types/context-layers";
import { Failure, createContextWorld } from "./world";

const FOLDER = "/fixture/work/erp/clients/acme-erp";
// eslint-disable-next-line @typescript-eslint/no-explicit-any
type Loose = Record<string, any>;
const ok = (value: unknown) => {
  expect(value).not.toBeInstanceOf(Failure);
  return value as Loose;
};
const fails = (value: unknown, code: string) => {
  expect(value).toBeInstanceOf(Failure);
  expect((value as Failure).code).toBe(code);
  return value as Failure;
};
const tokens = (world: ReturnType<typeof createContextWorld>, cwd = FOLDER) =>
  ok(world.run(["context", "loads", "--cwd", cwd, "--measured"])).total_tokens as number;

describe("context world, profiles and measurement", () => {
  it("answers every bundle command in the shape of the golden envelopes", () => {
    const world = createContextWorld();
    const shapes = { ...contextBundleShapes, ...contextLayerShapes };
    const argvs: Array<[string, string[]]> = [
      ["context-bundle-ls.applied", ["context", "bundle", "ls"]],
      ["context-bundle-show.bundle", ["context", "bundle", "show", "acme-dev"]],
      ["context-bundle-show.legacy", ["context", "bundle", "show", "default"]],
      ["context-bundle-status.none", ["context", "bundle", "status", "--cwd", FOLDER]],
      ["context-bundle-config.show", ["context", "bundle", "config"]],
      ["context-bundle-launch.apply", ["context", "bundle", "launch", "acme-dev"]],
      ["context-compose.layers", ["context", "compose", "--cwd", FOLDER]],
      ["context-bundle-add.preview", ["context", "bundle", "add", "fresh", "--dry-run"]],
      ["context-bundle-add.apply", ["context", "bundle", "add", "fresh"]],
      [
        "context-bundle-edit.preview",
        ["context", "bundle", "edit", "fresh", "--skills-off", "skill-01", "--dry-run"],
      ],
      [
        "context-bundle-edit.apply",
        ["context", "bundle", "edit", "fresh", "--skills-off", "skill-01"],
      ],
      ["context-bundle-rm.preview", ["context", "bundle", "rm", "fresh", "--dry-run"]],
      ["context-bundle-rm.apply", ["context", "bundle", "rm", "fresh"]],
      [
        "context-bundle-apply.plan",
        ["context", "bundle", "apply", "acme-dev", "--cwd", FOLDER, "--dry-run"],
      ],
      [
        "context-bundle-apply.result",
        ["context", "bundle", "apply", "acme-dev", "--cwd", FOLDER],
      ],
      ["context-bundle-status.clean", ["context", "bundle", "status", "--cwd", FOLDER]],
      [
        "context-bundle-undo.plan",
        ["context", "bundle", "undo", "--cwd", FOLDER, "--dry-run"],
      ],
      ["context-bundle-undo.result", ["context", "bundle", "undo", "--cwd", FOLDER]],
      [
        "context-use.bundle",
        ["context", "use", "acme-dev", "--cwd", FOLDER, "--dry-run"],
      ],
      ["context-use.bundle", ["context", "use", "acme-dev", "--cwd", FOLDER]],
      ["context-use.none", ["context", "use", "--none", "--cwd", FOLDER, "--dry-run"]],
      ["context-use.none-apply", ["context", "use", "--none", "--cwd", FOLDER]],
    ];
    for (const [stem, argv] of argvs) {
      const data = world.run(argv);
      expect(check(shapes[stem], data), `${stem}: ${argv.join(" ")}`).toEqual([]);
    }
    expect(
      check(loadsData, world.run(["context", "loads", "--cwd", FOLDER, "--measured"])),
    ).toEqual([]);
    expect(
      check(measureData, world.run(["context", "measure", "--cwd", FOLDER, "--yes"])),
    ).toEqual([]);
  });

  it("never changes anything for a preview, and an applied profile changes the next reads", () => {
    const world = createContextWorld();
    const before = tokens(world);
    ok(
      world.run(["context", "bundle", "apply", "acme-dev", "--cwd", FOLDER, "--dry-run"]),
    );
    ok(world.run(["context", "use", "acme-dev", "--cwd", FOLDER, "--dry-run"]));
    expect(tokens(world)).toBe(before);
    expect(
      ok(world.run(["context", "bundle", "status", "--cwd", FOLDER])).applied,
    ).toBeNull();

    const plan = ok(
      world.run(["context", "bundle", "apply", "acme-dev", "--cwd", FOLDER, "--dry-run"]),
    );
    expect(plan.plan.effects.tokens.before).toBe(before);
    expect(plan.plan.effects.tokens.basis).toBe("estimate");
    ok(world.run(["context", "bundle", "apply", "acme-dev", "--cwd", FOLDER]));
    expect(tokens(world)).toBe(plan.plan.effects.tokens.after);
    expect(tokens(world)).toBeLessThan(before);
    const status = ok(world.run(["context", "bundle", "status", "--cwd", FOLDER]));
    expect(status.applied).toMatchObject({ bundle: "acme-dev", drift: false });
    const list = ok(world.run(["context", "bundle", "ls"]));
    expect(
      list.bundles[0].appliedTo.map((one: { folder: string }) => one.folder),
    ).toEqual([FOLDER]);
    const loaded = ok(world.run(["context", "loads", "--cwd", FOLDER, "--measured"]));
    expect(
      loaded.items
        .filter((one: { kind: string; name: string }) => one.kind === "skill")
        .map((one: { name: string }) => one.name),
    ).not.toContain("skill-03");

    ok(world.run(["context", "bundle", "undo", "--cwd", FOLDER]));
    expect(tokens(world)).toBe(before);
    expect(
      ok(world.run(["context", "bundle", "status", "--cwd", FOLDER])).applied,
    ).toBeNull();
    fails(world.run(["context", "bundle", "undo", "--cwd", FOLDER]), "conflict");
  });

  it("measures for real only with --yes, keeps the run, and goes stale when the estimate moves", () => {
    const world = createContextWorld();
    fails(world.run(["context", "measure", "--cwd", FOLDER]), "confirm_required");
    expect(
      ok(world.run(["context", "loads", "--cwd", FOLDER, "--measured"])).measured,
    ).toBeNull();
    const first = ok(world.run(["context", "measure", "--cwd", FOLDER, "--yes"]));
    expect(first.cached).toBe(false);
    expect(first.runs).toHaveLength(1);
    expect(ok(world.run(["context", "measure", "--cwd", FOLDER, "--yes"])).cached).toBe(
      true,
    );
    const loaded = ok(world.run(["context", "loads", "--cwd", FOLDER, "--measured"]));
    expect(loaded.measured.total).toBe(first.runs[0].total);
    expect(loaded.measured_info.stale).toBe(false);

    const without = ok(
      world.run([
        "context",
        "measure",
        "--cwd",
        FOLDER,
        "--without",
        "plugin:kit@market",
        "--yes",
      ]),
    );
    expect(without.runs).toHaveLength(2);
    expect(without.deltas[0].tokens).toBeLessThan(0);
    fails(
      world.run([
        "context",
        "measure",
        "--cwd",
        FOLDER,
        "--without",
        "plugin:none@market",
        "--yes",
      ]),
      "not_found",
    );

    ok(world.run(["context", "use", "acme-dev", "--cwd", FOLDER]));
    const stale = ok(world.run(["context", "loads", "--cwd", FOLDER, "--measured"]));
    expect(stale.measured_info.stale).toBe(true);
    fails(
      world.run([
        "context",
        "measure",
        "--cwd",
        FOLDER,
        "--without",
        "plugin:kit@market",
        "--yes",
      ]),
      "not_found",
    );
  });

  it("writes the profile list: add, edit, empty a list, remove", () => {
    const world = createContextWorld({ fresh: true });
    expect(ok(world.run(["context", "bundle", "ls"])).bundles).toEqual([]);
    ok(
      world.run([
        "context",
        "bundle",
        "add",
        "lean",
        "--skills-off",
        "skill-01,skill-02",
      ]),
    );
    fails(world.run(["context", "bundle", "add", "lean"]), "conflict");
    expect(ok(world.run(["context", "bundle", "show", "lean"])).skills.off).toEqual([
      "skill-01",
      "skill-02",
    ]);
    ok(world.run(["context", "bundle", "edit", "lean", "--skills-off", ""]));
    expect(ok(world.run(["context", "bundle", "show", "lean"])).skills.off).toEqual([]);
    ok(world.run(["context", "bundle", "apply", "lean", "--cwd", FOLDER]));
    fails(world.run(["context", "bundle", "rm", "lean"]), "conflict");
    ok(world.run(["context", "bundle", "rm", "lean", "--force"]));
    fails(world.run(["context", "bundle", "show", "lean"]), "not_found");
  });

  it("composes the text of the layers that match the folder, and keeps the auto-apply switch", () => {
    const world = createContextWorld();
    const composed = ok(world.run(["context", "compose", "--cwd", FOLDER]));
    expect(composed.parts.map((one: { name: string }) => one.name)).toContain(
      "CLAUDE.local.md",
    );
    expect(ok(world.run(["context", "bundle", "config"])).autoApply).toBe(false);
    expect(
      ok(world.run(["context", "bundle", "config", "--auto-apply", "on"])).autoApply,
    ).toBe(true);
    expect(ok(world.run(["context", "bundle", "config"])).autoApply).toBe(true);
  });
});
