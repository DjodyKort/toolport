import { useMemo, useState } from "react";
import { BookOpen, Plus, RefreshCw, Trash2, TriangleAlert } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import type {
  SkillsLintData,
  SkillsLsData,
  SourcesLsData,
  SkillsSyncData,
} from "../bridge/data";
import type {
  SkillsAuditData,
  SkillsResolveData,
  SkillsStatusData,
} from "../types/skills";
import { ErrorState, errorText, ScreenSkeleton, Tabs } from "../ui";
import { BundlesPanel } from "./bundles";
import { InitDialog, NewDialog, SyncDialog } from "./dialogs";
import { ScopeBar } from "./fields";
import { InstallPanel } from "./install";
import { TapsPanel } from "./taps";
import { useRead, useRegistryRows, useWrite, type WriteControl } from "./hooks";
import {
  byClient,
  clientName,
  EDITOR_REASON,
  isLibrary,
  plural,
  rowKey,
  scopeArgs,
  SKILL_CLIENTS,
  TOOLS_WITHOUT_CLI,
  USER_SCOPE,
  type AuditFinding,
  type Collision,
  type LintMessage,
  type Rejected,
  type SkillRow,
  type StatusOutput,
  type WriteScope,
} from "./model";
import {
  Card,
  LintSummary,
  Messages,
  PathLine,
  Section,
  SourceBadge,
  Stat,
  Verdict,
} from "./parts";
import { useSkillRows } from "./useSkillRows";
import { WriteDialogs } from "./WriteDialogs";

interface Diff {
  new: string[];
  modified: unknown[];
  removed: unknown[];
  unchanged: number;
  clean: boolean;
  noLockfile: boolean;
}

function ClientStates({
  row,
  status,
}: {
  row: SkillRow;
  status: SkillsStatusData | null;
}) {
  const entry = status?.entries.find((e) => e.name === row.name && e.type === row.type);
  if (!status || !status.lockfilePresent || !entry?.knownToLockfile)
    return <span className="text-muted-foreground">Not synced to any client yet</span>;
  const outputs = status.outputs as StatusOutput[];
  const clients = [...status.targetedClients].sort(byClient);
  return (
    <ul aria-label={`Sync state of ${row.name}`} className="flex flex-col gap-1">
      {clients.map((client) => {
        const synced = (entry.clientsSynced as string[]).includes(client);
        const present = outputs.find(
          (o) => o.name === row.name && o.client === client,
        )?.present;
        return (
          <li key={client} className="flex flex-wrap items-center gap-2">
            <span className="w-28">{clientName(client)}</span>
            {!synced ? (
              <Badge variant="outline">Not written</Badge>
            ) : present === false ? (
              <Badge variant="destructive">Missing</Badge>
            ) : entry.drifted ? (
              <Badge variant="warning">Changed since sync</Badge>
            ) : (
              <Badge variant="success">In sync</Badge>
            )}
          </li>
        );
      })}
    </ul>
  );
}

function SkillLint({ name }: { name: string }) {
  const lint = useRead<SkillsLintData>(["skills", "lint", "--name", name]);
  if (lint.status === "loading")
    return <span className="text-muted-foreground">Checking…</span>;
  if (!lint.data)
    return (
      <ErrorState
        error={lint.error}
        title="Couldn't lint this skill"
        onRetry={lint.reload}
      />
    );
  return <LintSummary data={lint.data} noun={name} />;
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <>
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="min-w-0">{children}</dd>
    </>
  );
}

