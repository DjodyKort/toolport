import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import type { View } from "@/lib/types";
import { guiParity, type GuiParityManifest } from "./guiParity";
import { NAV_GROUPS, PLUS_VIEWS, isPlusView, navItemActive } from "./nav";
import {
  checkParity,
  paritySummary,
  pendingSummary,
  summaryLine,
  type RegistrySnapshot,
} from "./guiParityCheck";

const repo = join(__dirname, "../..");
const files = {
  exists: (path: string) => existsSync(join(repo, path)),
  text: (path: string) => readFileSync(join(repo, path), "utf8"),
};

function snapshot(): RegistrySnapshot {
  const envelope = JSON.parse(
    readFileSync(
      join(repo, "src-tauri/tests/fixtures/ctl-envelopes/commands.json"),
      "utf8",
    ),
  );
  return envelope.envelope.data;
}

describe("gui parity manifest", () => {
  it("has a built screen action for every registry command and tool and nothing stale", () => {
    const registry = snapshot();
    expect(registry.commands.length).toBeGreaterThan(100);
    expect(registry.tools.length).toBeGreaterThan(70);
    const report = checkParity(guiParity, registry, files);
    const summary = paritySummary(guiParity, registry, report);
    console.info(summaryLine(summary));

    expect(report.errors).toEqual([]);
    expect(
      [...report.pendingCommands, ...report.pendingTools],
      `rows without a built screen action (${pendingSummary(guiParity, report).join(", ")})`,
    ).toEqual([]);
    expect(summary.commandRows).toBe(summary.commands);
    expect(summary.toolRows).toBe(summary.tools);
    expect(summary.rowsOnBuilt).toBe(summary.commands + summary.tools);
    expect(summary.actionsBuilt).toBe(Object.keys(guiParity.actions).length);
    expect(summary.pending).toBe(0);
    expect(summary.waivers).toBe(0);
  });
});

describe("gui parity routes", () => {
  it("resolves every built route to a view that PlusViews renders and the sidebar reaches", () => {
    const items = NAV_GROUPS.flatMap((group) => group.items);
    for (const [id, route] of Object.entries(guiParity.routes)) {
      if (route.status !== "built") continue;
      const view = route.view ?? id;
      expect(isPlusView(view), `route ${id} -> view ${view}`).toBe(true);
      expect(PLUS_VIEWS).toContain(view);
      expect(
        items.some((item) => navItemActive(item, view as View)),
        `route ${id}: no sidebar entry opens view ${view}`,
      ).toBe(true);
    }
  });

  it("sends the servers route to the control view, with the classic page under the same entry", () => {
    expect(guiParity.routes.servers.view).toBe("control");
    expect(guiParity.routes.catalog.view).toBe("commands");
    const entry = NAV_GROUPS.flatMap((group) => group.items).find(
      (item) => item.view === "control",
    );
    expect(entry?.label).toBe("Servers");
    expect(entry && navItemActive(entry, "servers")).toBe(true);
  });
});

