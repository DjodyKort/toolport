import { useState } from "react";
import {
  ChevronRight,
  CircleCheck,
  DatabaseBackup,
  Info,
  RotateCw,
  SquareTerminal,
  TriangleAlert,
} from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { SectionHeader } from "@/components/ui/section-header";
import { SkeletonRows } from "@/components/ui/skeleton";
import { StatusCard } from "@/components/ui/status-card";
import { getRegistry } from "@/lib/api";
import { toastError } from "@/lib/toast";
import type { Registry } from "@/lib/types";
import { ctlData } from "../bridge/ctl";
import type { McpInstallData, McpUninstallData } from "../bridge/data";
import { rememberFolder } from "../context/useFolder";
import MemoryOverview from "../memory/MemoryOverview";
import { AllCommands } from "./AllCommands";
import { SetupPage } from "../setup/SetupPage";
import { Installation } from "./Installation";
import { leadFor, type LeadVariant } from "./model";
import { SelfManagement } from "./SelfManagement";
import { bridgeDown, errorText, useOverview } from "./useCtl";

const LEAD_ICON: Record<LeadVariant, React.ReactNode> = {
  danger: <TriangleAlert />,
  warning: <TriangleAlert />,
  success: <CircleCheck />,
  info: <Info />,
};

export interface ToolportPlusPageProps {
  registry: Registry | null;
  onRegistryChange: (registry: Registry) => void;
  onOpenServers: () => void;
}

/** Settings > Toolport+ (slice S1): the state of Toolport+, the self-management server, the
 * installation checks, and the way into every toolportctl command. */
export default function ToolportPlusPage({
  registry,
  onRegistryChange,
  onOpenServers,
}: ToolportPlusPageProps) {
  const { overview, refreshing, reload } = useOverview();
  const [busy, setBusy] = useState(false);
  const [showCommands, setShowCommands] = useState(false);
  const [showSetup, setShowSetup] = useState(false);

  async function change(argv: string[], done: (data: unknown) => string) {
    setBusy(true);
    try {
      const data = await ctlData<McpInstallData | McpUninstallData>(argv);
      toast.success(done(data));
      // The child process wrote the registry; the app's copy has to follow.
      onRegistryChange(await getRegistry());
      reload();
    } catch (error) {
      toastError("Could not change self-management", { description: errorText(error) });
    } finally {
      setBusy(false);
    }
  }

  if (overview === null) {
    return <SkeletonRows label="Loading Toolport+…" />;
  }

  if (bridgeDown(overview)) {
    return (
      <StatusCard
        role="alert"
        variant="danger"
        icon={<TriangleAlert />}
        title="Toolport+ tools are not available"
        detail={`The app could not run toolportctl: ${overview.status.error}`}
        actions={
          <Button variant="outline" size="sm" onClick={reload} disabled={refreshing}>
            <RotateCw className={refreshing ? "animate-spin" : undefined} /> Retry
          </Button>
        }
      />
    );
  }

  if (showSetup) return <SetupPage onBack={() => setShowSetup(false)} />;

  if (showCommands && overview.commands.data) {
    return (
      <AllCommands
        commands={overview.commands.data}
        tools={overview.tools.data}
        onBack={() => setShowCommands(false)}
      />
    );
  }

  const lead = leadFor(overview);
  const accessSets = (registry?.profiles ?? []).map((p) => ({ id: p.id, name: p.name }));

  return (
    <div className="flex flex-col gap-6">
      <div>
        <h2 className="text-base font-semibold">Toolport+</h2>
        <p className="text-meta text-muted-foreground">
          Self-management, installation checks and every toolportctl command.
        </p>
      </div>

      <StatusCard
        variant={lead.variant}
        icon={LEAD_ICON[lead.variant]}
        title={lead.title}
        detail={lead.detail}
        actions={
          <Button variant="outline" size="sm" onClick={reload} disabled={refreshing}>
            <RotateCw className={refreshing ? "animate-spin" : undefined} /> Check again
          </Button>
        }
      />

      <section aria-label="Self-management">
        <SectionHeader>Self-management</SectionHeader>
        {overview.mcp.data ? (
          <SelfManagement
            mcp={overview.mcp.data}
            tools={overview.tools.data}
            accessSets={accessSets}
            busy={busy}
            onInstall={(set) =>
              void change(
                set ? ["mcp", "install", "--access-set", set] : ["mcp", "install"],
                (data) =>
                  (data as McpInstallData).accessSet
                    ? `Listed in ${(data as McpInstallData).accessSet}`
                    : "Self-management connected",
              )
            }
            onUninstall={() =>
              change(["mcp", "uninstall"], () => "Self-management removed")
            }
            onOpenServers={onOpenServers}
          />
        ) : (
          <StatusCard
            role="alert"
            variant="danger"
            icon={<TriangleAlert />}
            title="Self-management could not be read"
            detail={overview.mcp.error ?? undefined}
          />
        )}
      </section>

      <section aria-label="Installation">
        <SectionHeader>Installation</SectionHeader>
        <Installation
          doctor={overview.doctor.data}
          status={overview.status.data}
          doctorError={overview.doctor.error}
          statusError={overview.status.error}
        />
      </section>

      <section aria-label="Memory & history">
        <SectionHeader>Memory &amp; history</SectionHeader>
        <MemoryOverview
          onOpen={(root) => {
            rememberFolder(root);
            toast.message("Context now opens on this repository", {
              description: "Choose Context in the sidebar to see it.",
            });
          }}
        />
      </section>

      <section aria-label="Setup and backup">
        <SectionHeader>Setup &amp; backup</SectionHeader>
        <div className="toolport-list">
          <div className="flex items-center gap-3 px-4 py-3">
            <div
              aria-hidden="true"
              className="grid size-8 shrink-0 place-items-center rounded-md bg-muted text-muted-foreground [&_svg]:size-4"
            >
              <DatabaseBackup />
            </div>
            <div className="min-w-0 flex-1">
              <p className="text-sm font-semibold">Setup file</p>
              <p className="text-meta text-muted-foreground">
                Export your setup, restore it on a new machine, and check what a reset
                would lose.
              </p>
            </div>
            <Button variant="outline" onClick={() => setShowSetup(true)}>
              Manage <ChevronRight />
            </Button>
          </div>
        </div>
      </section>

      <section aria-label="Commands">
        <SectionHeader>Commands</SectionHeader>
        <div className="toolport-list">
          <div className="flex items-center gap-3 px-4 py-3">
            <div
              aria-hidden="true"
              className="grid size-8 shrink-0 place-items-center rounded-md bg-muted text-muted-foreground [&_svg]:size-4"
            >
              <SquareTerminal />
            </div>
            <div className="min-w-0 flex-1">
              <p className="text-sm font-semibold">All commands</p>
              <p className="text-meta text-muted-foreground">
                Every toolportctl command with a Run button for the read-only ones.
              </p>
            </div>
            <Button
              variant="outline"
              disabled={!overview.commands.data}
              onClick={() => setShowCommands(true)}
            >
              Open <ChevronRight />
            </Button>
          </div>
        </div>
      </section>
    </div>
  );
}
