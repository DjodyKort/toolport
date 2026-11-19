import { BellOff, BellRing, CircleHelp } from "lucide-react";
import { Button } from "@/components/ui/button";
import { StatusCard } from "@/components/ui/status-card";
import type { NotifyPermission as Permission } from "../bridge/data";

const WHERE = "System Settings > Notifications > Toolport+";

/** What the system lets Toolport+ do, in the three states of the spec plus the one it cannot
 * tell: macOS gives the desktop plugin no way to read the permission. */
export function NotifyPermission({
  permission,
  busy,
  onOpenSettings,
}: {
  permission: Permission;
  busy: boolean;
  onOpenSettings: () => void;
}) {
  const open = (
    <Button size="xs" variant="outline" disabled={busy} onClick={onOpenSettings}>
      Open System Settings
    </Button>
  );
  if (permission === "blocked") {
    return (
      <StatusCard
        role="alert"
        variant="danger"
        icon={<BellOff />}
        title="macOS blocks notifications from Toolport+"
        detail={`You said no earlier. Change it in ${WHERE}. Until then the bell count is the only signal when the window is closed.`}
        actions={open}
      />
    );
  }
  if (permission === "not-asked") {
    return (
      <StatusCard
        role="status"
        variant="info"
        icon={<BellRing />}
        title="macOS has not been asked yet"
        detail="Sending a test asks once. Until you answer, the bell and the in-app toast still work."
      />
    );
  }
  if (permission === "allowed") {
    return (
      <StatusCard
        role="status"
        variant="success"
        icon={<BellRing />}
        title="macOS allows notifications from Toolport+"
        detail="Banners and sounds follow your Focus settings; nothing is sent while a Focus mode silences Toolport+."
      />
    );
  }
  return (
    <StatusCard
      role="status"
      variant="warning"
      icon={<CircleHelp />}
      title="Toolport+ cannot see whether macOS allows notifications"
      detail={`macOS does not tell the app. Send a test: if nothing appears, allow notifications in ${WHERE}. A blocked app gets no system notification, and the bell count is then the only signal when the window is closed.`}
      actions={open}
    />
  );
}
