import { lazy } from "react";
import type { ProbeResult, Registry, ServerEntry } from "@/lib/types";
import { Guard } from "../Guard";
import type { PlusHost } from "./ServerOperations";

export type { PlusHost } from "./ServerOperations";

const ServersBanner = lazy(() => import("./ServersBanner"));
const ServersListEnd = lazy(() =>
  import("./ServersBanner").then((m) => ({ default: m.ServersListEnd })),
);
const ServerOperations = lazy(() => import("./ServerOperations"));
const AccessCompare = lazy(() => import("./AccessCompare"));

/** The seams App.tsx and SettingsView.tsx call, one line each. The Servers screen of the fork
 * (slice S2) is drawn by what these mount. */
export function PlusServersTop(props: { registry: Registry | null; host: PlusHost }) {
  return (
    <Guard name="The fork's server rows">
      <ServersBanner {...props} />
    </Guard>
  );
}

export function PlusServersEnd(props: { registry: Registry | null; host: PlusHost }) {
  return (
    <Guard name="The default access line">
      <ServersListEnd {...props} />
    </Guard>
  );
}

export function PlusServerDetail(props: {
  server: ServerEntry;
  registry: Registry | null;
  health?: ProbeResult;
  host: PlusHost;
  onRegistryChange: (registry: Registry) => void;
}) {
  return (
    <Guard name="The Operations list">
      <ServerOperations {...props} />
    </Guard>
  );
}

export function PlusAccessCompare(props: {
  registry: Registry | null;
  onRegistryChange: (registry: Registry) => void;
}) {
  return (
    <Guard name="The access set table">
      <AccessCompare {...props} />
    </Guard>
  );
}