describe("gui parity check", () => {
  const registry: RegistrySnapshot = {
    commands: [
      { id: "status", kind: "command", path: ["status"], surface: "screen" },
      { id: "server", kind: "group", path: ["server"], surface: null },
      { id: "server ls", kind: "command", path: ["server", "ls"], surface: "screen" },
      { id: "direct run", kind: "command", path: ["direct", "run"], surface: "terminal" },
    ],
    tools: [
      { name: "where_am_i", command: null },
      { name: "servers_list", command: "server ls" },
    ],
  };
  const good = (): GuiParityManifest => ({
    schemaVersion: 1,
    routes: {
      pending: { title: "Pending", status: "planned" },
      servers: { title: "Servers", status: "built", component: "src/plus/guiParity.ts" },
    },
    actions: {
      "pending.run": { route: "pending", status: "planned", summary: "run" },
      "pending.terminal": {
        route: "pending",
        status: "planned",
        summary: "term",
      },
      "pending.tool": { route: "pending", status: "planned", summary: "tool" },
      "servers.list": {
        route: "servers",
        status: "built",
        summary: "list",
        test: "src/plus/guiParity.test.ts",
      },
    },
    owners: { status: "MIG-GUI-1", server: "MIG-GUI-1", direct: "MIG-GUI-1" },
    commands: {
      status: { route: "pending", action: "pending.run", surface: "screen" },
      "server ls": { route: "servers", action: "servers.list", surface: "screen" },
      "direct run": {
        route: "pending",
        action: "pending.terminal",
        surface: "terminal",
      },
    },
    tools: {
      where_am_i: {
        route: "pending",
        action: "pending.tool",
        surface: "screen",
      },
      servers_list: { route: "servers", action: "servers.list", surface: "screen" },
    },
  });
  const errorsOf = (change: (m: GuiParityManifest) => void, reg = registry) => {
    const manifest = good();
    change(manifest);
    return checkParity(manifest, reg, files).errors.join("\n");
  };

  it("accepts a consistent manifest and lists the rows that are still pending", () => {
    const report = checkParity(good(), registry, files);
    expect(report.errors).toEqual([]);
    expect(report.pendingCommands).toEqual(["direct run", "status"]);
    expect(report.pendingTools).toEqual(["where_am_i"]);
  });

  it("counts the pending rows and the waivers for the summary line", () => {
    const manifest = good();
    (manifest.commands.status as { surface: string }).surface = "waived";
    const report = checkParity(manifest, registry, files);
    const summary = paritySummary(manifest, registry, report);
    expect(summary).toEqual({
      commands: 3,
      tools: 2,
      commandRows: 3,
      toolRows: 2,
      rowsOnBuilt: 2,
      actionsBuilt: 1,
      pending: 3,
      waivers: 1,
    });
    expect(summaryLine(summary)).toBe(
      "gui parity: 3 commands + 2 tools = 5 manifest rows, 2 rows on built screen actions (1 distinct actions), 3 pending, 1 waivers",
    );
    expect(report.errors.join("\n")).toContain("neither screen nor terminal");
  });

  it("fails on a command or tool without an entry and prints the line to add", () => {
    const message = errorsOf((m) => delete m.commands["server ls"]);
    expect(message).toContain("map it to a built route and action");
    expect(message).toContain(
      '"server ls": {"route":"<route>","action":"<route>.<action>","surface":"screen"}',
    );
    expect(errorsOf((m) => delete m.tools.where_am_i)).toContain("tool `where_am_i`");
  });

  it("fails on a stale entry for a command or tool that no longer exists", () => {
    expect(
      errorsOf((m) => {
        m.commands.gone = m.commands.status;
        m.tools.gone = m.tools.where_am_i;
      }),
    ).toMatch(/entry `gone` is not a command[\s\S]*entry `gone` is not a tool/);
  });

  it("fails on an entry that points at a missing route or action", () => {
    expect(errorsOf((m) => (m.commands.status.route = "nowhere"))).toContain(
      "route `nowhere` is not in routes",
    );
    expect(errorsOf((m) => (m.commands.status.action = "nothing.run"))).toContain(
      "action `nothing.run` is not in actions",
    );
    expect(errorsOf((m) => (m.commands.status.action = "servers.list"))).toContain(
      "belongs to route `servers`",
    );
  });

  it("fails when the surface disagrees with the registry", () => {
    expect(errorsOf((m) => (m.commands["direct run"].surface = "screen"))).toContain(
      "registry says terminal",
    );
    expect(errorsOf((m) => (m.tools.servers_list.surface = "terminal"))).toContain(
      "tool `servers_list`: surface is terminal",
    );
  });

  it("fails on a built action without a component test that names it", () => {
    expect(errorsOf((m) => delete m.actions["servers.list"].test)).toContain(
      "built but has no component test",
    );
    expect(
      errorsOf((m) => (m.actions["servers.list"].test = "src/plus/missing.test.tsx")),
    ).toContain("does not exist");
    expect(
      errorsOf((m) => (m.actions["servers.list"].test = "src/plus/guiParity.ts")),
    ).toContain("never names the action");
  });

  it("fails on a built route without its component, on dead actions and unowned groups", () => {
    expect(errorsOf((m) => delete m.routes.servers.component)).toContain(
      "built but names no component",
    );
    expect(errorsOf((m) => (m.routes.servers.component = "src/plus/none.tsx"))).toContain(
      "component src/plus/none.tsx does not exist",
    );
    expect(errorsOf((m) => (m.routes.servers.status = "planned"))).toContain(
      "is built but its route `servers` is not",
    );
    expect(
      errorsOf((m) => {
        m.actions.unused = { route: "servers", status: "planned", summary: "x" };
      }),
    ).toContain("action `unused` is not used by any command or tool");
    expect(errorsOf((m) => delete m.owners.direct)).toContain(
      "group `direct` has no owner",
    );
  });

  it("is not fooled by a camelCase or dotted command name in the registry", () => {
    const odd: RegistrySnapshot = {
      commands: [
        ...registry.commands,
        {
          id: "context bundle apply",
          kind: "command",
          path: ["context", "bundle", "apply"],
          surface: "screen",
        },
      ],
      tools: [...registry.tools, { name: "context_bundle_apply", command: null }],
    };
    const message = errorsOf(() => {}, odd);
    expect(message).toContain("command `context bundle apply` has no entry");
    expect(message).toContain("tool `context_bundle_apply` has no entry");
  });
});
