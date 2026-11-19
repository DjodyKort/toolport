import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { createPluginsWorld, PLUGIN_FOLDER } from "./pluginsWorld";

const goldens = join(__dirname, "../../../plus/fixtures/ctl-envelopes");

function golden(name: string): unknown {
  return JSON.parse(readFileSync(join(goldens, `${name}.json`), "utf8")).envelope.data;
}

/** Every object has the keys of the golden, recursively; an array stands for its first element
 * and a null on either side is not compared (the world may know what the golden world does not). */
function same(got: unknown, want: unknown, path: string) {
  if (
    got === null ||
    want === null ||
    typeof got !== "object" ||
    typeof want !== "object"
  ) {
    return;
  }
  if (/\.counts\.(byOwner|perTool|otherEvents)$/.test(path)) return;
  if (Array.isArray(want)) {
    if (!Array.isArray(got) || got.length === 0 || want.length === 0) return;
    same(got[0], want[0], `${path}[0]`);
    return;
  }
  const g = got as Record<string, unknown>;
  const w = want as Record<string, unknown>;
  const wanted = Object.keys(w).sort();
  expect(Object.keys(g).sort(), path || "data").toEqual(wanted);
  for (const key of wanted) same(g[key], w[key], `${path}.${key}`);
}

const world = () => createPluginsWorld();

function data(argv: string[], w = world()) {
  const out = w.run(argv);
  if (!out.ok) throw new Error(out.error?.message);
  return out.data;
}

describe("the Plugins fixture world", () => {
  it("test01_its_answers_have_the_keys_of_the_Rust_goldens", () => {
    const folder = ["--cwd", PLUGIN_FOLDER];
    const pairs: Array<[string, string[]]> = [
      ["plugins-ls.installed", ["plugins", "ls"]],
      ["plugins-show.one", ["plugins", "show", "lint-rules@community"]],
      ["plugins-config.one", ["plugins", "config", "lint-rules@community"]],
      [
        "plugins-config.plan",
        [
          "plugins",
          "config",
          "lint-rules@community",
          ...folder,
          "--set",
          "profile=strict",
          "--dry-run",
        ],
      ],
      [
        "plugins-config.apply",
        [
          "plugins",
          "config",
          "lint-rules@community",
          ...folder,
          "--set",
          "profile=strict",
        ],
      ],
      ["plugins-mcp-ls.servers", ["plugins", "mcp", "ls", "kit@acme"]],
      [
        "plugins-mcp-deny.plan",
        ["plugins", "mcp", "deny", "kit@acme", "docs", ...folder, "--dry-run"],
      ],
      [
        "plugins-mcp-deny.apply",
        ["plugins", "mcp", "deny", "kit@acme", "docs", ...folder],
      ],
      ["hooks-ls.all", ["hooks", "ls"]],
      ["plugins-disable.plan", ["plugins", "disable", "kit@acme", "--dry-run"]],
      ["plugins-disable.apply", ["plugins", "disable", "lint-rules@community"]],
      ["plugins-update.plan", ["plugins", "update", "kit@acme", "--dry-run"]],
      ["plugins-update.apply", ["plugins", "update", "kit@acme"]],
    ];
    for (const [name, argv] of pairs) {
      same(data(argv), golden(name), name);
    }
  });

  it("test02_a_golden_row_has_no_key_the_fixture_row_lacks", () => {
    const row = (data(["plugins", "ls"]) as { plugins: unknown[] }).plugins[0];
    const want = (golden("plugins-ls.installed") as { plugins: unknown[] }).plugins[0];
    same(row, want, "plugins[0]");
  });

  it("test03_a_denied_server_and_a_written_knob_show_on_the_next_read", () => {
    const w = world();
    const folder = ["--cwd", PLUGIN_FOLDER];
    data(["plugins", "mcp", "deny", "kit@acme", "docs", ...folder], w);
    const listed = data(["plugins", "mcp", "ls", "kit@acme", ...folder], w) as {
      servers: Array<{ denied: { local: boolean } }>;
    };
    expect(listed.servers[0].denied.local).toBe(true);
    data(
      ["plugins", "config", "lint-rules@community", ...folder, "--set", "guard=off"],
      w,
    );
    const shown = data(["plugins", "show", "lint-rules@community", ...folder], w) as {
      knobs: Array<{ key: string; current: { value: string; from: string } }>;
    };
    expect(shown.knobs.find((k) => k.key === "guard")?.current).toEqual({
      value: "off",
      from: "folder-env",
    });
  });

  it("test03b_disable_and_enable_move_the_user_scope_and_ls_asks_for_a_restart", () => {
    const w = world();
    expect(
      (data(["plugins", "ls"], w) as { restartRequired: boolean }).restartRequired,
    ).toBe(false);
    data(["plugins", "disable", "kit@acme"], w);
    const ls = data(["plugins", "ls"], w) as {
      restartRequired: boolean;
      plugins: Array<{ id: string; enabled: { user: boolean; effective: boolean } }>;
    };
    expect(ls.restartRequired).toBe(true);
    expect(ls.plugins.find((p) => p.id === "kit@acme")?.enabled).toMatchObject({
      user: false,
      effective: false,
    });
    const again = data(["plugins", "disable", "kit@acme"], w) as {
      changes: Array<{ action: string }>;
    };
    expect(again.changes[0].action).toBe("none");
  });

  it("test04_a_knob_the_adapter_does_not_have_and_a_folder_only_knob_without_a_folder_are_refused", () => {
    const w = world();
    expect(
      w.run(["plugins", "config", "lint-rules@community", "--set", "nope=1"]).ok,
    ).toBe(false);
    expect(
      w.run(["plugins", "config", "lint-rules@community", "--set", "guard=off"]).ok,
    ).toBe(false);
    expect(w.run(["plugins", "show", "ghost@x"]).error?.code).toBe("not_found");
  });
});
