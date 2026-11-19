import { useState, type ReactNode } from "react";
import { ExternalLink, TriangleAlert } from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { SkeletonRows } from "@/components/ui/skeleton";
import { StatusCard } from "@/components/ui/status-card";
import { Switch } from "@/components/ui/switch";
import { toastError } from "@/lib/toast";
import { useLoad } from "../servers/useLoad";
import { openView } from "../shell/navigate";
import { errorText } from "../settings/useCtl";
import { setConfig, spendStatus, type ConfigEdit } from "./commands";
import type { StatusData } from "./data";
import { boundedInt, SETTING_BOUNDS, tildePath, weeklySchedule } from "./model";
import { validPort } from "./plan";
import { useSpendDialogs, type SpendDialogs } from "./useSpendDialogs";

const MB = 1024 * 1024;
const ROW = "flex flex-wrap items-center gap-3 px-4 py-3";

function Row({
  title,
  help,
  children,
}: {
  title: string;
  help: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className={ROW}>
      <div className="min-w-0 flex-1 basis-60">
        <p className="text-sm font-semibold">{title}</p>
        <p className="text-meta text-muted-foreground">{help}</p>
      </div>
      <div className="flex flex-wrap items-center gap-2">{children}</div>
    </div>
  );
}

function portOf(status: StatusData): number {
  return Number(status.receiver.address.split(":").pop()) || 4318;
}

interface BodyProps {
  status: StatusData;
  dialogs: SpendDialogs;
  reload: () => void;
}

