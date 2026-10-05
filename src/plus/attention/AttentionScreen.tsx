import { useState } from "react";
import { BellRing, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { EmptyState } from "@/components/ui/empty-state";
import { useRestoreFocus } from "../agents/useRestoreFocus";
import { refreshAttentionCount } from "../attention";
import { Offline, Stat } from "../logins/atoms";
import type { PlusView } from "../nav";
import { policyOf } from "../system/model";
import { rowsOf, useRegistry } from "../system/hooks";
import { WriteDialogs } from "../system/WriteDialogs";
import type { AttentionItem } from "../types/attention";
import { ErrorState, ScreenSkeleton, errorText } from "../ui";
import { DismissDialog } from "./DismissDialog";
import { useAttentionList, useAttentionWrite } from "./hooks";
import {
  actionArgs,
  commandOf,
  dismissArgv,
  groupItems,
  LEVELS,
  untilDate,
  viewOfRoute,
  type DismissChoice,
} from "./model";
import { Row } from "./Row";

const clock = (at: number) =>
  new Date(at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

/** The Attention screen: everything that wants a decision in one list, most urgent first. A
 * row says what is wrong, opens the screen where it is fixed and may offer one command, which
 * runs through the same preview and confirmation as everywhere else. The list is read from
 * local state only (`attention ls`), so it is fast and works offline. */
export function AttentionScreen({
  onNavigate,
  onOpenCommands,
}: {
  onNavigate: (view: PlusView, params: Record<string, string>) => void;
  onOpenCommands?: (group?: string) => void;
}) {
  useRestoreFocus();
  const list = useAttentionList();
  const registry = useRegistry();
  const rows = rowsOf(registry);
  const [now, setNow] = useState(() => Date.now());
  const { reload } = list;
  const refresh = () => {
    setNow(Date.now());
    reload();
  };
  const write = useAttentionWrite(rows, refresh);
  const [hiding, setHiding] = useState<AttentionItem | null>(null);

  const data = list.data;
  if (!data) {
    if (list.status !== "error") return <ScreenSkeleton label="Loading attention" />;
    return errorText(list.error).code === "bridge" ? (
      <Offline error={list.error} onRetry={list.reload} onOpenCommands={onOpenCommands} />
    ) : (
      <ErrorState
        error={list.error}
        title="Couldn't read what needs you"
        context="attention ls"
        onRetry={list.reload}
      />
    );
  }

  const groups = groupItems(data.items);
  const { counts } = data;

  const terminalArgs = (item: AttentionItem) => {
    const args = item.action ? actionArgs(item.action) : null;
    const row = args && commandOf(rows, args);
    return args && row && policyOf(rows, row.id)?.terminal ? args : null;
  };

  const open = (item: AttentionItem) => {
    const view = viewOfRoute(item.target.route);
    if (view) onNavigate(view, item.target.params);
  };

  const run = (item: AttentionItem) => {
    const args = item.action ? actionArgs(item.action) : null;
    if (!item.action || !args) {
      write.refuse(
        "This action does not run toolportctl, so Toolport does not run it from here.",
      );
      return;
    }
    write.begin({
      command: commandOf(rows, args)?.id ?? args.slice(0, 2).join(" "),
      title: `${item.action.label}: ${item.title}`,
      argv: args,
      confirmLabel: item.action.label,
    });
  };

  const hide = (item: AttentionItem, choice: DismissChoice) => {
    setHiding(null);
    write.begin({
      command: "attention dismiss",
      title: `Hide ${item.title}`,
      argv: dismissArgv(item.id, untilDate(choice)),
      confirmLabel: "Hide",
    });
  };

  const checkAgain = () => {
    refresh();
    refreshAttentionCount();
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="flex justify-end">
        <Button variant="outline" onClick={checkAgain}>
          <RefreshCw /> Check again
        </Button>
      </div>
      {list.status === "error" && (
        <Callout
          variant="warning"
          role="status"
          className="flex flex-wrap items-center gap-2"
        >
          <span>
            Could not refresh, showing the last answer: {errorText(list.error).message}
          </span>
          <Button size="sm" variant="outline" onClick={list.reload}>
            Retry
          </Button>
        </Callout>
      )}
      <WriteDialogs write={write} />
      <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
        <Stat
          label="Needs you"
          tone={counts.needsYou > 0 ? "warn" : "ok"}
          value={counts.needsYou}
        />
        <Stat label="Worth a look" value={counts.look} />
        <Stat label="For your information" value={counts.fyi} />
        <Stat label="Last check" value={clock(now)} />
      </div>
      {data.items.length === 0 ? (
        <EmptyState
          icon={<BellRing />}
          title="Nothing needs you"
          description="Every check Toolport runs is quiet. Rows appear here when a login, a secret, a task or a source wants a decision."
        />
      ) : (
        LEVELS.filter(({ level }) => groups[level].length > 0).map(
          ({ level, label, tone }) => (
            <section key={level} aria-label={label} className="flex flex-col gap-2">
              <h3 className="flex items-center gap-2 text-sm font-medium">
                {label}
                <span className="text-xs text-muted-foreground tabular-nums">
                  {groups[level].length}
                </span>
              </h3>
              <ul aria-label={`${label} rows`} className="flex flex-col gap-2">
                {groups[level].map((item) => (
                  <Row
                    key={item.id}
                    item={item}
                    tone={tone}
                    now={now}
                    busy={write.busy}
                    terminal={terminalArgs(item)}
                    onOpen={open}
                    onAction={run}
                    onDismiss={setHiding}
                  />
                ))}
              </ul>
            </section>
          ),
        )
      )}
      <p className="text-xs text-muted-foreground">
        Every row opens the screen where it is fixed. Nothing is changed from here without
        the usual preview and confirmation.
      </p>
      {hiding && (
        <DismissDialog
          item={hiding}
          onChoose={(choice) => hide(hiding, choice)}
          onClose={() => setHiding(null)}
        />
      )}
    </div>
  );
}
