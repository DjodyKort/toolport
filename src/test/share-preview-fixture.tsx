// Development-only controlled rendering of the actual Tauri preview and confirmation.
import { createRoot } from "react-dom/client";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { TeamSharePreview } from "@/components/TeamSharePreview";
import { teamShareAction } from "@/lib/teamShare";
import type { ShareSelectionPreview, TeamPushPreview } from "@/lib/api";
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
const selection = (
  name: string,
  teamChange: ShareSelectionPreview["teamChange"],
  teamDetail: string,
  outcome: ShareSelectionPreview["local"]["outcome"],
  message: string,
  notes: string[] = [],
): ShareSelectionPreview => ({
  id: name.toLowerCase(),
  name,
  teamChange,
  teamDetail,
  notes,
  local: { id: name.toLowerCase(), name, outcome, message },
});
const switches =
  "This profile switches to the Team copy. Your personal server stays saved and turns off here.";
const definitions =
  mode === "existing"
    ? []
    : mode === "stdio"
      ? [stdio]
      : mode === "http"
        ? [http]
        : [stdio, http];
// Sharing into a Team that already has definitions: one already shared, one whose
// Team copy has its own sign-in, and a same-name definition that stays separate.
const existing = [
  selection(
    "Linear",
    "Already shared",
    "The Team already has this exact definition, so nothing changes for the team.",
    "switched",
    switches,
    [
      "The team also has a separate definition named Linear (ID linear-2). It stays separate because sharing matches server IDs, not names.",
    ],
  ),
  selection(
    "Vercel (Full API)",
    "Already shared",
    "The Team already has this exact definition, so nothing changes for the team.",
    "attention",
    "This team copy already has its own local credentials. Keep its existing setup and enable it separately. Your personal server stays on in this profile.",
  ),
];
const preview: TeamPushPreview = {
  baseVersion: 3,
  localFingerprint: "fixture",
  definitions,
  added: definitions.filter((d) => d.change === "Added").map((d) => d.name),
  changed: definitions.filter((d) => d.change === "Changed").map((d) => d.name),
  removed: mode === "multiple" ? ["Retired tools"] : [],
  selections:
    mode === "existing"
      ? existing
      : definitions.map((d) =>
          selection(
            d.name,
            d.change === "Added" ? "New" : "Update",
            d.change === "Added"
              ? "Adds a new Team definition."
              : "Replaces the Team definition with the same server ID. Members review the change before it runs for them.",
            "switched",
            switches,
          ),
        ),
};
// This synthetic value is deliberately outside the display allowlist.
Object.assign(preview, { credentialValue: "SYNTHETIC_PREVIEW_SECRET_MUST_NOT_RENDER" });
createRoot(document.getElementById("root")!).render(
  <ConfirmDialog
    open
    contentClassName="sm:max-w-lg"
    title="Share selected servers with your team?"
    description={<TeamSharePreview preview={preview} />}
    confirmLabel={teamShareAction(preview) ?? "Share selected"}
    confirmDisabled={!teamShareAction(preview)}
    onConfirm={() => {}}
  />,
);