function SkillDetail({
  row,
  status,
  audit,
  write,
  writeScope,
}: {
  row: SkillRow;
  status: SkillsStatusData | null;
  audit: SkillsAuditData | null;
  write: WriteControl;
  writeScope: WriteScope;
}) {
  const own = isLibrary(row);
  const findings = ((audit?.findings ?? []) as AuditFinding[]).filter(
    (f) => f.skill === row.name,
  );
  const rejected = ((status?.rejected ?? []) as Rejected[]).filter(
    (r) => r.name === row.name,
  );
  return (
    <section
      aria-label={`Skill ${row.name}`}
      className="flex flex-col gap-4 rounded-lg border bg-card p-4"
    >
      <h4 className="flex flex-wrap items-center gap-2 text-base font-semibold">
        {row.name}
        <Badge variant="outline">{row.type}</Badge>
        <SourceBadge origin={row.origin} />
        <Badge variant={row.writable ? "success" : "secondary"}>
          {row.writable ? "Toolport can edit" : "read-only"}
        </Badge>
      </h4>
      {row.description && (
        <p className="text-sm text-muted-foreground">{row.description}</p>
      )}
      <dl className="grid grid-cols-[8rem_minmax(0,1fr)] gap-x-3 gap-y-2 text-sm">
        <Row label="Source">
          {row.origin.name}
          {!own && (
            <span className="text-muted-foreground">
              {" "}
              · Toolport shows it and never edits it
            </span>
          )}
        </Row>
        <Row label="Found at">
          <PathLine path={row.path} />
        </Row>
        <Row label="Activation">
          {row.activation}
          {row.activation === "manual" && " (you start it with a slash command)"}
        </Row>
        <Row label="Visible to Claude">
          {row.visible ? (
            <Badge variant="success">yes</Badge>
          ) : (
            <span className="flex flex-col gap-1">
              <Badge variant="destructive">no: slash command only</Badge>
              <span className="text-xs text-muted-foreground">{row.invisibleReason}</span>
            </span>
          )}
        </Row>
        {own && (
          <>
            <Row label="Installed in">
              <ClientStates row={row} status={status} />
            </Row>
            <Row label="Lint">
              <SkillLint name={row.name} />
            </Row>
            <Row label="Audit">
              {findings.length === 0 ? (
                <Badge variant="success">no findings</Badge>
              ) : (
                <ul className="flex flex-col gap-1">
                  {findings.map((f, i) => (
                    <li key={i} className="flex flex-wrap items-baseline gap-2">
                      <Badge variant={f.severity === "high" ? "destructive" : "warning"}>
                        {f.severity}
                      </Badge>
                      <span>
                        {f.message}
                        {f.line != null && ` (line ${f.line})`}
                      </span>
                    </li>
                  ))}
                </ul>
              )}
            </Row>
            {rejected.length > 0 && (
              <Row label="Claude Code">
                {rejected.map((r) => (
                  <p key={r.client} className="text-xs text-destructive">
                    Rejects the {clientName(r.client)} copy: {r.reason}
                  </p>
                ))}
              </Row>
            )}
          </>
        )}
      </dl>
      <div className="flex flex-wrap gap-2">
        <Button
          size="sm"
          variant="outline"
          disabled
          title={EDITOR_REASON}
          aria-label={`Open editor for ${row.name}`}
        >
          Open editor
        </Button>
        {own && (
          <Button
            size="sm"
            variant="outline"
            aria-label={`Uninstall ${row.name}`}
            disabled={write.busy}
            onClick={() =>
              write.begin({
                command: "skills uninstall",
                title: `Uninstall skill ${row.name}`,
                argv: ["skills", "uninstall", row.name, ...scopeArgs(writeScope)],
                confirmLabel: "Uninstall",
                phrase: row.name,
              })
            }
          >
            <Trash2 /> Uninstall…
          </Button>
        )}
      </div>
      {!own && (
        <p className="text-xs text-muted-foreground">
          {EDITOR_REASON.split(" (")[0]}: read-only sources are not edited here.
        </p>
      )}
    </section>
  );
}

