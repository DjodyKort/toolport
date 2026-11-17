import { useEffect, useState } from "react";
import { Gauge, TriangleAlert } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { SkeletonRows } from "@/components/ui/skeleton";
import { StatusCard } from "@/components/ui/status-card";
import { toastError } from "@/lib/toast";
import { Group, RowShell } from "../context/StackRows";
import { useLoad } from "../servers/useLoad";
import { errorText } from "../settings/useCtl";
import {
  applyHooks,
  compressionDoctor,
  compressionStatus,
  hooksStatus,
  pauseHooks,
  resumeHooks,
  undoHooks,
} from "./commands";
import type { DoctorCheck, HookProviderId, HooksWriteData } from "./data";
import { AdvancedPanel, HeadlineCard, HookCard, ProviderTile } from "./HookCards";
import MeasurementsPage from "./MeasurementsPage";
import { headline, hookRows, providerTiles, untilText } from "./model";
import { changeHooks } from "./useHookWrites";

interface Verification {
  at: string;
  checks: DoctorCheck[];
  states: string[];
}

function Verified({ result }: { result: Verification }) {
  return (
    <div className="flex flex-col gap-1 text-meta" data-testid="verification">
      <p className="font-medium">Checked at {result.at}</p>
      <ul className="flex flex-col gap-0.5">
        {result.states.map((line) => (
          <li key={line}>{line}</li>
        ))}
        {result.checks.map((check) => (
          <li key={check.name} className="flex flex-wrap items-center gap-1.5">
            <Badge variant={check.ok ? "success" : "destructive"}>
              {check.ok ? "ok" : "failed"}
            </Badge>
            <span>
              {check.name}: {check.detail}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}

function Overview({ onMeasurements }: { onMeasurements: () => void }) {
  const hooks = useLoad(hooksStatus, "hooks");
  const policy = useLoad(compressionStatus, "policy");
  const [busy, setBusy] = useState(false);
  const [preview, setPreview] = useState<HooksWriteData | null>(null);
  const [dismissed, setDismissed] = useState(false);
  const [verification, setVerification] = useState<Verification | null>(null);
  const until = hooks.data?.pausedUntil ?? null;
  const reload = hooks.reload;

  useEffect(() => {
    if (!until) return;
    const wait = Math.min(new Date(until).getTime() - Date.now() + 1000, 2 ** 31 - 1);
    const timer = setTimeout(reload, Math.max(wait, 0));
    return () => clearTimeout(timer);
  }, [until, reload]);

  if (!hooks.data) {
    return hooks.error ? (
      <StatusCard
        role="alert"
        variant="danger"
        icon={<TriangleAlert />}
        title="The hook state could not be read"
        detail={hooks.error}
        actions={
          <Button size="sm" variant="outline" onClick={hooks.reload}>
            Try again
          </Button>
        }
      />
    ) : (
      <SkeletonRows label="Loading the compression hooks…" />
    );
  }

  const status = hooks.data;
  const rows = hookRows(status);
  const head = headline(rows, status.paused);
  const tiles = providerTiles(status, policy.data);

  async function write<R>(
    done: string | ((result: R) => string),
    run: () => Promise<R>,
    undo: (() => Promise<unknown>) | null,
  ) {
    setBusy(true);
    setPreview(null);
    try {
      await changeHooks(done, run, undo, hooks.reload);
    } finally {
      setBusy(false);
    }
  }

  const undoEach = async (ids: HookProviderId[]) => {
    for (const id of ids) await undoHooks(id);
  };
  const adopt = (ids: HookProviderId[]) =>
    write(
      `Adopted ${ids.join(" and ")}`,
      () => applyHooks(),
      () => undoEach(ids),
    );

  async function showPreview() {
    try {
      setPreview(await applyHooks(undefined, true));
    } catch (error) {
      toastError("Could not preview the adoption", { description: errorText(error) });
    }
  }

  async function verify() {
    setBusy(true);
    try {
      const [check, fresh] = await Promise.all([compressionDoctor(), hooksStatus()]);
      setVerification({
        at: new Date().toLocaleTimeString("en-GB", {
          hour: "2-digit",
          minute: "2-digit",
        }),
        checks: check.checks,
        states: hookRows(fresh).map(
          (row) =>
            `${row.provider}: ${
              row.health === "ok"
                ? "the entry reads as Toolport wrote it"
                : row.health === "absent"
                  ? "no entry found"
                  : `the entry is ${row.health}`
            }`,
        ),
      });
      hooks.reload();
    } catch (error) {
      toastError("Could not verify the hooks", { description: errorText(error) });
    } finally {
      setBusy(false);
    }
  }

  const owned = rows.filter((r) => ["ok", "edited", "missing"].includes(r.health));
  const foreign = rows.filter((r) => r.health === "foreign");
  const card = (row: (typeof rows)[number]) => (
    <HookCard
      key={row.provider}
      row={row}
      paused={status.paused}
      busy={busy}
      onAdopt={() =>
        void write(
          `Adopted ${row.provider}`,
          () => applyHooks(row.provider),
          () => undoHooks(row.provider),
        )
      }
      onUndo={() =>
        void write(
          `Undid the adoption of ${row.provider}`,
          () => undoHooks(row.provider),
          () => applyHooks(row.provider),
        )
      }
      onRepair={() =>
        void write(`Restored ${row.provider}`, () => applyHooks(row.provider), null)
      }
    />
  );

  return (
    <div className="flex flex-col gap-5">
      {!(head.kind === "adopt" && dismissed) && (
        <HeadlineCard
          headline={head}
          rows={rows}
          pausedSince={status.pausedSince}
          pausedUntil={status.pausedUntil}
          busy={busy}
          preview={preview}
          checks={verification && <Verified result={verification} />}
          onAdopt={() => void adopt(head.foreign)}
          onPreview={() => void showPreview()}
          onNotNow={() => setDismissed(true)}
          onRepair={() =>
            void write("Restored the compression hooks", () => applyHooks(), null)
          }
          onPause={() =>
            void write(
              (result) =>
                result.pausedUntil
                  ? `Compression paused until ${untilText(result.pausedUntil)}`
                  : "Compression paused",
              () => pauseHooks("1d"),
              resumeHooks,
            )
          }
          onResume={() => void write("Compression resumed", resumeHooks, pauseHooks)}
          onVerify={() => void verify()}
        />
      )}

      <section
        aria-label="Is it paying off?"
        className="flex flex-wrap items-center justify-between gap-3 rounded-xl border bg-card p-4"
      >
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          <h3 className="text-sm font-semibold">Is it paying off?</h3>
          <p className="text-meta text-muted-foreground">
            Where tokens go, what the hooks did, the billed outcome per epoch and a rework
            check. Every figure shows its population and what it cannot tell you.
          </p>
        </div>
        <Button size="sm" variant="outline" onClick={onMeasurements}>
          <Gauge data-icon="inline-start" />
          See measurements
        </Button>
      </section>

      {owned.length > 0 && (
        <Group title="Written by Toolport" count={owned.length}>
          {owned.map(card)}
        </Group>
      )}
      {foreign.length > 0 && (
        <Group
          title="Found in settings.json"
          count={foreign.length}
          hint="not written by Toolport"
        >
          {foreign.map(card)}
        </Group>
      )}

      <Group title="Providers" count={tiles.length}>
        {tiles.map((tile) => (
          <ProviderTile key={tile.id} tile={tile} />
        ))}
      </Group>

      <Group title="Everything that runs around a tool call">
        <RowShell
          title="Inventory not available in this build"
          badge={<Badge variant="off">unavailable</Badge>}
          subtitle="The list of every hook that runs around a tool call, with who owns each one, needs the plugin reader, which arrives with the plugins slice. The two compression hooks above are read straight from settings.json."
        />
      </Group>

      <AdvancedPanel status={policy.data} />

      <p className="text-meta text-muted-foreground">{status.cannotTell}</p>
    </div>
  );
}

/** The Hooks & compression tab of Context: the rtk and read-once entries Toolport owns, the
 * providers and, one level down, the four honest measurements. */
export default function HooksCompression() {
  const [page, setPage] = useState<"overview" | "measurements">("overview");
  return page === "measurements" ? (
    <MeasurementsPage onBack={() => setPage("overview")} />
  ) : (
    <Overview onMeasurements={() => setPage("measurements")} />
  );
}
