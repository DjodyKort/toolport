import { lazy, Suspense, useState } from "react";
import { RefreshCw } from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { StatusCard } from "@/components/ui/status-card";
import { toastError } from "@/lib/toast";
import { errorText } from "../settings/useCtl";
import { useLoad } from "../servers/useLoad";
import { syncContext } from "./commands";
import { plural, shortPath } from "./model";
import { Group, RowShell } from "./StackRows";

const LaunchProfiles = lazy(() => import("./LaunchProfiles"));

/** The one pipeline of this tab is `context sync`. This build runs its bundle part (apply the
 * bound bundles when auto-apply is on); the layers deploy and shims are not
 * part of it yet and the tab says so instead of showing health it cannot know. */
export default function RulesLaunch({
  home,
  folder,
}: {
  home: string | null;
  folder: string | null;
}) {
  const preview = useLoad(() => syncContext(true), "sync");
  const [busy, setBusy] = useState(false);

  async function syncNow() {
    setBusy(true);
    try {
      const data = await syncContext(false);
      const applied = data.bundles?.filter((b) => b.applied).length ?? 0;
      toast.success(
        applied ? `Applied ${plural(applied, "bundle")}` : "Nothing to apply",
      );
      preview.reload();
    } catch (error) {
      toastError("Sync did not finish", { description: errorText(error) });
    } finally {
      setBusy(false);
    }
  }

  if (preview.error && !preview.data) {
    return (
      <StatusCard
        role="alert"
        variant="danger"
        icon={<RefreshCw />}
        title="The sync plan could not be read"
        detail={preview.error}
        actions={
          <Button size="sm" variant="outline" onClick={preview.reload}>
            Try again
          </Button>
        }
      />
    );
  }
  const data = preview.data;
  const bundles = data?.bundles ?? [];
  return (
    <div className="flex flex-col gap-5">
      <StatusCard
        variant="info"
        icon={<RefreshCw />}
        title="Launch wiring and rule layers are not part of this build yet"
        detail={
          data?.deploy.note
            ? `${data.deploy.note[0].toUpperCase()}${data.deploy.note.slice(1)}. This tab shows the part that works: syncing bound bundles.`
            : "This tab shows the part that works: syncing bound bundles."
        }
      />
      <Group
        title="Sync"
        hint="context sync"
        action={
          <Button size="sm" disabled={busy || !data} onClick={() => void syncNow()}>
            Sync now
          </Button>
        }
      >
        <RowShell
          title="Apply bound bundles"
          badge={
            data && (
              <Badge variant={data.autoApply ? "success" : "off"}>
                {data.autoApply ? "auto-apply on" : "auto-apply off"}
              </Badge>
            )
          }
          subtitle={
            data?.autoApply
              ? bundles.length === 0
                ? "Every bound folder that matches already has a bundle."
                : `${plural(bundles.length, "folder")} would get a bundle:`
              : "Turn on Apply automatically in bound folders on Folder bundles to let sync apply them."
          }
        />
        {bundles.map((b) => (
          <RowShell
            key={`${b.bundle}-${b.folder}`}
            title={`${b.bundle} in ${shortPath(b.folder, home)}`}
            subtitle={b.error ?? "would be applied"}
          />
        ))}
      </Group>
      <Suspense fallback={<p className="text-meta">Loading the launch profiles…</p>}>
        <LaunchProfiles home={home} folder={folder} />
      </Suspense>
      <Group title="Not built yet" hint="lands with the next part of the Context slice">
        <RowShell
          title="Rule layers"
          subtitle="Deploying client and org layers to the machine."
        />
        <RowShell
          title="Shell shims and launch wiring"
          subtitle="The source order of the shell and the presync stamp."
        />
        <RowShell
          title="The checkpoint gauge"
          subtitle="Auto-compact window and the checkpoint hook."
        />
      </Group>
    </div>
  );
}
