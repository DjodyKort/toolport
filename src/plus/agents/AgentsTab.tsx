import { useState } from "react";
import { Plus, RefreshCw, Trash2, TriangleAlert } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import type { AgentsLsData } from "../bridge/data";
import type {
  AgentsAuditData,
  AgentsDiffData,
  AgentsLintData,
  AgentsStatusData,
  AgentsSyncData,
} from "../types/agents";
import { AsyncView, errorText } from "../ui";
import { useRead, useRegistryRows, useWrite, type WriteControl } from "./hooks";
import {
  byClient,
  clientFlag,
  clientName,
  droppedOf,
  plural,
  warnsClient,
  type AuditFinding,
  type StatusOutput,
} from "./model";
import { NewDialog } from "./NewDialog";
import { Card, DiffBody, Discovery, LintBody, PathLine, Section, Verdict } from "./parts";
import { WriteDialogs } from "./WriteDialogs";

type Agent = AgentsLsData["agents"][number];

function ClientRow({
  agent,
  client,
  files,
  warnings,
  state,
  write,
}: {
  agent: string;
  client: string;
  files: string[];
  warnings: string[];
  state: "ok" | "missing" | "unsynced";
  write: WriteControl;
}) {
  const dropped = warnings.map(droppedOf).filter((d) => warnsClient(d, client));
  return (
    <li className="grid gap-1 border-t px-3 py-2 text-sm first:border-t-0 sm:grid-cols-[9rem_minmax(0,1fr)_auto] sm:items-start sm:gap-3">
      <span className="font-medium">{clientName(client)}</span>
      <div className="flex min-w-0 flex-col gap-1">
        <code className="font-mono text-xs break-all text-muted-foreground">
          {files.join(", ")}
        </code>
        {dropped.map((d, i) => (
          <p key={i} className="flex items-start gap-1.5 text-xs text-warning">
            <TriangleAlert className="mt-px size-3.5 shrink-0" aria-hidden="true" />
            <span>
              {d.field ? (
                <>
                  <b>{d.field}</b> is dropped for {clientName(client)}
                </>
              ) : (
                d.text
              )}
              <span className="sr-only">: {d.text}</span>
            </span>
          </p>
        ))}
      </div>
      <div className="flex items-center gap-2">
        {state === "ok" && <Badge variant="success">In sync</Badge>}
        {state === "missing" && <Badge variant="destructive">Missing</Badge>}
        {state === "unsynced" && <Badge variant="outline">Not written yet</Badge>}
        <Button
          size="xs"
          variant="outline"
          aria-label={`Sync ${agent} to ${clientName(client)}`}
          disabled={write.busy}
          onClick={() =>
            write.begin({
              command: "agents sync",
              title: `Sync agents to ${clientName(client)}`,
              argv: ["agents", "sync", ...clientFlag(client)],
              confirmLabel: "Sync",
              phrase: "sync",
            })
          }
        >
          Sync
        </Button>
      </div>
    </li>
  );
}

