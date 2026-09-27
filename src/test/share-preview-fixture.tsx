// Development-only controlled rendering of the actual Tauri preview and confirmation.
import { createRoot } from "react-dom/client";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { TeamSharePreview } from "@/components/TeamSharePreview";
import type { TeamPushPreview } from "@/lib/api";
import "../index.css";

if (!import.meta.env.DEV) throw new Error("Fixtures require the development server");
const mode = new URLSearchParams(location.search).get("case") || "multiple";
const stdio = {
  id: "local",
  name: "Project tools",
  change: "Added" as const,
  transport: "stdio",
  fields: [
    { label: "Command", value: "python3" },
    {
      label: "Arguments",
      value:
        '"/home/test/projects/a-long-workspace-name/services/tool-server/server.py"\n"--workspace"\n"/home/test/projects/team workspace"\n"--verbose"',
    },
    {
      label: "Working directory",
      value: "/home/test/projects/a-long-workspace-name/services/tool-server",
    },
    {
      label: "Environment / credential keys",
      value: "GITHUB_TOKEN, WORKSPACE_KEY, SERVICE_ACCOUNT_TOKEN",
    },
  ],
};
const http = {
  id: "http",
  name: "Internal knowledge",
  change: "Changed" as const,
  transport: "http",
  fields: [
    {
      label: "Endpoint",
      value:
        "https://example.internal/platform/knowledge/mcp?workspace=engineering&region=us-east",
    },
    { label: "Environment / credential keys", value: "API_TOKEN" },
  ],
};
const definitions = mode === "stdio" ? [stdio] : mode === "http" ? [http] : [stdio, http];
const preview: TeamPushPreview = {
  baseVersion: 3,
  localFingerprint: "fixture",
  definitions,
  added: definitions.filter((d) => d.change === "Added").map((d) => d.name),
  changed: definitions.filter((d) => d.change === "Changed").map((d) => d.name),
  removed: mode === "multiple" ? ["Retired tools"] : [],
};
// This synthetic value is deliberately outside the display allowlist.
Object.assign(preview, { credentialValue: "SYNTHETIC_PREVIEW_SECRET_MUST_NOT_RENDER" });
createRoot(document.getElementById("root")!).render(
  <ConfirmDialog
    open
    contentClassName="sm:max-w-lg"
    title="Share selected servers with your team?"
    description={<TeamSharePreview preview={preview} />}
    confirmLabel="Share selected"
    onConfirm={() => {}}
  />,
);