function Collisions({
  query,
  write,
  writeScope,
}: {
  query: ReturnType<typeof useRead<SkillsResolveData>>;
  write: WriteControl;
  writeScope: WriteScope;
}) {
  const collisions = (query.data?.collisions ?? []) as Collision[];
  if (query.status === "error")
    return (
      <ErrorState
        error={query.error}
        title="Couldn't look for collisions"
        onRetry={query.reload}
      />
    );
  return (
    <section
      aria-label="Collisions"
      className="flex flex-col gap-2 rounded-lg border bg-card p-4"
    >
      <h4 className="text-sm font-medium">{plural(collisions.length, "collision")}</h4>
      {collisions.length === 0 ? (
        <p className="text-sm text-muted-foreground">
          No command file shadows a synced skill.
        </p>
      ) : (
        <>
          <ul className="flex flex-col gap-1.5 text-sm">
            {collisions.map((c) => (
              <li key={`${c.skill}-${c.client}`} className="flex items-start gap-1.5">
                <TriangleAlert
                  className="mt-0.5 size-3.5 shrink-0 text-warning"
                  aria-hidden="true"
                />
                <span>
                  <b>{c.skill}</b> exists as a skill and as a command file for{" "}
                  {clientName(c.client)}:{" "}
                  <code className="font-mono text-xs">{c.collisionPath}</code>. Claude
                  Code prefers the skill; the command stays as a duplicate until you
                  resolve it.
                </span>
              </li>
            ))}
          </ul>
          <div>
            <Button
              size="sm"
              variant="outline"
              disabled={write.busy}
              onClick={() =>
                write.begin({
                  command: "skills resolve",
                  title: "Resolve command collisions",
                  argv: ["skills", "resolve", "--migrate", ...scopeArgs(writeScope)],
                  confirmLabel: "Resolve",
                  phrase: "resolve",
                })
              }
            >
              Resolve…
            </Button>
          </div>
        </>
      )}
    </section>
  );
}

function DiffBody({ data }: { data: Diff }) {
  const marks = [
    ["new", "+", "text-success"],
    ["modified", "~", "text-warning"],
    ["removed", "−", "text-destructive"],
  ] as const;
  return (
    <div className="flex flex-col gap-2">
      <Verdict ok={data.clean && !data.noLockfile}>
        {data.noLockfile
          ? "Never synced: every skill is new"
          : data.clean
            ? "No changes since the last sync"
            : "Changed since the last sync"}
      </Verdict>
      <ul className="flex flex-col gap-1 text-sm">
        {marks.flatMap(([key, glyph, tone]) =>
          (data[key] as unknown[]).map(String).map((name) => (
            <li key={`${key}-${name}`} className="flex gap-2">
              <span className={`w-4 text-center font-mono ${tone}`} aria-hidden="true">
                {glyph}
              </span>
              <code className="font-mono text-xs">{name}</code>
              <span className="text-muted-foreground">({key})</span>
            </li>
          )),
        )}
      </ul>
      {data.unchanged > 0 && (
        <p className="text-xs text-muted-foreground">{data.unchanged} unchanged</p>
      )}
    </div>
  );
}

function DriftBody({ data }: { data: SkillsStatusData }) {
  const missing = (data.outputs as StatusOutput[]).filter((o) => !o.present);
  const rejected = data.rejected as Rejected[];
  if (!data.lockfilePresent)
    return <Verdict ok={false}>Not synced yet. Sync writes the outputs.</Verdict>;
  return (
    <div className="flex flex-col gap-2">
      <Verdict ok={!data.drift && rejected.length === 0}>
        {data.drift
          ? `${plural(missing.length, "output")} missing or changed since the last sync`
          : `All ${plural(data.lockedCount, "synced skill")} still in place`}
      </Verdict>
      <ul className="flex flex-col gap-1 text-sm">
        {missing.map((o, i) => (
          <li key={i}>
            <code className="font-mono text-xs">{o.name}</code> is missing for{" "}
            {clientName(o.client)}
          </li>
        ))}
        {rejected.map((r, i) => (
          <li key={`r${i}`}>
            <code className="font-mono text-xs">{r.name}</code> is rejected by{" "}
            {clientName(r.client)}: {r.reason}
          </li>
        ))}
      </ul>
    </div>
  );
}

interface Scope {
  filter: string;
  setFilter: (id: string) => void;
  selected: string | null;
  setSelected: (key: string) => void;
  writeScope: WriteScope;
  setWriteScope: (scope: WriteScope) => void;
}

