import { useCallback, useEffect, useState } from "react";
import { RotateCw } from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { SectionHeader } from "@/components/ui/section-header";
import { SkeletonRows } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { toastError } from "@/lib/toast";
import type {
  AttentionCheckName,
  AttentionData,
  AttentionHiddenRow,
  AttentionSettings,
} from "../bridge/data";
import { ageLabel, type AttentionItem } from "../shell/attentionFeed";
import { toItem } from "../shell/ctlAttentionFeed";
import { openView } from "../shell/navigate";
import {
  ATTENTION_REFRESH_EVENT,
  checkNow,
  listWithHidden,
  readSettings,
  refreshAttention,
  restoreRow,
  sendTestNotification,
  setSwitch,
} from "./attentionCtl";
import { RowMenu } from "./RowMenu";

type Filter = "all" | "needs-you" | "worth-a-look";

const CHECK_INFO: Record<
  AttentionCheckName,
  { title: string; watches: string; runs: string }
> = {
  credentials: {
    title: "Credentials",
    watches:
      "Logins that expired, were revoked or cannot be reached. Missing secrets. Logins that expire within 3 days.",
    runs: "Every probe, 10 min to 6 h",
  },
  updates: {
    title: "Updates",
    watches:
      "New commits of your skill taps, plugin marketplaces, the library and the git checkouts servers run from, and newer releases of the npm, PyPI and Docker packages servers pin. The only check that uses the network.",
    runs: "When you press Check now, or daily with the updates-check task",
  },
  access: {
    title: "Access",
    watches:
      "A server added after your access sets last changed that no set reaches, so no agent sees its tools. Servers that no set reaches, and folder mappings that outrank a client's own set, as quiet rows.",
    runs: "On every read",
  },
  tasks: {
    title: "Tasks",
    watches:
      "A task waiting for you, a task that failed twice in a row, a task that is due but may not start on its own.",
    runs: "On every run",
  },
  library: {
    title: "Library",
    watches:
      "Held changes of the automatic sync, and a library that is behind or has diverged.",
    runs: "On every read",
  },
};

const NOTIFY_NOTE: Partial<Record<AttentionCheckName, string>> = {
  access:
    "A new server nobody reaches can wait, so this never notifies unless you turn it on",
  updates: "Update rows are Worth a look, so they never notify unless you turn this on",
  library: "Library rows are Worth a look, so they never notify unless you turn this on",
};

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function when(text: string | undefined): string {
  const at = text === undefined ? NaN : Date.parse(text);
  return Number.isNaN(at)
    ? ""
    : new Date(at).toLocaleString(undefined, {
        weekday: "short",
        day: "numeric",
        month: "short",
        hour: "2-digit",
        minute: "2-digit",
      });
}

function why(row: AttentionHiddenRow): string {
  const { kind, until } = row.hidden;
  if (kind === "snooze") return `Snoozed until ${when(until)}`;
  if (kind === "skip") return "Skipped: this version only. Returns with the next one.";
  return "On purpose: returns if the state it describes changes.";
}

function OpenRow({ item }: { item: AttentionItem }) {
  return (
    <li className="flex items-start gap-2.5 border-t px-4 py-3 first:border-t-0">
      <span
        aria-hidden="true"
        className={`mt-1.5 size-1.5 shrink-0 rounded-full ${item.urgency === "needs-you" ? "bg-destructive" : "bg-warning"}`}
      />
      <div className="min-w-0 flex-1">
        <p className="text-sm font-medium break-words">{item.title}</p>
        {item.detail && (
          <p className="text-meta break-words text-muted-foreground">{item.detail}</p>
        )}
        <p className="mt-1 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
          <Badge variant="secondary">{item.source}</Badge>
          {ageLabel(item.since)}
          <code className="break-all">{item.id}</code>
        </p>
      </div>
      {item.action && (
        <Button
          size="xs"
          variant={item.urgency === "needs-you" ? "default" : "outline"}
          onClick={() => item.action && openView(item.action.view)}
        >
          {item.action.label}
        </Button>
      )}
      <RowMenu item={item} />
    </li>
  );
}