function AgentCard({
  agent,
  sync,
  status,
  write,
}: {
  agent: Agent;
  sync: AgentsSyncData | null;
  status: AgentsStatusData | null;
  write: WriteControl;
}) {
  const entry = sync?.agents.find((a) => a.name === agent.name);
  const present = new Map(
    (status?.outputs as StatusOutput[] | undefined)
      ?.filter((o) => o.name === agent.name)
      .map((o) => [o.client, o.present]),
  );
  const clients = [...(entry?.outputFiles ?? [])].sort((a, b) =>
    byClient(a.client, b.client),
  );
  return (
    <li className="rounded-lg border bg-card">
      <div className="flex flex-wrap items-start justify-between gap-3 p-4">
        <div className="flex min-w-0 flex-col gap-1">
          <p className="flex flex-wrap items-center gap-2">
            <b className="text-sm">{agent.name}</b>
            <Badge variant="secondary">{agent.model || "inherit"}</Badge>
            {agent.tools.map((tool) => (
              <code key={tool} className="rounded bg-muted px-1.5 font-mono text-xs">
                {tool}
              </code>
            ))}
          </p>
          {agent.description && (
            <p className="text-sm text-muted-foreground">{agent.description}</p>
          )}
          <PathLine path={agent.path} />
        </div>
        <div className="flex flex-wrap gap-2">
          <Button
            size="sm"
            variant="outline"
            disabled
            title="Editing the body needs `mcp call agents_edit_body` (MIG-GUI-14). Open the file above in your editor for now."
            aria-label={`Edit body of ${agent.name}`}
          >
            Edit body
          </Button>
          <Button
            size="sm"
            variant="outline"
            aria-label={`Uninstall ${agent.name}`}
            disabled={write.busy}
            onClick={() =>
              write.begin({
                command: "agents uninstall",
                title: `Uninstall agent ${agent.name}`,
                argv: ["agents", "uninstall", agent.name],
                confirmLabel: "Uninstall",
                phrase: agent.name,
              })
            }
          >
            <Trash2 /> Uninstall…
          </Button>
        </div>
      </div>
      {clients.length > 0 && (
        <ul aria-label={`Output of ${agent.name} per client`} className="border-t">
          {clients.map((out) => (
            <ClientRow
              key={out.client}
              agent={agent.name}
              client={out.client}
              files={out.files}
              warnings={entry?.warnings.map(String) ?? []}
              state={
                !status?.lockfilePresent
                  ? "unsynced"
                  : present.get(out.client) === false
                    ? "missing"
                    : present.has(out.client)
                      ? "ok"
                      : "unsynced"
              }
              write={write}
            />
          ))}
        </ul>
      )}
    </li>
  );
}

function Agents({ write, onNew }: { write: WriteControl; onNew: () => void }) {
  const list = useRead<AgentsLsData>(["agents", "ls"]);
  const sync = useRead<AgentsSyncData>(["agents", "sync", "--dry-run"]);
  const status = useRead<AgentsStatusData>(["agents", "status"]);
  const lint = useRead<AgentsLintData>(["agents", "lint"]);
  const audit = useRead<AgentsAuditData>(["agents", "audit"]);
  const diff = useRead<AgentsDiffData>(["agents", "diff"]);
  return (
    <>
      <Section title="Agents" count={list.data?.agents.length}>
        <AsyncView
          query={list}
          errorTitle="Couldn't list agents"
          isEmpty={(d) => d.agents.length === 0}
          empty={
            <EmptyState
              title="No agents yet"
              description="An agent is one AGENT.md that Toolport writes in the format each client reads."
              action={
                <Button onClick={onNew}>
                  <Plus /> Create your first agent
                </Button>
              }
            />
          }
        >
          {(data) => (
            <>
              <Discovery warnings={data.discoveryWarnings} />
              {sync.status === "error" && (
                <p role="status" className="text-sm text-muted-foreground">
                  The per-client output could not be worked out:{" "}
                  {errorText(sync.error).message}
                </p>
              )}
              <ul className="flex flex-col gap-3">
                {data.agents.map((agent) => (
                  <AgentCard
                    key={agent.name}
                    agent={agent}
                    sync={sync.data}
                    status={status.data}
                    write={write}
                  />
                ))}
              </ul>
            </>
          )}
        </AsyncView>
      </Section>
      <Section title="Checks">
        <div className="grid gap-3 lg:grid-cols-2">
          <Card title="Lint" query={lint}>
            {(data) => <LintBody data={data} noun="your agents" />}
          </Card>
          <Card title="Audit" query={audit}>
            {(data) => <AuditBody data={data} />}
          </Card>
          <Card title="Changes since last sync" query={diff}>
            {(data) => <DiffBody data={data} noun="agent" />}
          </Card>
          <Card title="Drift" query={status}>
            {(data) => <DriftBody data={data} />}
          </Card>
        </div>
      </Section>
    </>
  );
}