function SettingsBody({ status, dialogs, reload }: BodyProps) {
  const current = portOf(status);
  const [port, setPort] = useState(String(current));
  const [days, setDays] = useState(String(status.store.retentionDays));
  const [mb, setMb] = useState(String(Math.round(status.store.maxBytes / MB)));
  const [busy, setBusy] = useState(false);

  const newPort = validPort(port);
  const newDays = boundedInt(
    days,
    SETTING_BOUNDS.retentionDays.min,
    SETTING_BOUNDS.retentionDays.max,
  );
  const newMb = boundedInt(mb, SETTING_BOUNDS.maxMb.min, SETTING_BOUNDS.maxMb.max);
  const limitsChanged =
    newDays !== status.store.retentionDays ||
    newMb !== Math.round(status.store.maxBytes / MB);

  async function save(edit: ConfigEdit, done: string) {
    setBusy(true);
    try {
      await setConfig(edit);
      toast.success(done);
      reload();
    } catch (error) {
      toastError("Could not save the setting", { description: errorText(error) });
    } finally {
      setBusy(false);
    }
  }

  function savePort() {
    if (newPort === null) return;
    if (status.enabled && status.profilesSetUp > 0) dialogs.usePort(newPort, current);
    else void save({ port: newPort }, `Receiver port set to ${newPort}`);
  }

  return (
    <div className="toolport-list" data-slot="spend-settings">
      <Row
        title="Spend collecting"
        help="Starts a receiver in the Toolport app and writes five settings for Claude Code after you approve a plan."
      >
        <Badge variant={status.enabled ? "success" : "off"}>
          {status.enabled ? "On" : "Off"}
        </Badge>
        {status.enabled ? (
          <Button variant="outline" size="sm" onClick={dialogs.undo}>
            Undo setup…
          </Button>
        ) : (
          <Button size="sm" onClick={dialogs.setup}>
            Set up…
          </Button>
        )}
      </Row>
      <Row
        title="Receiver port"
        help={`127.0.0.1 only. Change it if another program uses ${current}.`}
      >
        <Input
          aria-label="Receiver port"
          inputMode="numeric"
          className="w-24"
          value={port}
          aria-invalid={newPort === null}
          onChange={(event) => setPort(event.target.value)}
        />
        <Button
          variant="outline"
          size="sm"
          disabled={busy || newPort === null || newPort === current}
          onClick={savePort}
        >
          Save
        </Button>
      </Row>
      <Row
        title="Keep events for"
        help={`Days or megabytes, whichever comes first (${SETTING_BOUNDS.retentionDays.min} to ${SETTING_BOUNDS.retentionDays.max} days, ${SETTING_BOUNDS.maxMb.min} to ${SETTING_BOUNDS.maxMb.max} MB). Daily totals stay after events are pruned unless you switch them off.`}
      >
        <Input
          aria-label="Days to keep events"
          inputMode="numeric"
          className="w-16"
          value={days}
          aria-invalid={newDays === null}
          onChange={(event) => setDays(event.target.value)}
        />
        <span className="text-meta">days or</span>
        <Input
          aria-label="Size cap in MB"
          inputMode="numeric"
          className="w-16"
          value={mb}
          aria-invalid={newMb === null}
          onChange={(event) => setMb(event.target.value)}
        />
        <span className="text-meta">MB</span>
        <Button
          variant="outline"
          size="sm"
          disabled={busy || newDays === null || newMb === null || !limitsChanged}
          onClick={() =>
            void save(
              { retentionDays: newDays ?? undefined, maxMb: newMb ?? undefined },
              "Retention saved",
            )
          }
        >
          Save
        </Button>
      </Row>
      <Row title="Show cost" help="Turn off on a subscription plan to show tokens only.">
        <Switch
          aria-label="Show cost"
          checked={status.showCost}
          disabled={busy}
          onCheckedChange={(on) =>
            void save({ showCost: on }, on ? "Cost is shown" : "Cost is hidden")
          }
        />
      </Row>
      <Row
        title="Daily totals"
        help={`Keep one line per day, project and model after events are pruned, so the charts keep their history. ${
          status.store.dailyLines > 0
            ? `${status.store.dailyLines} ${status.store.dailyLines === 1 ? "line" : "lines"} kept.`
            : "Nothing kept yet."
        }`}
      >
        <Switch
          aria-label="Keep daily totals"
          checked={status.store.dailyTotals}
          disabled={busy}
          onCheckedChange={(on) =>
            void save(
              { dailyTotals: on },
              on ? "Daily totals are kept" : "Daily totals are no longer kept",
            )
          }
        />
        <Button
          variant="outline"
          size="sm"
          disabled={busy || status.store.dailyLines === 0}
          onClick={dialogs.deleteTotals}
        >
          Delete totals…
        </Button>
      </Row>
      <Row
        title="Weekly measure"
        help={
          status.weeklyMeasure.enabled
            ? `Runs context measure for ${tildePath(status.weeklyMeasure.folder ?? "one folder")}: ${weeklySchedule(status.weeklyMeasure.cron).toLowerCase()}, one small request to the ${status.weeklyMeasure.model} model.`
            : "Runs context measure once a week as a task (haiku, one folder, one small model request). Turning it on asks for approval."
        }
      >
        <Switch
          aria-label="Weekly measure"
          checked={status.weeklyMeasure.enabled}
          disabled={busy}
          onCheckedChange={(on) =>
            on
              ? dialogs.weeklyMeasure()
              : void save({ weeklyMeasure: false }, "The weekly measure is off")
          }
        />
      </Row>
      <Row title="Spend page" help="Cost, tokens, project, model and task.">
        <Button variant="outline" size="sm" onClick={() => openView("spend")}>
          <ExternalLink data-icon="inline-start" />
          Open Spend
        </Button>
      </Row>
    </div>
  );
}

/** Settings > Toolport+ > Spend: the numbers `spend config set` takes, and the way to set
 * collecting up or undo it. */
export default function SpendSettings() {
  const status = useLoad(spendStatus, "status");
  const dialogs = useSpendDialogs(status.reload);
  if (status.error && !status.data)
    return (
      <StatusCard
        role="alert"
        variant="danger"
        icon={<TriangleAlert />}
        title="Spend settings could not be read"
        detail={status.error}
        actions={
          <Button variant="outline" size="sm" onClick={status.reload}>
            Retry
          </Button>
        }
      />
    );
  if (!status.data) return <SkeletonRows label="Reading the Spend settings" />;
  const { data } = status;
  const key = [
    data.receiver.address,
    data.store.retentionDays,
    data.store.maxBytes,
    data.showCost,
    data.enabled,
    data.store.dailyTotals,
    data.store.dailyLines,
    data.weeklyMeasure.enabled,
    data.weeklyMeasure.folder,
  ].join("|");
  return (
    <>
      <SettingsBody key={key} status={data} dialogs={dialogs} reload={status.reload} />
      {dialogs.element}
    </>
  );
}
