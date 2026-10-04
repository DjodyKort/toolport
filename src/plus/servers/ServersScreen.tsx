import { useState } from "react";
import { History, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Tabs } from "../ui";
import { ClientsTab } from "./ClientsTab";
import { GatewayStrip } from "./GatewayStrip";
import { HealthTab } from "./HealthTab";
import { ProfilesTab } from "./ProfilesTab";
import { ServersTab } from "./ServersTab";
import { SLOT_TABS } from "./slotTabs";
import { Slot } from "./slots";
import { ServersProvider } from "./ServersProvider";
import { useRestoreFocus } from "./useRestoreFocus";
import { useServers, type TabId } from "./useServers";

function Body({
  tab,
  setTab,
  onOpenCommands,
  onOpenClassic,
}: {
  tab: TabId;
  setTab: (tab: TabId) => void;
  onOpenCommands: (group?: string) => void;
  onOpenClassic?: () => void;
}) {
  const { views, profiles, clients } = useServers();
  const [doctorRun, setDoctorRun] = useState(0);
  useRestoreFocus();
  const slot = SLOT_TABS.find((entry) => entry.id === tab);
  return (
    <div className="flex flex-col gap-4">
      <div className="flex justify-end gap-2">
        {tab === "servers" && onOpenClassic && (
          <Button variant="ghost" onClick={onOpenClassic}>
            <History /> Classic view
          </Button>
        )}
        <Button
          variant="outline"
          onClick={() => {
            setDoctorRun((n) => n + 1);
            setTab("health");
          }}
        >
          <RefreshCw /> Run doctor
        </Button>
      </div>
      <Tabs
        label="Servers sections"
        value={tab}
        onValueChange={(id) => setTab(id as TabId)}
        items={[
          { id: "servers", label: "Servers", count: views?.length },
          { id: "profiles", label: "Profiles", count: profiles.data?.profiles.length },
          { id: "clients", label: "Clients", count: clients.data?.clients.length },
          ...SLOT_TABS.map(({ id, label }) => ({ id, label })),
          { id: "health", label: "Health" },
        ]}
      >
        <div className="flex flex-col gap-4">
          {!slot && <GatewayStrip />}
          {tab === "servers" && <ServersTab />}
          {tab === "profiles" && <ProfilesTab />}
          {tab === "clients" && <ClientsTab />}
          {tab === "health" && <HealthTab key={doctorRun} />}
          {slot && <Slot tab={slot} onOpenCommands={onOpenCommands} />}
        </div>
      </Tabs>
    </div>
  );
}

/** The Servers screen of the control center: every server with its live state, the profiles,
 * what each client sees, and the health of the whole. Every read and write goes through
 * `toolportctl` (D-060); the Logins, Secrets and Integrations tabs are built by MIG-GUI-2. */
export function ServersScreen({
  onOpenCommands,
  onOpenClassic,
  pollMs,
}: {
  onOpenCommands: (group?: string) => void;
  /** Opens the upstream Servers page, which keeps the health filter and the team review. */
  onOpenClassic?: () => void;
  pollMs?: number;
}) {
  const [tab, setTab] = useState<TabId>("servers");
  return (
    <ServersProvider go={setTab} pollMs={pollMs}>
      <Body
        tab={tab}
        setTab={setTab}
        onOpenCommands={onOpenCommands}
        onOpenClassic={onOpenClassic}
      />
    </ServersProvider>
  );
}
