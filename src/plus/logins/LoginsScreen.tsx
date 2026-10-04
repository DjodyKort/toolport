import { useState } from "react";
import { Tabs } from "../ui";
import { IntegrationsTab } from "./IntegrationsTab";
import { LoginsTab, type TabProps } from "./LoginsTab";
import { LOGIN_TABS, type LoginTabId } from "./model";
import { SecretsTab } from "./SecretsTab";

/** The three tabs of "Logins & secrets" in one screen. The Servers screen mounts the same
 * three panels as tabs of its own; this screen is where a notification or a link from
 * Settings lands. */
export function LoginsScreen({
  initialTab = "logins",
  ...tab
}: TabProps & { initialTab?: LoginTabId }) {
  const [current, setCurrent] = useState<LoginTabId>(initialTab);
  return (
    <Tabs
      items={[...LOGIN_TABS]}
      value={current}
      onValueChange={(id) => setCurrent(id as LoginTabId)}
      label="Logins and secrets sections"
    >
      {current === "logins" && <LoginsTab {...tab} />}
      {current === "secrets" && <SecretsTab {...tab} />}
      {current === "integrations" && <IntegrationsTab {...tab} />}
    </Tabs>
  );
}