function Body({ write, scope }: { write: WriteControl; scope: Scope }) {
  const { filter, setFilter, selected, setSelected, writeScope, setWriteScope } = scope;
  const [dialog, setDialog] = useState<"sync" | "new" | "init" | null>(null);
  const lib = useRead<SkillsLsData>(["skills", "ls"]);
  const sources = useRead<SourcesLsData>(["sources", "ls"]);
  const status = useRead<SkillsStatusData>(["skills", "status"]);
  const lint = useRead<SkillsLintData>(["skills", "lint"]);
  const audit = useRead<SkillsAuditData>(["skills", "audit"]);
  const diff = useRead<Diff>(["skills", "diff"]);
  const plan = useRead<SkillsSyncData>(["skills", "sync", "--dry-run"]);
  const collisions = useRead<SkillsResolveData>(["skills", "resolve", "--dry-run"]);

  const others = useMemo(
    () =>
      (sources.data?.sources ?? []).filter(
        (s) => s.id !== "library" && s.counts.skill > 0,
      ),
    [sources.data],
  );
  const list = useSkillRows(
    filter,
    useMemo(() => others.map((s) => s.id), [others]),
    lib,
  );
  const libRows = lib.data?.skills ?? [];
  const libSkills = libRows.filter((r) => r.type === "skill");
  const invisible = libSkills.filter((r) => !r.visible).length;
  const otherCount = others.reduce((n, s) => n + s.counts.skill, 0);
  const chips = [
    { id: "all", label: "All", count: libRows.length + otherCount },
    { id: "library", label: "Library", count: libRows.length },
    ...others.map((s) => ({ id: s.id, label: s.origin.name, count: s.counts.skill })),
  ];
  const row = list.rows.find((r) => rowKey(r) === selected) ?? list.rows[0];
  const looks = [
    invisible > 0 && plural(invisible, "invisible skill"),
    (collisions.data?.collisions.length ?? 0) > 0 &&
      plural(collisions.data?.collisions.length ?? 0, "collision"),
    status.data?.drift && "drift since the last sync",
    ((lint.data?.messages ?? []) as LintMessage[]).some((m) => m.level !== "info") &&
      "lint warnings",
    (audit.data?.findings.length ?? 0) > 0 &&
      plural(audit.data?.findings.length ?? 0, "audit finding"),
  ].filter((x): x is string => !!x);

  return (
    <>
      {dialog === "sync" && plan.data && (
        <SyncDialog
          known={[...SKILL_CLIENTS, ...(status.data?.targetedClients ?? [])]}
          initial={plan.data.targetedClients}
          source={plan.data.clientSource}
          onClose={() => setDialog(null)}
          onSubmit={(clients) => {
            setDialog(null);
            write.begin({
              command: "skills sync",
              title: `Sync skills to ${plural(clients.length, "client")}`,
              argv: [
                "skills",
                "sync",
                ...scopeArgs(writeScope),
                ...clients.flatMap((c) => ["--client", c]),
              ],
              confirmLabel: "Sync",
              phrase: "sync",
            });
          }}
        />
      )}
      {dialog === "new" && (
        <NewDialog
          onClose={() => setDialog(null)}
          onSubmit={(name, type, progressive) => {
            setDialog(null);
            write.begin({
              command: "skills add",
              title: `Create ${type} ${name}`,
              argv: [
                "skills",
                "add",
                name,
                "--type",
                type,
                ...(progressive ? ["--with-progressive"] : []),
              ],
              confirmLabel: "Create",
              phrase: name,
            });
          }}
        />
      )}
      {dialog === "init" && (
        <InitDialog
          onClose={() => setDialog(null)}
          onSubmit={(path, name) => {
            setDialog(null);
            write.begin({
              command: "skills init",
              title: "Create the skills repository",
              argv: [
                "skills",
                "init",
                ...(path ? ["--path", path] : []),
                ...(name ? ["--name", name] : []),
              ],
              confirmLabel: "Create",
              phrase: "create",
            });
          }}
        />
      )}
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="max-w-prose text-sm text-muted-foreground">
          Everything Claude Code can load on this Mac, and where each item comes from.
          Only your own library is written to; the other sources are shown and never
          edited.
        </p>
        <div className="flex flex-wrap gap-2">
          <Button
            variant="outline"
            disabled={write.busy}
            onClick={() => setDialog("new")}
          >
            <Plus /> New skill…
          </Button>
          <Button disabled={write.busy || !plan.data} onClick={() => setDialog("sync")}>
            <RefreshCw /> Sync…
          </Button>
          <Button
            variant="outline"
            disabled={write.busy}
            onClick={() =>
              write.begin({
                command: "skills clean",
                title: "Remove synced skill files",
                argv: ["skills", "clean", ...scopeArgs(writeScope)],
                confirmLabel: "Remove",
                phrase: "clean skills",
              })
            }
          >
            <Trash2 /> Clean outputs…
          </Button>
        </div>
      </div>
      <ScopeBar scope={writeScope} onChange={setWriteScope} />
      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
        <Stat
          label="Library"
          value={`${libRows.length} items`}
          note={`${plural(libSkills.length, "skill")}, ${plural(libRows.length - libSkills.length, "rule")}`}
        />
        <Stat
          label="Other sources"
          value={`${otherCount} skills`}
          note={plural(others.length, "source")}
        />
        <Stat
          label="Visible to Claude"
          warn={invisible > 0}
          value={`${libSkills.length - invisible} of ${libSkills.length}`}
          note={
            invisible > 0
              ? `${plural(invisible, "library skill")} slash-only: the synced frontmatter is rejected`
              : "every library skill"
          }
        />
        <Stat
          label="Needs a look"
          warn={looks.length > 0}
          value={looks.length}
          note={looks.join(" · ") || "nothing"}
        />
      </div>
      <div role="group" aria-label="Source" className="flex flex-wrap gap-2">
        {chips.map((chip) => (
          <Button
            key={chip.id}
            size="sm"
            variant={filter === chip.id ? "default" : "outline"}
            aria-pressed={filter === chip.id}
            onClick={() => setFilter(chip.id)}
          >
            {chip.label} <b>{chip.count}</b>
          </Button>
        ))}
      </div>
      {plan.data && (
        <p className="text-xs text-muted-foreground">
          A sync goes to {plan.data.targetedClients.map(clientName).join(", ")} (
          {plan.data.clientSource === "lock"
            ? "the clients of the last sync"
            : "no sync has chosen clients yet"}
          ).
        </p>
      )}
      {sources.status === "error" && (
        <p role="status" className="text-sm text-muted-foreground">
          The other sources could not be listed, so only your library shows.
        </p>
      )}
      {list.skipped.map((s) => (
        <p key={s.source} role="status" className="text-xs text-warning">
          {s.source} is incomplete: {s.reason} ({s.detector}).
        </p>
      ))}
      {list.failed.map((f) => (
        <p key={f.source} role="status" className="text-xs text-destructive">
          {f.source} could not be read: {f.message}
        </p>
      ))}
      <Section title="Skills" count={list.rows.length}>
        {list.status === "loading" ? (
          <ScreenSkeleton label="Loading skills" />
        ) : list.status === "error" ? (
          /no skills repository/i.test(errorText(lib.error).message) ? (
            <EmptyState
              icon={<BookOpen />}
              title="No skills repository yet"
              description="Toolport keeps your skills, rules, agents and styles in one git-friendly folder. Create it to start."
              action={
                <Button onClick={() => setDialog("init")}>
                  <Plus /> Create repository…
                </Button>
              }
            />
          ) : (
            <ErrorState
              error={lib.error}
              title="Couldn't list skills"
              onRetry={() => {
                list.reload();
                for (const read of [sources, status, lint, audit, diff, plan, collisions])
                  if (read.status === "error") read.reload();
              }}
            />
          )
        ) : list.rows.length === 0 ? (
          <EmptyState
            icon={<BookOpen />}
            title={
              filter === "library"
                ? "No skills in your library yet"
                : "Nothing found in this source"
            }
            description={
              filter === "library"
                ? "A skill is one SKILL.md that Toolport writes in the format each client reads."
                : "Toolport looked and found no skills here."
            }
            action={
              filter === "library" ? (
                <Button onClick={() => setDialog("new")}>
                  <Plus /> Create your first skill
                </Button>
              ) : undefined
            }
          />
        ) : (
          <div className="grid gap-4 lg:grid-cols-[minmax(0,2fr)_minmax(0,3fr)]">
            <ul
              aria-label="Skills"
              className="flex max-h-[32rem] flex-col gap-1 overflow-auto"
            >
              {list.rows.map((r) => (
                <li key={rowKey(r)}>
                  <button
                    type="button"
                    aria-current={r === row ? "true" : undefined}
                    onClick={() => setSelected(rowKey(r))}
                    className="flex w-full items-center gap-2 rounded-lg border px-3 py-2 text-left text-sm outline-none hover:bg-muted focus-visible:ring-1 focus-visible:ring-ring aria-[current=true]:border-primary aria-[current=true]:bg-muted"
                  >
                    <span className="flex min-w-0 flex-1 flex-col">
                      <b className="truncate">{r.name}</b>
                      <small className="truncate text-muted-foreground">
                        {r.description}
                      </small>
                    </span>
                    {!r.visible && <Badge variant="destructive">not visible</Badge>}
                    <SourceBadge origin={r.origin} />
                    {r.type === "rule" && <Badge variant="outline">rule</Badge>}
                  </button>
                </li>
              ))}
            </ul>
            <div className="flex flex-col gap-4">
              {row && (
                <SkillDetail
                  row={row}
                  status={status.data}
                  audit={audit.data}
                  write={write}
                  writeScope={writeScope}
                />
              )}
              <Collisions query={collisions} write={write} writeScope={writeScope} />
            </div>
          </div>
        )}
      </Section>
      <Section title="Checks">
        <div className="grid gap-3 lg:grid-cols-2">
          <Card title="Lint" query={lint}>
            {(data) => <LintSummary data={data} noun="your skills" />}
          </Card>
          <Card title="Audit" query={audit}>
            {(data) => (
              <div className="flex flex-col gap-2">
                <Verdict ok={data.clean}>
                  {data.clean
                    ? `No risky instructions found in ${plural(data.skillCount, "skill")}`
                    : `${data.high} high, ${data.medium} medium, ${data.low} low`}
                </Verdict>
                <Messages
                  messages={(data.findings as AuditFinding[]).map((f) => ({
                    level: f.severity === "high" ? "error" : "warning",
                    name: f.skill,
                    message: `${f.message}${f.line != null ? ` (line ${f.line})` : ""}`,
                  }))}
                />
              </div>
            )}
          </Card>
          <Card title="Changes since last sync" query={diff}>
            {(data) => <DiffBody data={data} />}
          </Card>
          <Card title="Drift" query={status}>
            {(data) => <DriftBody data={data} />}
          </Card>
        </div>
      </Section>
      <Section title="More library actions">
        <p className="text-xs text-muted-foreground">
          These exist only as self-MCP tools today and have no command of their own. They
          switch on when the app can run them through `mcp call`.
        </p>
        <ul aria-label="Actions not available yet" className="flex flex-col gap-2">
          {TOOLS_WITHOUT_CLI.map((tool) => (
            <li key={tool.tool} className="flex flex-wrap items-center gap-3 text-sm">
              <Button size="sm" variant="outline" disabled title={tool.reason}>
                {tool.label}
              </Button>
              <span className="min-w-0 flex-1 text-xs text-muted-foreground">
                {tool.reason}
              </span>
            </li>
          ))}
        </ul>
      </Section>
    </>
  );
}