function HiddenRow({
  row,
  onRestore,
}: {
  row: AttentionHiddenRow;
  onRestore: (id: string) => void;
}) {
  return (
    <li className="flex items-center gap-3 border-t px-4 py-3 first:border-t-0">
      <div className="min-w-0 flex-1">
        <p className="text-sm font-medium break-words">{row.title}</p>
        <p className="text-meta text-muted-foreground">{why(row)}</p>
      </div>
      <Button size="xs" variant="outline" onClick={() => onRestore(row.id)}>
        Restore
      </Button>
    </li>
  );
}

interface View {
  data: AttentionData;
  settings: AttentionSettings;
}

function useAttentionView() {
  const [view, setView] = useState<View | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tick, setTick] = useState(0);
  const load = useCallback(() => setTick((n) => n + 1), []);
  useEffect(() => {
    let alive = true;
    void Promise.all([listWithHidden(), readSettings()]).then(
      ([data, settings]) => {
        if (!alive) return;
        setView({ data, settings });
        setError(null);
      },
      (failure) => alive && setError(errorText(failure)),
    );
    return () => {
      alive = false;
    };
  }, [tick]);
  useEffect(() => {
    window.addEventListener(ATTENTION_REFRESH_EVENT, load);
    return () => window.removeEventListener(ATTENTION_REFRESH_EVENT, load);
  }, [load]);
  return { view, error, load, setView };
}

/** Settings > Toolport+ > Attention: the whole list, what is hidden, which checks run and which
 * rows may notify. Every control is one `toolportctl attention` command. */
