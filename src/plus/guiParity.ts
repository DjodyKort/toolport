const meta = import.meta.glob("./gui-parity/_meta.json", {
  eager: true,
  import: "default",
});
const entries = import.meta.glob("./gui-parity/*/*.json", {
  eager: true,
  import: "default",
});

export type GuiSurface = "screen" | "terminal";
export type GuiStatus = "planned" | "built";

export interface GuiRoute {
  title: string;
  /** Where the shell shows the route: `settings:<page id>` or `view:<id of a Claude group page>`. */
  page?: string;
  status: GuiStatus;
  /** Repo-relative path of the component; required once the route is built. */
  component?: string;
}

export interface GuiAction {
  route: string;
  summary: string;
  status: GuiStatus;
  /** Repo-relative component test that names the action id; required once it is built. */
  test?: string;
}

export interface GuiEntry {
  route: string;
  action: string;
  surface: GuiSurface;
}

/** `src/plus/gui-parity/` (D-058, R3), one file per entry: where every registry command and
 * self-management tool lives in the app. A row whose action is not `built` is not done. */
export interface GuiParityManifest {
  schemaVersion: 1;
  routes: Record<string, GuiRoute>;
  actions: Record<string, GuiAction>;
  /** Command group (first word of the id) to the item that gives it a dedicated screen. */
  owners: Record<string, string>;
  commands: Record<string, GuiEntry>;
  tools: Record<string, GuiEntry>;
}

/** A command id is stored with its spaces as underscores (`server_ls.json`). */
function section(name: string): Record<string, unknown> {
  const rows: Record<string, unknown> = {};
  for (const [path, value] of Object.entries(entries)) {
    const [, group, file] = /\/gui-parity\/([^/]+)\/([^/]+)\.json$/.exec(path) ?? [];
    if (group === name)
      rows[name === "commands" ? file.replace(/_/g, " ") : file] = value;
  }
  return rows;
}

export const guiParity = {
  ...(Object.values(meta)[0] as { schemaVersion: 1 }),
  routes: section("routes"),
  actions: section("actions"),
  owners: section("owners"),
  commands: section("commands"),
  tools: section("tools"),
} as GuiParityManifest;
