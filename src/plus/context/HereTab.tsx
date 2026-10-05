import { useEffect, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import type { LoadItem, LoadsData, MeasureData } from "../bridge/data";
import type {
  ContextBundleLsData,
  ContextBundleStatusData,
} from "../types/context-bundle";
import { Stat } from "../logins/atoms";
import { ComposedView } from "./ComposedView";
import { FolderField, suggestions, useRecentFolders } from "./folder";
import { useRead, useWrite, type WriteControl } from "./hooks";
import { WriteDialogs } from "./WriteDialogs";
import { formatTokens, plural } from "./model";
import { Code, QuerySection, Section, useRows } from "./parts";
import {
  basisWord,
  biggestPlugin,
  measuredView,
  skillBudgetState,
  stackGroups,
  tokenLabel,
  withoutSpec,
} from "./stackModel";
import type { PlanV1 } from "../ui";

const SHOWN = 4;

function measurePlan(cwd: string, without?: string): PlanV1 {
  return {
    summary: `Measure what Claude Code loads in ${cwd}`,
    steps: [
      {
        op: "exec",
        detail: without
          ? `Two minimal requests with claude -p in the folder: as it is, and without ${without}`
          : "One minimal request with claude -p in the folder",
      },
      {
        op: "note",
        detail:
          "Reads the real lists of skills, agents, commands and plugins, and the input tokens of the request",
      },
      {
        op: "note",
        detail:
          "Nothing is written to the folder. The result is kept for this folder and this Claude Code version",
      },
    ],
    effects: {},
    warnings: ["Uses a small part of your plan: every request spends model tokens."],
    undo: "",
  };
}

function MeasureResult({ data }: { data: MeasureData }) {
  const view = measuredView(data);
  return (
    <section aria-label="Measured" className="flex flex-col gap-2 text-sm">
      <p>
        {view.asIs && (
          <>
            <b className="tabular-nums">{formatTokens(view.asIs.total)}</b> tokens in the
            first request, <b>measured</b> by Claude Code {data.claudeCodeVersion} (
            {data.model}){data.cached ? ", from the saved measurement" : ""}.
          </>
        )}
      </p>
      {view.deltas.length > 0 && (
        <ul aria-label="Measured savings" className="list-disc pl-4">
          {view.deltas.map((delta) => (
            <li key={delta.label}>
              {delta.label}: {delta.tokens > 0 ? "+" : "−"}
              {formatTokens(Math.abs(delta.tokens))} tokens ({delta.percent}%), measured
            </li>
          ))}
        </ul>
      )}
      {view.invisible.length > 0 && (
        <p className="text-xs text-muted-foreground">
          {plural(view.invisible.length, "skill")} Claude Code does not list:{" "}
          {view.invisible.map((skill) => skill.name).join(", ")}.
        </p>
      )}
    </section>
  );
}

function LoadRow({
  item,
  max,
  disabled,
  onWithout,
}: {
  item: LoadItem;
  max: number;
  disabled: boolean;
  onWithout: (item: LoadItem) => void;
}) {
  const pct = max > 0 ? Math.max(2, Math.round((item.tokens / max) * 100)) : 0;
  const notes = [item.reason, item.via.length > 0 ? `via ${item.via.join(" > ")}` : ""]
    .filter(Boolean)
    .join(" · ");
  return (
    <li className="grid grid-cols-[minmax(0,1fr)_auto_7rem_auto] items-center gap-3 px-3 py-2 text-sm">
      <div className="min-w-0">
        <b className="break-words">{item.name}</b>
        <p className="text-xs break-words text-muted-foreground">{notes}</p>
      </div>
      <span className="flex flex-wrap justify-end gap-1.5">
        <Badge variant="secondary">{item.origin.name}</Badge>
        {item.lazy && <Badge variant="outline">on demand</Badge>}
        {!item.loaded && !item.lazy && <Badge variant="outline">not loaded</Badge>}
        {item.origin.kind === "inert" && <Badge variant="destructive">never loads</Badge>}
        {item.kind === "skill" && item.visible === false && (
          <Badge variant="warning">not in the skill list</Badge>
        )}
      </span>
      <div className="text-right" aria-label={tokenLabel(item.tokens, item.basis)}>
        <span className="block tabular-nums">{formatTokens(item.tokens)}</span>
        <small
          className={
            item.basis === "estimate" ? "text-muted-foreground" : "text-emerald-600"
          }
        >
          {basisWord(item.basis)}
        </small>
        <div className="mt-0.5 h-1 overflow-hidden rounded-full bg-muted">
          <div className="h-full rounded-full bg-primary" style={{ width: `${pct}%` }} />
        </div>
      </div>
      {item.kind === "plugin" && item.loaded ? (
        <Button
          size="xs"
          variant="outline"
          disabled={disabled}
          onClick={() => onWithout(item)}
        >
          Measure without it…
        </Button>
      ) : (
        <span />
      )}
    </li>
  );
}

function Stack({
  data,
  write,
  cwd,
}: {
  data: LoadsData;
  write: WriteControl;
  cwd: string;
}) {
  const [open, setOpen] = useState<string | null>(null);
  const groups = stackGroups(data);
  const max = Math.max(0, ...data.items.map((item) => item.tokens));
  const budget = skillBudgetState(data);
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-1.5" role="group" aria-label="Skill list budget">
        <p className="text-sm">
          Skill list: <b className="tabular-nums">{formatTokens(budget.used)}</b> of{" "}
          {formatTokens(budget.limit)} tokens, {basisWord("estimate")}
          {budget.over
            ? `; ${plural(budget.capped.length, "skill")} lose their description`
            : ""}
          .
        </p>
        <div
          role="meter"
          aria-label="Skill list budget"
          aria-valuemin={0}
          aria-valuemax={budget.limit}
          aria-valuenow={budget.used}
          className="h-2 overflow-hidden rounded-full bg-muted"
        >
          <div
            className={`h-full rounded-full ${budget.over ? "bg-amber-500" : "bg-primary"}`}
            style={{
              width: `${budget.limit > 0 ? Math.min(100, (budget.used / budget.limit) * 100) : 0}%`,
            }}
          />
        </div>
        {budget.over && (
          <p className="text-xs text-muted-foreground">
            Claude Code caps the skill list. Which skills lose their description is a
            guess until the folder is measured.
          </p>
        )}
      </div>
      {groups.map(({ group, items, tokens }) => {
        const big = items.length > 5;
        const all = open === group;
        const shown = big && !all ? items.slice(0, SHOWN) : items;
        return (
          <section key={group} aria-label={group} className="flex flex-col gap-1.5">
            <div className="flex items-center gap-2">
              <h4 className="text-sm font-semibold">{group}</h4>
              <span className="text-xs tabular-nums text-muted-foreground">
                {formatTokens(tokens)}
              </span>
              {big && (
                <Button
                  size="xs"
                  variant="ghost"
                  className="ml-auto"
                  onClick={() => setOpen(all ? null : group)}
                >
                  {all ? "Show fewer" : `Show all ${items.length}`}
                </Button>
              )}
            </div>
            <ul
              aria-label={`${group} rows`}
              className="flex flex-col divide-y rounded-lg border"
            >
              {shown.map((item) => (
                <LoadRow
                  key={`${item.kind}:${item.name}:${item.path ?? ""}`}
                  item={item}
                  max={max}
                  disabled={write.busy}
                  onWithout={(plugin) =>
                    write.begin({
                      command: "context measure",
                      title: `Measure without ${plugin.name}`,
                      argv: [
                        "context",
                        "measure",
                        "--cwd",
                        cwd,
                        "--without",
                        withoutSpec(plugin),
                        "--yes",
                      ],
                      confirmLabel: "Measure",
                      phrase: "measure",
                      planned: measurePlan(cwd, withoutSpec(plugin)),
                      warnNoPreview: false,
                      renderResult: (result) => (
                        <MeasureResult data={result as MeasureData} />
                      ),
                    })
                  }
                />
              ))}
            </ul>
          </section>
        );
      })}
    </div>
  );
}