export default function AttentionManage() {
  const { view, error, load, setView } = useAttentionView();
  const [filter, setFilter] = useState<Filter>("all");
  const [busy, setBusy] = useState(false);

  async function run(job: () => Promise<void>, failure: string) {
    setBusy(true);
    try {
      await job();
    } catch (cause) {
      toastError(failure, { description: errorText(cause) });
    } finally {
      setBusy(false);
    }
  }

  const check = () =>
    run(async () => {
      const report = await checkNow();
      const failed = report.sources.filter((s) => !s.ok).length;
      const looked = `Looked at ${report.sources.length} ${report.sources.length === 1 ? "source" : "sources"}`;
      if (report.off) toast.info("The updates check is switched off");
      else if (failed > 0) toast.warning(`${looked}; ${failed} could not be reached`);
      else toast.success(looked);
      refreshAttention();
    }, "Could not check for updates");

  const restore = (id: string) =>
    run(async () => {
      await restoreRow(id);
      refreshAttention();
    }, "Could not restore the row");

  const flip = (key: Parameters<typeof setSwitch>[0], on: boolean) =>
    run(async () => {
      const { settings } = await setSwitch(key, on);
      setView((current) => current && { ...current, settings });
      refreshAttention();
    }, "Could not change the setting");

  const sendTest = () =>
    run(async () => {
      await sendTestNotification();
      toast.success("A test notification was sent");
    }, "The test notification did not arrive");

  if (error !== null && view === null) {
    return (
      <div role="alert" className="text-meta text-destructive">
        The attention list could not be read: {error}
        <Button size="xs" variant="outline" className="ml-2" onClick={load}>
          <RotateCw /> Retry
        </Button>
      </div>
    );
  }
  if (view === null) return <SkeletonRows label="Loading attention" />;

  const { data, settings } = view;
  const open = data.items.map(toItem);
  const shown = open.filter((item) => filter === "all" || item.urgency === filter);
  const hidden = data.hidden ?? [];
  const needs = open.filter((item) => item.urgency === "needs-you").length;
  const checkFor = (name: string) => data.checks.find((c) => c.name === name);

  return (
    <div className="flex flex-col gap-6">
      <div className="flex items-start justify-between gap-3">
        <div>
          <p className="text-meta text-muted-foreground">
            What needs you, what you hid, and what Toolport+ checks. Checked{" "}
            {ageLabel(Date.parse(data.checkedAt))}.
          </p>
        </div>
        <Button size="sm" variant="outline" onClick={() => void check()} disabled={busy}>
          <RotateCw className={busy ? "animate-spin" : undefined} /> Check for updates now
        </Button>
      </div>

      <section aria-label="Open now">
        <SectionHeader count={open.length}>Open now</SectionHeader>
        <div role="group" aria-label="Show" className="mb-2 flex gap-1.5">
          {(
            [
              ["all", `All ${open.length}`],
              ["needs-you", `Needs you ${needs}`],
              ["worth-a-look", `Worth a look ${open.length - needs}`],
            ] as const
          ).map(([value, label]) => (
            <Button
              key={value}
              size="xs"
              variant={filter === value ? "secondary" : "ghost"}
              aria-pressed={filter === value}
              onClick={() => setFilter(value)}
            >
              {label}
            </Button>
          ))}
        </div>
        {shown.length === 0 ? (
          <p className="toolport-list px-4 py-6 text-center text-meta text-muted-foreground">
            Nothing needs you.
          </p>
        ) : (
          <ul className="toolport-list">
            {shown.map((item) => (
              <OpenRow key={item.id} item={item} />
            ))}
          </ul>
        )}
      </section>

      <section aria-label="Hidden">
        <SectionHeader count={hidden.length}>Hidden</SectionHeader>
        {hidden.length === 0 ? (
          <p className="text-meta text-muted-foreground">
            Rows you snooze, skip or mark as on purpose wait here.
          </p>
        ) : (
          <>
            <ul className="toolport-list">
              {hidden.map((row) => (
                <HiddenRow key={row.id} row={row} onRestore={(id) => void restore(id)} />
              ))}
            </ul>
            <p className="mt-1.5 text-meta text-muted-foreground">
              Entries for rows that no longer exist are removed after 30 days.
            </p>
          </>
        )}
      </section>

      <section aria-label="Checks">
        <SectionHeader>Checks</SectionHeader>
        <div className="toolport-list overflow-x-auto">
          <table className="w-full text-left text-sm">
            <thead className="text-xs text-muted-foreground">
              <tr>
                <th className="px-4 py-2 font-medium">Check</th>
                <th className="px-2 py-2 font-medium">Last read</th>
                <th className="px-2 py-2 font-medium">Notify me</th>
                <th className="px-2 py-2 font-medium">On</th>
              </tr>
            </thead>
            <tbody>
              {settings.checks.map(({ name, on }) => {
                const info = CHECK_INFO[name];
                const status = checkFor(name);
                const notify = settings.notify.categories.find((c) => c.name === name);
                return (
                  <tr key={name} className="border-t align-top">
                    <th scope="row" className="px-4 py-3 font-normal">
                      <p className="font-medium">{info.title}</p>
                      <p className="text-meta text-muted-foreground">{info.watches}</p>
                      <p className="text-meta text-muted-foreground">Runs: {info.runs}</p>
                      {status && !status.ok && (
                        <p role="status" className="text-meta text-destructive">
                          Not read: {status.error}
                        </p>
                      )}
                    </th>
                    <td className="px-2 py-3 text-meta text-muted-foreground">
                      {status?.off
                        ? "Off"
                        : status?.checkedAt
                          ? when(status.checkedAt)
                          : "Every time the list is read"}
                    </td>
                    <td className="px-2 py-3">
                      <Switch
                        size="sm"
                        aria-label={`Notify me about ${info.title}`}
                        checked={notify?.on ?? false}
                        disabled={busy || !settings.notify.enabled || !on}
                        onCheckedChange={(next) => void flip(`notify.${name}`, next)}
                      />
                      {NOTIFY_NOTE[name] && (
                        <p className="mt-1 max-w-48 text-xs text-muted-foreground">
                          {NOTIFY_NOTE[name]}
                        </p>
                      )}
                    </td>
                    <td className="px-2 py-3">
                      <Switch
                        size="sm"
                        aria-label={`Check ${info.title}`}
                        checked={on}
                        disabled={busy}
                        onCheckedChange={(next) => void flip(`check.${name}`, next)}
                      />
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </section>

      <section aria-label="Notifications">
        <SectionHeader>Notifications</SectionHeader>
        <div className="toolport-list">
          <div className="flex items-center gap-3 px-4 py-3">
            <div className="min-w-0 flex-1">
              <p className="text-sm font-medium">Notify me when something needs me</p>
              <p className="text-meta text-muted-foreground">
                Only Needs-you rows ever notify. One notification per new row; several at
                once arrive as one. Nothing shows while this window has the focus.
              </p>
            </div>
            <Button
              size="xs"
              variant="outline"
              disabled={busy || !settings.notify.enabled}
              onClick={() => void sendTest()}
            >
              Send a test
            </Button>
            <Switch
              aria-label="Notify me when something needs me"
              checked={settings.notify.enabled}
              disabled={busy}
              onCheckedChange={(next) => void flip("notify", next)}
            />
          </div>
        </div>
      </section>
    </div>
  );
}