function AuditBody({ data }: { data: AgentsAuditData }) {
  const findings = data.findings as AuditFinding[];
  return (
    <div className="flex flex-col gap-2">
      <Verdict ok={data.clean}>
        {data.clean
          ? `No risky instructions found in ${plural(data.agentCount, "agent")}`
          : `${data.high} high, ${data.medium} medium, ${data.low} low`}
      </Verdict>
      <ul className="flex flex-col gap-1.5 text-sm">
        {findings.map((f, i) => (
          <li key={i} className="flex flex-wrap items-baseline gap-2">
            <Badge variant={f.severity === "high" ? "destructive" : "warning"}>
              {f.severity}
            </Badge>
            <code className="font-mono text-xs">{f.agent}</code>
            <span className="min-w-0 break-words">
              {f.message}
              {f.line != null && ` (line ${f.line})`}
            </span>
          </li>
        ))}
      </ul>
      <Discovery warnings={data.discoveryWarnings.map(String)} />
    </div>
  );
}

function DriftBody({ data }: { data: AgentsStatusData }) {
  const outputs = data.outputs as StatusOutput[];
  const missing = outputs.filter((o) => !o.present);
  if (!data.lockfilePresent)
    return <Verdict ok={false}>Not synced yet. Sync writes the outputs.</Verdict>;
  return (
    <div className="flex flex-col gap-2">
      <Verdict ok={!data.drift}>
        {data.drift
          ? `${plural(missing.length, "output")} missing since the last sync`
          : `All ${plural(data.lockedCount, "synced agent")} still in place`}
      </Verdict>
      <ul className="flex flex-col gap-1 text-sm">
        {missing.map((o, i) => (
          <li key={i}>
            <code className="font-mono text-xs">{o.name}</code> is missing for{" "}
            {clientName(o.client)}
          </li>
        ))}
      </ul>
    </div>
  );
}

/** The Agents panel: the agents, what each client gets (with the fields a client drops), and
 * the lint, audit, diff and drift checks. Every write is previewed first. */
export function AgentsTab() {
  const rows = useRegistryRows();
  const [epoch, setEpoch] = useState(0);
  const [naming, setNaming] = useState(false);
  const write = useWrite(rows, () => setEpoch((n) => n + 1));
  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="max-w-prose text-sm text-muted-foreground">
          One AGENT.md in your skills repository becomes a file in each client's own
          format. Fields a client has no place for are dropped, and shown here.
        </p>
        <div className="flex flex-wrap gap-2">
          <Button variant="outline" disabled={write.busy} onClick={() => setNaming(true)}>
            <Plus /> New agent…
          </Button>
          <Button
            disabled={write.busy}
            onClick={() =>
              write.begin({
                command: "agents sync",
                title: "Sync agents to every client",
                argv: ["agents", "sync"],
                confirmLabel: "Sync",
                phrase: "sync",
              })
            }
          >
            <RefreshCw /> Sync…
          </Button>
          <Button
            variant="outline"
            disabled={write.busy}
            onClick={() =>
              write.begin({
                command: "agents clean",
                title: "Remove synced agent files",
                argv: ["agents", "clean"],
                confirmLabel: "Remove",
                phrase: "clean agents",
              })
            }
          >
            <Trash2 /> Clean outputs…
          </Button>
        </div>
      </div>
      <WriteDialogs write={write} />
      {naming && (
        <NewDialog
          kind="agent"
          onClose={() => setNaming(false)}
          onSubmit={(name) => {
            setNaming(false);
            write.begin({
              command: "agents add",
              title: `Create agent ${name}`,
              argv: ["agents", "add", name],
              confirmLabel: "Create",
              phrase: name,
            });
          }}
        />
      )}
      <Agents key={epoch} write={write} onNew={() => setNaming(true)} />
    </div>
  );
}