const SECTIONS = [
  { id: "installed", label: "Installed" },
  { id: "taps", label: "Taps" },
  { id: "install", label: "Find and install" },
  { id: "bundles", label: "Bundles" },
];

/** The Skills panel: every skill and rule with its source, the sync state per client, drift,
 * lint and audit, collisions and the writes that go with them. Every write is previewed. */
export function SkillsTab() {
  const rows = useRegistryRows();
  const [epoch, setEpoch] = useState(0);
  const [filter, setFilter] = useState("library");
  const [selected, setSelected] = useState<string | null>(null);
  const [section, setSection] = useState("installed");
  const [writeScope, setWriteScope] = useState<WriteScope>(USER_SCOPE);
  const write = useWrite(rows, () => setEpoch((n) => n + 1));
  return (
    <div className="flex flex-col gap-6">
      <WriteDialogs write={write} />
      <Tabs
        items={SECTIONS}
        value={section}
        onValueChange={setSection}
        label="Skills sections"
      >
        <div key={epoch} className="flex flex-col gap-6">
          {section === "taps" ? (
            <TapsPanel write={write} />
          ) : section === "install" ? (
            <InstallPanel write={write} />
          ) : section === "bundles" ? (
            <BundlesPanel write={write} />
          ) : (
            <Body
              write={write}
              scope={{
                filter,
                setFilter,
                selected,
                setSelected,
                writeScope,
                setWriteScope,
              }}
            />
          )}
        </div>
      </Tabs>
    </div>
  );
}
