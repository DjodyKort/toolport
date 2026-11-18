import { useState } from "react";
import { ArrowLeft, Search, Undo2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { SkeletonRows } from "@/components/ui/skeleton";
import { toast } from "sonner";
import { toastError } from "@/lib/toast";
import { useLoad } from "../servers/useLoad";
import { errorText } from "../settings/useCtl";
import HookReview from "./HookReview";
import SelectPlan from "./SelectPlan";
import { selectLs, selectPlan, selectSet, selectUndo } from "./selectCommands";
import type { ItemKind, SelectItem, SelectPlanData } from "./selectData";
import {
  addressable,
  changeCount,
  changesOf,
  commandLines,
  GROUP_LIMIT,
  grouped,
  hookTitle,
  isOn,
  itemRef,
  KIND_TITLE,
  KINDS,
  onCount,
  shortRef,
  stage,
  unapplied,
  visibleItems,
  type Filter,
  type Staged,
  type StateView,
} from "./selectModel";
import { Banner, CliFooter, ItemPill, OfferBanner, PolicyBanners } from "./SelectParts";
import { plural } from "./model";

const SELECT_CLASS =
  "h-8 rounded-md border bg-background px-2 text-sm text-foreground focus-visible:ring-3 focus-visible:ring-ring/50 focus-visible:outline-none";

function Row({
  item,
  staged,
  disabled,
  hooksOff,
  onToggle,
}: {
  item: SelectItem;
  staged: Staged;
  disabled: boolean;
  hooksOff: boolean;
  onToggle: (item: SelectItem) => void;
}) {
  const on = isOn(item, staged);
  const reachable = addressable(item);
  const label = item.kind === "hook" ? hookTitle(item) : item.id;
  return (
    <li className="flex flex-wrap items-center gap-x-3 gap-y-1 rounded-lg border bg-card px-3 py-2">
      <input
        type="checkbox"
        aria-label={`${item.kind} ${label}`}
        checked={on}
        disabled={disabled || !reachable}
        onChange={() => onToggle(item)}
      />
      <div className="min-w-0 flex-1 basis-40">
        <p className="text-sm font-medium break-words">{label}</p>
        <p className="text-meta break-words text-muted-foreground">
          {item.kind}
          {item.kind === "hook" && item.command && (
            <>
              {" "}
              · <code className="font-mono text-xs">{item.command}</code>
            </>
          )}
          {item.state === "removed" &&
            " · no longer in the source, kept from the pinned copy"}
          {item.pending && " · command changed upstream, tick it again"}
          {!reachable && " · the name has a comma, which the command line cannot address"}
        </p>
      </div>
      <ItemPill item={item} staged={staged} hooksOff={hooksOff} />
    </li>
  );
}

function UndoDialog({
  source,
  onClose,
  onDone,
}: {
  source: string;
  onClose: () => void;
  onDone: () => void;
}) {
  const [busy, setBusy] = useState(false);
  async function go() {
    setBusy(true);
    try {
      await selectUndo(source);
      toast.success("Restored what Claude Code loads from before the last apply", {
        description: "Your choices are kept; review the plan to apply them again.",
      });
      onDone();
      onClose();
    } catch (error) {
      toastError("Nothing was changed", { description: errorText(error) });
    } finally {
      setBusy(false);
    }
  }
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Undo the last apply</DialogTitle>
          <DialogDescription>
            Puts back the selection, the generated plugin folder and the settings entries
            that the last apply of {source} wrote. One level only.
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button disabled={busy} onClick={() => void go()}>
            Undo
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export default function SelectScreen({
  source,
  onBack,
}: {
  source: string;
  onBack: () => void;
}) {
  const ls = useLoad(() => selectLs(source), source);
  const [staged, setStaged] = useState<Staged>({});
  const [filter, setFilter] = useState<Filter>({ query: "", kind: "all", view: "all" });
  const [review, setReview] = useState<SelectItem | null>(null);
  const [plan, setPlan] = useState<SelectPlanData | null>(null);
  const [switchWhole, setSwitchWhole] = useState(false);
  const [busy, setBusy] = useState(false);
  const [undoing, setUndoing] = useState(false);
  const [open, setOpen] = useState<Set<ItemKind>>(new Set());
  const data = ls.data;

  if (plan) {
    return (
      <SelectPlan
        source={source}
        plan={plan}
        items={data?.items ?? []}
        switchWhole={switchWhole}
        onPlan={setPlan}
        onBack={() => {
          setPlan(null);
          ls.reload();
        }}
        onApplied={() => {
          setPlan(null);
          setSwitchWhole(false);
          ls.reload();
        }}
      />
    );
  }

  if (!data) {
    return (
      <section aria-label="Choose items" className="flex flex-col gap-3">
        <Button variant="ghost" size="sm" className="self-start" onClick={onBack}>
          <ArrowLeft /> Sources
        </Button>
        {ls.error ? (
          <p role="alert" className="text-meta text-destructive">
            {ls.error}
          </p>
        ) : (
          <SkeletonRows rows={4} label="Reading the source" />
        )}
      </section>
    );
  }

  const blocked = data.policy.blocked !== null;
  const pending = changeCount(staged);
  const shown = visibleItems(data.items, staged, filter);
  const groups = grouped(shown);
  const total = data.items.length;
  const on = onCount(data.items, staged);
  const needsPlan = pending > 0 || unapplied(data.items) || switchWhole;
  const everythingOn = data.items.length > 0 && on === total;

  function toggle(item: SelectItem) {
    if (item.kind === "hook" && !isOn(item, staged)) {
      setReview(item);
      return;
    }
    if (item.kind === "hook" && item.pending && !staged[itemRef(item)]) {
      setReview(item);
      return;
    }
    setStaged((current) => stage(current, item, isOn(item, current) ? "off" : "on"));
  }

  function selectNone() {
    setStaged((current) =>
      data
        ? data.items
            .filter((item) => isOn(item, current))
            .reduce((next, item) => stage(next, item, "off"), current)
        : current,
    );
  }

  async function makePlan() {
    setBusy(true);
    try {
      if (pending > 0) await selectSet(source, changesOf(staged));
      setStaged({});
      const next = await selectPlan(source, switchWhole);
      ls.reload();
      setPlan(next);
    } catch (error) {
      toastError("No plan was made", { description: errorText(error) });
    } finally {
      setBusy(false);
    }
  }

  const stagedChanges = changesOf(staged);
  const lines = commandLines(source, stagedChanges, null, switchWhole);

  return (
    <section aria-label="Choose items" className="flex flex-col gap-3">
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="min-w-0">
          <Button variant="ghost" size="sm" className="-ml-2" onClick={onBack}>
            <ArrowLeft /> Sources
          </Button>
          <h2 className="flex flex-wrap items-center gap-2 text-sm font-semibold">
            {data.source}
            {data.ref && <Badge variant="secondary">pinned {shortRef(data.ref)}</Badge>}
          </h2>
          <p className="text-meta text-muted-foreground">
            {on} of {plural(total, "item")} on · nothing is loaded until you apply
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          {data.recorded && (
            <Button variant="ghost" size="sm" onClick={() => setUndoing(true)}>
              <Undo2 /> Undo last apply
            </Button>
          )}
          <Button variant="outline" size="sm" disabled={blocked} onClick={selectNone}>
            Select none
          </Button>
          <Button
            size="sm"
            disabled={blocked || busy || (!needsPlan && !data.recorded)}
            onClick={() => void makePlan()}
          >
            {pending > 0
              ? `Apply ${plural(pending, "change")}`
              : needsPlan || !data.recorded
                ? "Review plan"
                : "Check for updates"}
          </Button>
        </div>
      </div>

      <PolicyBanners policy={data.policy} />
      {data.offer && (
        <OfferBanner offer={data.offer} checked={switchWhole} onChange={setSwitchWhole} />
      )}
      {!data.recorded && on === 0 && (
        <Banner tone="info">
          Nothing is chosen yet, so this source loads nothing. Tick the items you want.
        </Banner>
      )}
      {everythingOn && (
        <Banner tone="warning">
          Everything is on: all {total} items will load. That is allowed, but it is a big
          load.
        </Banner>
      )}
      {data.warnings.map((warning) => (
        <Banner key={warning} tone="warning">
          {warning}
        </Banner>
      ))}

      <div className="flex flex-wrap items-center gap-2">
        <div className="relative min-w-40 flex-1">
          <Search className="pointer-events-none absolute top-2 left-2 size-4 text-muted-foreground" />
          <Input
            aria-label="Search items"
            placeholder="Search items"
            className="pl-8"
            value={filter.query}
            onChange={(event) => setFilter({ ...filter, query: event.target.value })}
          />
        </div>
        <select
          aria-label="Kind"
          className={SELECT_CLASS}
          value={filter.kind}
          onChange={(event) =>
            setFilter({ ...filter, kind: event.target.value as Filter["kind"] })
          }
        >
          <option value="all">All kinds</option>
          {KINDS.map((kind) => (
            <option key={kind} value={kind}>
              {KIND_TITLE[kind]}
            </option>
          ))}
        </select>
        <select
          aria-label="Show"
          className={SELECT_CLASS}
          value={filter.view}
          onChange={(event) =>
            setFilter({ ...filter, view: event.target.value as StateView })
          }
        >
          <option value="all">All</option>
          <option value="on">On</option>
          <option value="new">New ({data.new})</option>
        </select>
      </div>

      {groups.length === 0 && (
        <p className="rounded-xl border bg-card px-4 py-4 text-meta text-muted-foreground">
          {total === 0
            ? "This source offers no skills, agents, commands, hooks or rules."
            : "No item matches."}
        </p>
      )}
      {groups.map(([kind, rows]) => {
        const all = data.items.filter((item) => item.kind === kind);
        const expanded = open.has(kind) || filter.query.trim() !== "";
        const visible = expanded ? rows : rows.slice(0, GROUP_LIMIT);
        return (
          <section
            key={kind}
            aria-label={KIND_TITLE[kind]}
            className="flex flex-col gap-1.5"
          >
            <div className="flex items-center justify-between gap-2">
              <h3 className="text-sm font-semibold">
                {KIND_TITLE[kind]} ({all.length})
              </h3>
              <span className="text-meta text-muted-foreground">
                {all.filter((item) => isOn(item, staged)).length} on
                {kind === "hook" && " · each hook is ticked on its own"}
                {kind === "rule" && " · linked through the Library, not the plugin"}
              </span>
            </div>
            <ul className="flex flex-col gap-1.5">
              {visible.map((item) => (
                <Row
                  key={itemRef(item)}
                  item={item}
                  staged={staged}
                  disabled={blocked}
                  hooksOff={data.policy.hooksOff !== null}
                  onToggle={toggle}
                />
              ))}
            </ul>
            {visible.length < rows.length && (
              <Button
                variant="ghost"
                size="sm"
                className="self-start"
                onClick={() => setOpen(new Set(open).add(kind))}
              >
                Show {rows.length - visible.length} more {KIND_TITLE[kind].toLowerCase()}
              </Button>
            )}
          </section>
        );
      })}

      <CliFooter lines={lines} />

      {review && (
        <HookReview
          hook={{
            id: review.id,
            event: review.event ?? review.id,
            matcher: review.matcher ?? null,
            command: review.pending?.command ?? review.command ?? "",
            was: review.pending ? review.command : null,
          }}
          onCancel={() => setReview(null)}
          onConfirm={() => {
            setStaged((current) => stage(current, review, "tick"));
            setReview(null);
          }}
        />
      )}
      {undoing && (
        <UndoDialog
          source={source}
          onClose={() => setUndoing(false)}
          onDone={() => {
            setStaged({});
            ls.reload();
          }}
        />
      )}
    </section>
  );
}
