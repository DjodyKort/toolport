import { describe, expect, it } from "vitest";
import { check } from "../bridge/shape";
import { contextShapes } from "../types/context";
import { Failure, createContextWorld } from "./world";

const ok = (value: unknown) => {
  expect(value).not.toBeInstanceOf(Failure);
  return value as Record<string, unknown>;
};
const fails = (value: unknown) => {
  expect(value).toBeInstanceOf(Failure);
  return value as Failure;
};

describe("context world", () => {
  it("answers every command in the shape of the golden envelopes", () => {
    const world = createContextWorld();
    const argvs: Array<[string, string[]]> = [
      ["context-status", ["context", "status"]],
      ["context-plan", ["context", "plan"]],
      ["context-apply.preview", ["context", "apply", "--dry-run"]],
      ["context-apply.apply", ["context", "apply"]],
      ["context-sync.preview", ["context", "sync", "--dry-run"]],
      ["context-sync.apply", ["context", "sync"]],
      ["context-init.preview", ["context", "init", "--yes", "--dry-run"]],
      ["context-init.apply", ["context", "init", "--yes"]],
      ["context-client-list", ["context", "client", "list"]],
      ["context-client-add.preview", ["context", "client", "add", "acme", "--dry-run"]],
      ["context-profile-list", ["context", "profile", "list"]],
      [
        "context-profile-add.preview",
        ["context", "profile", "add", "work", "--rules", "none", "--dry-run"],
      ],
      ["context-profile-add.apply", ["context", "profile", "add", "work"]],
      [
        "context-profile-remove.preview",
        ["context", "profile", "remove", "work", "--purge", "--dry-run"],
      ],
      ["context-disable.preview", ["context", "disable", "--dry-run"]],
      ["context-loads.home", ["context", "loads"]],
      ["context-folders", ["context", "folders"]],
      ["context-profile-remove.apply", ["context", "profile", "remove", "work"]],
      ["context-client-add.apply", ["context", "client", "add", "partner"]],
      ["context-disable.apply", ["context", "disable", "--purge-profiles"]],
    ];
    for (const [stem, argv] of argvs) {
      const data = world.run(argv);
      expect(check(contextShapes[stem], data), `${stem}: ${argv.join(" ")}`).toEqual([]);
    }
    expect(
      check(
        contextShapes["context-checkpoint-status.status"],
        world.run(["context", "checkpoint-status", "--checkpoint-at", "50000"], "{}"),
      ),
    ).toEqual([]);
  });

  it("never changes anything for a preview, and changes the next read for an apply", () => {
    const world = createContextWorld({ fresh: true });
    const before = JSON.stringify(world.run(["context", "status"]));
    for (const argv of [
      ["context", "init", "--yes", "--dry-run"],
      ["context", "client", "add", "acme", "--dry-run"],
      ["context", "profile", "add", "work", "--dry-run"],
      ["context", "apply", "--dry-run"],
      ["context", "sync", "--dry-run"],
      ["context", "disable", "--dry-run"],
    ]) {
      ok(world.run(argv));
    }
    expect(JSON.stringify(world.run(["context", "status"]))).toBe(before);

    ok(world.run(["context", "init", "--yes"]));
    ok(world.run(["context", "client", "add", "acme"]));
    ok(world.run(["context", "profile", "add", "work", "--no-org", "--servers", "a,b"]));
    const status = ok(world.run(["context", "status"])) as {
      config: { exists: boolean };
      layers: Array<{ name: string; globs: string[] }>;
      profiles: Array<{ name: string; org: boolean; servers: unknown }>;
      shims: { exists: boolean };
    };
    expect(status.config.exists).toBe(true);
    expect(status.layers.map((layer) => layer.name)).toEqual(["personal", "client-acme"]);
    expect(status.layers[1].globs).toEqual(["**/clients/acme/**"]);
    expect(status.profiles[0]).toMatchObject({
      name: "work",
      org: false,
      servers: ["a", "b"],
    });
    expect(status.shims.exists).toBe(true);

    ok(world.run(["context", "disable", "--purge-profiles"]));
    expect(
      (ok(world.run(["context", "status"])) as { shims: { exists: boolean } }).shims
        .exists,
    ).toBe(false);
    expect(
      (
        ok(world.run(["context", "profile", "list"])) as {
          profiles: Array<{ generated: boolean }>;
        }
      ).profiles[0].generated,
    ).toBe(false);
    ok(world.run(["context", "apply"]));
    expect(world.snapshot().shims).toBe(true);
    expect(world.snapshot().profiles[0].generated).toBe(true);
  });

  it("moves the legacy shell lines only with --rewrite-zshrc", () => {
    const world = createContextWorld();
    ok(world.run(["context", "sync"]));
    expect(world.snapshot().legacy).toHaveLength(1);
    const preview = ok(
      world.run(["context", "sync", "--rewrite-zshrc", "--dry-run"]),
    ) as {
      plan: { zshrc: { changes: unknown[] } };
    };
    expect(preview.plan.zshrc.changes).toHaveLength(1);
    expect(world.snapshot().legacy).toHaveLength(1);
    ok(world.run(["context", "sync", "--rewrite-zshrc"]));
    expect(world.snapshot().legacy).toEqual([]);
  });

  it("derives what loads from the layers, the profile and the org file", () => {
    const world = createContextWorld({ fresh: true });
    const total = () => ok(world.run(["context", "loads"])).total_tokens as number;
    const fresh = total();
    ok(world.run(["context", "init", "--yes"]));
    expect(total()).toBeGreaterThan(fresh);
    ok(
      world.run([
        "context",
        "profile",
        "add",
        "lean",
        "--no-org",
        "--rules",
        "none",
        "--servers",
        "none",
      ]),
    );
    const lean = ok(world.run(["context", "loads", "--profile", "lean"])) as {
      total_tokens: number;
      profile: string;
    };
    expect(lean).toMatchObject({ total_tokens: 0, profile: "lean" });
    expect(fails(world.run(["context", "loads", "--profile", "nope"])).code).toBe(
      "not_found",
    );
  });

  it("turns folder routing on and off and reads the checkpoint from stdin", () => {
    const world = createContextWorld();
    const enabled = (argv: string[]) => ok(world.run(argv)).enabled;
    expect(enabled(["context", "folders"])).toBe(false);
    expect(enabled(["context", "folders", "--enable"])).toBe(true);
    expect(enabled(["context", "folders"])).toBe(true);
    expect(enabled(["context", "folders", "--disable"])).toBe(false);

    const json = JSON.stringify({
      context_window: { context_window_size: 200000, used_percentage: 80 },
    });
    expect(ok(world.run(["context", "checkpoint-status"], json))).toMatchObject({
      used_tokens: 160000,
      at_checkpoint: true,
      checkpoint_point: 150000,
    });
    expect(fails(world.run(["context", "checkpoint-status"], "")).message).toMatch(
      /statusline JSON/,
    );
  });

  it("fails like the CLI on a missing name or an unknown profile", () => {
    const world = createContextWorld();
    expect(fails(world.run(["context", "profile", "add"])).code).toBe("usage");
    expect(fails(world.run(["context", "client", "add"])).code).toBe("usage");
    expect(fails(world.run(["context", "profile", "remove"])).code).toBe("usage");
    expect(fails(world.run(["context", "profile", "remove", "nope"])).code).toBe(
      "not_found",
    );
    expect(fails(world.run(["context", "profile", "add", "-bad"])).code).toBe(
      "bad_input",
    );
  });

  it("lists argv rows for the dev browser that read the live world", () => {
    const rows = new Map(createContextWorld().rows());
    expect(rows.has("context status")).toBe(true);
    expect(rows.has("context profile remove bare --purge --dry-run")).toBe(true);
    expect(rows.has("context sync --rewrite-zshrc --dry-run")).toBe(true);
    expect(rows.has("context disable --purge-profiles")).toBe(true);
    const sync = rows.get("context sync")!;
    const status = rows.get("context status")!;
    sync();
    expect((status() as { shims: { exists: boolean } }).shims.exists).toBe(true);
    const gauge = rows.get("context checkpoint-status --checkpoint-at 50000")!();
    expect(gauge).toMatchObject({ used_tokens: 1500, window_source: "model" });
  });
});