function Summary({ data }: { data: LoadsData }) {
  const measured = data.measured;
  const info = data.measured_info;
  const plugin = biggestPlugin(data);
  const lazy = data.items.filter((item) => item.lazy).length;
  return (
    <div
      role="group"
      aria-label="What loads, in numbers"
      className="grid gap-3 [grid-template-columns:repeat(auto-fit,minmax(200px,1fr))]"
    >
      <Stat
        label="Measured"
        tone={measured && !info?.stale ? "ok" : undefined}
        value={
          measured ? (
            `${formatTokens(measured.total)} tokens`
          ) : (
            <span className="text-muted-foreground">not measured yet</span>
          )
        }
        note={
          measured && info
            ? `first request · Claude Code ${info.claudeCodeVersion} · ${info.model} · ${info.measuredAt}${info.stale ? " · stale" : ""}`
            : "Measure for real asks Claude Code itself"
        }
      />
      <Stat
        label="Your files, estimated"
        value={`≈ ${formatTokens(data.total_tokens)} tokens`}
        note="bytes ÷ 4; skills are overstated"
      />
      <Stat
        label="On demand"
        value={`${formatTokens(data.tokens_lazy)} tokens`}
        note={`${plural(lazy, "file")} load when Claude touches them`}
      />
      <Stat
        label="Biggest to switch off"
        value={plugin ? plugin.name : "none"}
        note={
          plugin
            ? `${formatTokens(plugin.tokens)} tokens, ${basisWord(plugin.basis)}`
            : undefined
        }
      />
    </div>
  );
}

