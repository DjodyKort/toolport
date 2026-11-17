import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { createContextWorld, FIXTURE_FOLDER } from "./contextWorld";

const goldens = join(__dirname, "../../../plus/fixtures/ctl-envelopes");

function golden(name: string): unknown {
  return JSON.parse(readFileSync(join(goldens, `${name}.json`), "utf8")).envelope.data;
}

/** The keys of a value, recursively, with an array reduced to the keys of its first element. */
function shape(value: unknown): unknown {
  if (Array.isArray(value)) return value.length ? [shape(value[0])] : [];
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>)
        .sort(([a], [b]) => a.localeCompare(b))
        .map(([key, inner]) => [key, shape(inner)]),
    );
  }
  return typeof value === "string" ? "text" : value === null ? null : typeof value;
}

function reply(argv: string[], world = createContextWorld({ measured: "fresh" })) {
  const out = world.run(argv);
  if (!out.ok) throw new Error(out.error?.message);
  return out.data;
}

const cwd = ["--cwd", FIXTURE_FOLDER];

describe("the Context fixture world", () => {
  it("test01_its_answers_have_the_keys_of_the_Rust_goldens", () => {
    const pairs: Array<[string, string[]]> = [
      ["context-bundle-config.show", ["context", "bundle", "config"]],
      ["context-bundle-status.none", ["context", "bundle", "status", ...cwd]],
      ["context-bundle-launch.launch", ["context", "bundle", "launch", "Stage"]],
      ["context-measure.plan", ["context", "measure", ...cwd, "--force"]],
      ["context-sync.off", ["context", "sync", "--dry-run"]],
    ];
    for (const [name, argv] of pairs) {
      const got = shape(reply(argv, createContextWorld()));
      const want = shape(golden(name));
      expect(Object.keys(got as object), name).toEqual(Object.keys(want as object));
    }
  });

  it("test02_loads_status_and_leave_out_carry_every_key_the_screen_reads", () => {
    const world = createContextWorld({ measured: "fresh" });
    const loads = reply(["context", "loads", ...cwd, "--measured"], world) as Record<
      string,
      unknown
    >;
    expect(Object.keys(shape(loads) as object)).toEqual(
      Object.keys(shape(golden("context-loads.measured")) as object),
    );
    const items = loads.items as Array<Record<string, unknown>>;
    expect(Object.keys(shape(items[0]) as object)).toEqual(
      Object.keys(
        shape(
          (golden("context-loads.measured") as { items: unknown[] }).items[0],
        ) as object,
      ),
    );
    const status = reply(["context", "status", ...cwd], world);
    expect(Object.keys(shape(status) as object)).toEqual(
      Object.keys(shape(golden("context-status.folder")) as object),
    );
    const leave = reply(
      ["context", "leave-out", "add", "/x/CLAUDE.md", ...cwd, "--dry-run"],
      world,
    );
    expect(Object.keys(shape(leave) as object)).toEqual(
      Object.keys(shape(golden("context-leave-out.plan")) as object),
    );
    const plugin = reply(["plugins", "off", "kit@market", ...cwd], world) as {
      plan: object;
    };
    expect(Object.keys(plugin.plan).sort()).toEqual(
      Object.keys((golden("plugins-off.apply") as { plan: object }).plan).sort(),
    );
  });

  it("test03_bundle_rows_have_the_keys_of_bundle_ls_and_bundle_show", () => {
    const world = createContextWorld();
    const list = reply(["context", "bundle", "ls"], world) as { bundles: unknown[] };
    const want = golden("context-bundle-ls") as { bundles: unknown[] };
    const keys = (row: unknown) =>
      Object.keys(shape(row) as object)
        .filter((k) => k !== "memory")
        .sort();
    expect(keys(list.bundles[0])).toEqual(keys(want.bundles[0]));
    const show = reply(["context", "bundle", "show", "Stage"], world);
    const wantShow = golden("context-bundle-show.bundle") as object;
    expect(Object.keys(show as object).sort()).toEqual(
      expect.arrayContaining(Object.keys(wantShow).filter((k) => k !== "legacy")),
    );
  });
});
