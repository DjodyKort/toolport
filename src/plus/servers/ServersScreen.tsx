import { useState } from "react";
import { History, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Tabs } from "../ui";
import { IntegrationsTab, LOGIN_TABS, LoginsTab, SecretsTab } from "../logins";
import { ClientsTab } from "./ClientsTab";
import { GatewayStrip } from "./GatewayStrip";
import { HealthTab } from "./HealthTab";
import { ProfilesTab } from "./ProfilesTab";
import { ServersTab } from "./ServersTab";
import { ServersProvider } from "./ServersProvider";
import { useRestoreFocus } from "./useRestoreFocus";
import { useServers, type TabId } from "./useServers";

function Body({
  tab,
  setTab,
  onOpenCommands,
  onOpenClassic,
  pollMs,
}: {
  tab: TabId;
  setTab: (tab: TabId) => void;
  onOpenCommands: (group?: string) => void;
  onOpenClassic?: () => void;
  pollMs?: number;
}) {
  const { views, profiles, clients } = useServers();
  const [doctorRun, setDoctorRun] = useState(0);
  useRestoreFocus();
  const login = LOGIN_TABS.some((entry) => entry.id === tab);
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
          ...LOGIN_TABS.map(({ id, label }) => ({ id, label })),
          { id: "health", label: "Health" },
        ]}
      >
        <div className="flex flex-col gap-4">
          {!login && <GatewayStrip />}
          {tab === "servers" && <ServersTab />}
          {tab === "profiles" && <ProfilesTab />}
          {tab === "clients" && <ClientsTab />}
          {tab === "health" && <HealthTab key={doctorRun} />}
          {tab === "logins" && (
            <LoginsTab onOpenCommands={onOpenCommands} pollMs={pollMs} />
          )}
          {tab === "secrets" && (
            <SecretsTab onOpenCommands={onOpenCommands} pollMs={pollMs} />
          )}
          {tab === "integrations" && (
            <IntegrationsTab onOpenCommands={onOpenCommands} pollMs={pollMs} />
          )}
        </div>
      </Tabs>
    </div>
  );
}

/** The Servers screen of the control center: every server with its live state, the profiles,
 * what each client sees, and the health of the whole. Every read and write goes through
 * `toolportctl` (D-060); the Logins, Secrets and Integrations tabs are the panels of
 * `src/plus/logins`. */
export function ServersScreen({
  initialTab,
  onOpenCommands,
  onOpenClassic,
  pollMs,
}: {
  initialTab?: TabId;
  onOpenCommands: (group?: string) => void;
  /** Opens the upstream Servers page, which keeps the health filter and the team review. */
  onOpenClassic?: () => void;
  pollMs?: number;
}) {
  const [tab, setTab] = useState<TabId>(initialTab ?? "servers");
  return (
    <ServersProvider go={setTab} pollMs={pollMs}>
      <Body
        tab={tab}
        setTab={setTab}
        onOpenCommands={onOpenCommands}
        onOpenClassic={onOpenClassic}
        pollMs={pollMs}
      />
    </ServersProvider>
  );
}