function AppliedHere({
  cwd,
  version,
  onOpenProfiles,
}: {
  cwd: string;
  version: number;
  onOpenProfiles: () => void;
}) {
  const query = useRead<ContextBundleStatusData>([
    "context",
    "bundle",
    "status",
    "--cwd",
    cwd,
  ]);
  const { reload } = query;
  useEffect(() => {
    if (version > 0) reload();
  }, [version, reload]);
  const applied = query.data?.applied;
  return (
    <div className="flex flex-wrap items-center gap-2 text-sm">
      <span className="text-muted-foreground">Profile applied here:</span>
      {query.status === "loading" ? (
        <span className="text-muted-foreground">checking…</span>
      ) : query.status === "error" ? (
        <span className="text-destructive">could not be read</span>
      ) : applied ? (
        <>
          <b>{applied.bundle}</b>
          {applied.drift && <Badge variant="warning">changed since the apply</Badge>}
        </>
      ) : (
        <b>none</b>
      )}
      <Button size="sm" onClick={onOpenProfiles}>
        Apply a profile…
      </Button>
    </div>
  );
}

/** The Context tab "This folder": the stack of what loads in a folder with the source and the
 * cost of every row, the real measurement behind a confirm, the composed text and the profile
 * applied here. */
export function HereTab({ openTab }: { openTab?: (id: string) => void }) {
  const rows = useRows();
  const { recent, remember } = useRecentFolders();
  const [draft, setDraft] = useState("");
  const [folder, setFolder] = useState("");
  const loads = useRead<LoadsData>([
    "context",
    "loads",
    ...(folder ? ["--cwd", folder] : []),
    "--measured",
  ]);
  const bundles = useRead<ContextBundleLsData>(["context", "bundle", "ls"]);
  const [version, setVersion] = useState(0);
  const { reload } = loads;
  const write = useWrite(rows, () => {
    reload();
    setVersion((n) => n + 1);
  });
  const cwd = loads.data?.cwd ?? null;
  const known = suggestions(
    recent,
    bundles.data?.bundles.flatMap((bundle) => bundle.appliedTo.map((to) => to.folder)),
  );
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-end gap-3">
        <FolderField
          value={draft}
          onChange={setDraft}
          options={known}
          placeholder={cwd ?? "Folder where Claude starts"}
          submitLabel="Show"
          onSubmit={() => {
            setFolder(draft.trim());
            remember(draft);
          }}
        />
        <Badge
          variant="info"
          title="Other clients receive the synced skills and rules, not this stack"
        >
          Claude Code only
        </Badge>
        <Button
          disabled={!cwd || write.busy}
          onClick={() =>
            cwd &&
            write.begin({
              command: "context measure",
              title: "Measure what Claude really loads here",
              argv: ["context", "measure", "--cwd", cwd, "--yes"],
              confirmLabel: "Measure",
              phrase: "measure",
              planned: measurePlan(cwd),
              warnNoPreview: false,
              renderResult: (data) => <MeasureResult data={data as MeasureData} />,
            })
          }
        >
          Measure for real…
        </Button>
      </div>
      {cwd && (
        <AppliedHere
          cwd={cwd}
          version={version}
          onOpenProfiles={() => {
            remember(cwd);
            openTab?.("profiles");
          }}
        />
      )}
      <Section
        title="What loads"
        hint="One stack per folder, in the order Claude Code builds it, with the source of every row and what it costs."
        actions={
          cwd && (
            <span className="text-xs text-muted-foreground">
              <Code>{cwd}</Code>
            </span>
          )
        }
      >
        <QuerySection
          title="Stack"
          query={loads}
          isEmpty={(data) => data.items.length === 0}
          empty={
            <p className="text-sm text-muted-foreground">Nothing loads in this folder.</p>
          }
        >
          {(data) => (
            <div className="flex flex-col gap-4">
              <Summary data={data} />
              <Callout variant="info">
                Skills are cheap, plugins are not: Claude Code caps the skill list, so a
                size estimate overstates skills. Token numbers are estimates unless marked
                measured; a saving is only shown once it was measured.
              </Callout>
              {data.partial && (
                <Callout variant="warning" role="status">
                  Some sources could not be read, so the list may be incomplete.
                </Callout>
              )}
              <Stack data={data} write={write} cwd={data.cwd} />
              {data.notes.map((note) => (
                <p key={note} className="text-xs text-muted-foreground">
                  {note}
                </p>
              ))}
            </div>
          )}
        </QuerySection>
      </Section>
      <ComposedView cwd={cwd} version={version} />
      <WriteDialogs write={write} />
    </div>
  );
}
