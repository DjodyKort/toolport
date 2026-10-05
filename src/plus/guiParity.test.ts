import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import type { View } from "@/lib/types";
import { guiParity, type GuiParityManifest } from "./guiParity";
import { NAV_GROUPS, PLUS_VIEWS, isPlusView, navItemActive } from "./nav";
import { checkParity, pendingSummary, type RegistrySnapshot } from "./guiParityCheck";

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
  it("has an entry for every registry command and tool and nothing stale", () => {
    const registry = snapshot();
    expect(registry.commands.length).toBeGreaterThan(100);
    expect(registry.tools.length).toBeGreaterThan(70);
    const report = checkParity(guiParity, registry, files);
    expect(report.errors).toEqual([]);

    const pending = [...report.pendingCommands, ...report.pendingTools];
    if (process.env.GUI_PARITY_STRICT) {
      expect(pending, "rows still only on the All commands page").toEqual([]);
    } else {
      console.info(
        `gui parity: ${report.pendingCommands.length} commands and ${report.pendingTools.length} tools ` +
          `are only on the All commands page (${pendingSummary(guiParity, report).join(", ")})`,
      );
    }
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
      "all-commands": { title: "All commands", status: "planned" },
      servers: { title: "Servers", status: "built", component: "src/plus/guiParity.ts" },
    },
    actions: {
      "all-commands.run": { route: "all-commands", status: "planned", summary: "run" },
      "all-commands.terminal": {
        route: "all-commands",
        status: "planned",
        summary: "term",
      },
      "all-commands.tool": { route: "all-commands", status: "planned", summary: "tool" },
      "servers.list": {
        route: "servers",
        status: "built",
        summary: "list",
        test: "src/plus/guiParity.test.ts",
      },
    },
    owners: { status: "MIG-GUI-1", server: "MIG-GUI-1", direct: "MIG-GUI-1" },
    commands: {
      status: { route: "all-commands", action: "all-commands.run", surface: "screen" },
      "server ls": { route: "servers", action: "servers.list", surface: "screen" },
      "direct run": {
        route: "all-commands",
        action: "all-commands.terminal",
        surface: "terminal",
      },
    },
    tools: {
      where_am_i: {
        route: "all-commands",
        action: "all-commands.tool",
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

  it("fails on a command or tool without an entry and prints the line to add", () => {
    expect(errorsOf((m) => delete m.commands["server ls"])).toContain(
      '"server ls": {"route":"all-commands","action":"all-commands.run","surface":"screen"}',
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
