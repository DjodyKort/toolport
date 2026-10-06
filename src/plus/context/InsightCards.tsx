import { useEffect, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { ContextFoldersData, ContextLoadsData } from "../types/context";
import type { LoadsData } from "../bridge/data";
import { useRead, type WriteControl } from "./hooks";
import { folderLabel, formatTokens, plural, type ProfileRow } from "./model";
import { Code, Field, QuerySection, SELECT_CLASS } from "./parts";

const LAYER_LABEL: Record<string, string> = {
  policy: "Managed policy",
  org: "Org file",
  personal: "Personal layer",
  "client-layer": "Client layers",
  plugin: "Plugins",
  project: "This project",
  user: "Your user files",
  loose: "Loose files",
};

export interface LayerCost {
  source: string;
  label: string;
  tokens: number;
  files: number;
}

/** Tokens of what loads at the start of a session, per source, biggest first. The rows that
 * load only on demand are counted apart. */
export function costPerLayer(data: LoadsData): { layers: LayerCost[]; onDemand: number } {
  const by = new Map<string, LayerCost>();
  let onDemand = 0;
  for (const item of data.items) {
    if (!item.loaded) {
      if (item.lazy) onDemand += item.tokens;
      continue;
    }
    const row = by.get(item.source) ?? {
      source: item.source,
      label: LAYER_LABEL[item.source] ?? item.source,
      tokens: 0,
      files: 0,
    };
    row.tokens += item.tokens;
    row.files += 1;
    by.set(item.source, row);
  }
  return { layers: [...by.values()].sort((a, b) => b.tokens - a.tokens), onDemand };
}

function Bar({ value, max, label }: { value: number; max: number; label: string }) {
  const pct = max > 0 ? Math.max(2, Math.round((value / max) * 100)) : 0;
  return (
    <div
      role="meter"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={max}
      aria-valuenow={value}
      className="h-1.5 overflow-hidden rounded-full bg-muted"
    >
      <div className="h-full rounded-full bg-primary" style={{ width: `${pct}%` }} />
    </div>
  );
}

export function LoadsSection({
  version,
  profiles,
}: {
  version: number;
  profiles: ProfileRow[] | undefined;
}) {
  const [profile, setProfile] = useState("");
  const [cwd, setCwd] = useState("");
  const [applied, setApplied] = useState("");
  const argv = [
    "context",
    "loads",
    ...(profile ? ["--profile", profile] : []),
    ...(applied ? ["--cwd", applied] : []),
  ];
  const query = useRead<ContextLoadsData>(argv);
  const { reload } = query;
  useEffect(() => {
    if (version > 0) reload();
  }, [version, reload]);
  return (
    <QuerySection
      title="What loads"
      hint="What a Claude session loads at the start, and what each layer costs. Numbers are estimates."
      query={query}
    >
      {(data) => {
        const { layers, onDemand } = costPerLayer(data);
        const max = Math.max(0, ...layers.map((l) => l.tokens));
        const budget = data.skill_budget;
        return (
          <div className="flex flex-col gap-3">
            <form
              className="flex flex-wrap items-end gap-3"
              onSubmit={(event) => {
                event.preventDefault();
                setApplied(cwd.trim());
              }}
            >
              <Field label="Launch profile">
                {(id) => (
                  <select
                    id={id}
                    className={SELECT_CLASS}
                    value={profile}
                    onChange={(e) => setProfile(e.target.value)}
                  >
                    <option value="">Default</option>
                    {(profiles ?? []).map((p) => (
                      <option key={p.name} value={p.name}>
                        {p.name}
                      </option>
                    ))}
                  </select>
                )}
              </Field>
              <Field label="Folder">
                {(id) => (
                  <Input
                    id={id}
                    value={cwd}
                    placeholder={data.cwd}
                    onChange={(e) => setCwd(e.target.value)}
                    autoComplete="off"
                  />
                )}
              </Field>
              <Button type="submit" size="sm" variant="outline">
                Show
              </Button>
            </form>
            <p className="text-sm">
              <b className="tabular-nums">{formatTokens(data.total_tokens)}</b> tokens at
              the start in <Code>{data.cwd}</Code>
              {data.profile ? <> with the profile {data.profile}</> : null};{" "}
              {formatTokens(onDemand)} more load on demand.
            </p>
            <ul aria-label="Tokens per layer" className="flex flex-col gap-2">
              {layers.map((layer) => (
                <li
                  key={layer.source}
                  className="grid grid-cols-[minmax(8rem,12rem)_minmax(0,1fr)_5rem] items-center gap-3 text-sm"
                >
                  <span>
                    {layer.label}{" "}
                    <span className="text-xs text-muted-foreground">
                      ({plural(layer.files, "file")})
                    </span>
                  </span>
                  <Bar value={layer.tokens} max={max} label={`${layer.label} tokens`} />
                  <span className="text-right tabular-nums">
                    {formatTokens(layer.tokens)}
                  </span>
                </li>
              ))}
            </ul>
            <p className="text-xs text-muted-foreground">
              Skill list: {formatTokens(budget.used_tokens)} of{" "}
              {formatTokens(budget.limit_tokens)} tokens
              {budget.capped.length > 0
                ? `; ${plural(budget.capped.length, "skill")} lose their description`
                : ""}
              .
            </p>
            {data.clobbers.length > 0 && (
              <ul
                aria-label="Overrides"
                className="list-disc pl-4 text-xs text-muted-foreground"
              >
                {data.clobbers.map((c) => (
                  <li key={`${c.kind}:${c.key}`}>
                    {c.key}: {c.winner} {c.relation} {c.overridden.join(", ")}
                  </li>
                ))}
              </ul>
            )}
            {data.notes.map((note) => (
              <p key={note} className="text-xs text-muted-foreground">
                {note}
              </p>
            ))}
          </div>
        );
      }}
    </QuerySection>
  );
}

export function FoldersSection({
  write,
  version,
}: {
  write: WriteControl;
  version: number;
}) {
  const query = useRead<ContextFoldersData>(["context", "folders"]);
  const { reload } = query;
  useEffect(() => {
    if (version > 0) reload();
  }, [version, reload]);
  return (
    <QuerySection
      title="Folder routing"
      hint="Picks a profile by folder. Off, every client keeps its own profile."
      query={query}
      actions={
        query.data && (
          <Button
            size="sm"
            variant="outline"
            disabled={write.busy}
            onClick={() => {
              const enable = !query.data!.enabled;
              write.begin({
                command: "context folders",
                title: enable ? "Turn folder routing on" : "Turn folder routing off",
                argv: ["context", "folders", enable ? "--enable" : "--disable"],
                confirmLabel: enable ? "Turn on" : "Turn off",
                phrase: "folders",
                planned: {
                  summary: enable
                    ? "The gateway starts picking a profile by folder."
                    : "The gateway ignores folder mappings again.",
                  steps: [
                    {
                      op: "update",
                      detail: enable
                        ? "Folder routing: off to on"
                        : "Folder routing: on to off",
                    },
                  ],
                  effects: {},
                  warnings: [],
                  undo: `toolportctl context folders ${enable ? "--disable" : "--enable"}`,
                },
              });
            }}
          >
            {query.data.enabled ? "Turn off…" : "Turn on…"}
          </Button>
        )
      }
      isEmpty={(data) => data.folders.length === 0 && data.mappings.length === 0}
      empty={
        <p className="text-sm text-muted-foreground">
          No folders are mapped to a profile.
        </p>
      }
    >
      {(data) => (
        <div className="flex flex-col gap-2">
          <p className="text-sm">
            <Badge variant={data.enabled ? "success" : "secondary"}>
              {data.enabled ? "On" : "Off"}
            </Badge>{" "}
            <span className="text-muted-foreground">
              {plural(data.mappings.length, "mapping")}
            </span>
          </p>
          <ul
            aria-label="Profile per folder"
            className="flex flex-col divide-y rounded-lg border"
          >
            {data.folders.map((folder) => (
              <li
                key={folder.root}
                className="flex flex-wrap items-baseline gap-x-3 gap-y-0.5 px-3 py-2 text-sm"
              >
                <span className="font-mono text-xs break-all" title={folder.root}>
                  {folderLabel(folder.root)}
                </span>
                <span>
                  {folder.profile ??
                    (folder.wouldApply
                      ? `${String(folder.wouldApply)} (inactive)`
                      : "default")}
                </span>
                <span className="ml-auto text-xs tabular-nums text-muted-foreground">
                  ~{formatTokens(folder.tokens)} tokens
                </span>
                <span className="basis-full text-xs text-muted-foreground">
                  {folder.reason}
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}
    </QuerySection>
  );
}
