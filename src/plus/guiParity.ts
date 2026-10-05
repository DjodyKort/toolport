import manifest from "./gui-parity.json";

export type GuiSurface = "screen" | "terminal";
export type GuiStatus = "planned" | "built";

export interface GuiRoute {
  title: string;
  /** The view id `PlusViews` renders for the route, when it is not the route id itself. */
  view?: string;
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

/** `src/plus/gui-parity.json` (D-058, R3): where every registry command and self-management
 * tool lives in the app. A row whose action is not `built` is not done. */
export interface GuiParityManifest {
  schemaVersion: 1;
  routes: Record<string, GuiRoute>;
  actions: Record<string, GuiAction>;
  /** Command group (first word of the id) to the item that gives it a dedicated screen. */
  owners: Record<string, string>;
  commands: Record<string, GuiEntry>;
  tools: Record<string, GuiEntry>;
}

export const guiParity = manifest as GuiParityManifest;
