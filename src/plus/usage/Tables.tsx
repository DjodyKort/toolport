import { useState, type ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { Cell, DataTable, Muted, Section } from "./atoms";
import {
  baseName,
  compact,
  exact,
  formatTs,
  percent,
  share,
  tokensOf,
  type Counts,
  type McpFailure,
  type ModelRow,
  type ProjectRow,
  type ServerRow,
  type SessionRow,
  type UsageView,
} from "./model";

const SHOWN = 8;

function Limited<T>({
  rows,
  noun,
  children,
}: {
  rows: T[];
  noun: string;
  children: (rows: T[]) => ReactNode;
}) {
  const [all, setAll] = useState(false);
  const shown = all ? rows : rows.slice(0, SHOWN);
  return (
    <>
      {children(shown)}
      {rows.length > SHOWN && (
        <Button
          size="xs"
          variant="ghost"
          className="self-start"
          aria-expanded={all}
          onClick={() => setAll((open) => !open)}
        >
          {all ? `Show the top ${SHOWN}` : `Show all ${rows.length} ${noun}`}
        </Button>
      )}
    </>
  );
}

export function ServersCard({ servers }: { servers: ServerRow[] }) {
  const [open, setOpen] = useState<Set<string>>(new Set());
  const toggle = (name: string) =>
    setOpen((prev) => {
      const next = new Set(prev);
      if (!next.delete(name)) next.add(name);
      return next;
    });
  return (
    <Section
      title="By MCP server"
      note="Calls to each server's tools, from the tool_use blocks in the transcripts. Tokens are not split per server."
    >
      {servers.length === 0 ? (
        <Muted>No MCP tool calls in the indexed transcripts.</Muted>
      ) : (
        <Limited rows={servers} noun="servers">
          {(rows) => (
            <DataTable
              caption="Calls by MCP server"
              columns={[
                { label: "Server" },
                { label: "Calls", numeric: true },
                { label: "Tools", numeric: true },
              ]}
            >
              {rows.map((server) => {
                const expanded = open.has(server.name);
                return (
                  <ServerRows
                    key={server.name}
                    server={server}
                    expanded={expanded}
                    onToggle={() => toggle(server.name)}
                  />
                );
              })}
            </DataTable>
          )}
        </Limited>
      )}
    </Section>
  );
}

function ServerRows({
  server,
  expanded,
  onToggle,
}: {
  server: ServerRow;
  expanded: boolean;
  onToggle: () => void;
}) {
  return (
    <>
      <tr>
        <Cell>
          <b className="font-medium">{server.name}</b>
        </Cell>
        <Cell numeric>{exact(server.calls)}</Cell>
        <Cell numeric>
          <Button
            size="xs"
            variant="ghost"
            aria-expanded={expanded}
            aria-label={`${expanded ? "Hide" : "Show"} the ${server.tools.length} tools of ${server.name}`}
            onClick={onToggle}
          >
            {server.tools.length}
          </Button>
        </Cell>
      </tr>
      {expanded &&
        server.tools.map((tool) => (
          <tr key={tool.name} className="bg-muted/40">
            <Cell className="pl-6">
              <code className="font-mono text-xs break-all">{tool.name}</code>
            </Cell>
            <Cell numeric>{exact(tool.calls)}</Cell>
            <Cell>{""}</Cell>
          </tr>
        ))}
    </>
  );
}

export function ProjectsCard({ projects }: { projects: ProjectRow[] }) {
  return (
    <Section
      title="By project"
      note="Sessions grouped by the folder they ran in. All indexed messages."
    >
      {projects.length === 0 ? (
        <Muted>No sessions in the index.</Muted>
      ) : (
        <Limited rows={projects} noun="projects">
          {(rows) => (
            <DataTable
              caption="Tokens by project"
              columns={[
                { label: "Project" },
                { label: "Sessions", numeric: true },
                { label: "Messages", numeric: true },
                { label: "Tokens", numeric: true },
                { label: "Cache read", numeric: true },
              ]}
            >
              {rows.map((project) => (
                <tr key={project.cwd}>
                  <Cell>
                    <b className="font-medium">{project.name}</b>
                    {project.name !== project.cwd && (
                      <small className="block font-mono text-xs break-all text-muted-foreground">
                        {project.cwd}
                      </small>
                    )}
                  </Cell>
                  <Cell numeric>{exact(project.sessions)}</Cell>
                  <Cell numeric>{exact(project.messages)}</Cell>
                  <Cell numeric title={exact(tokensOf(project))}>
                    {exact(tokensOf(project))}
                  </Cell>
                  <Cell numeric>
                    {percent(share(project.cacheRead, tokensOf(project)))}
                  </Cell>
                </tr>
              ))}
            </DataTable>
          )}
        </Limited>
      )}
    </Section>
  );
}

export function SessionsCard({ sessions }: { sessions: SessionRow[] }) {
  const top = sessions.slice(0, 10);
  return (
    <Section
      title="Top sessions"
      note="The most tokens first. A transcript holds no price, so this ranks by tokens, not by cost."
    >
      {top.length === 0 ? (
        <Muted>No sessions in the index.</Muted>
      ) : (
        <DataTable
          caption="Sessions with the most tokens"
          columns={[
            { label: "Session" },
            { label: "Project" },
            { label: "Last message" },
            { label: "Messages", numeric: true },
            { label: "Tokens", numeric: true },
          ]}
        >
          {top.map((session) => (
            <tr key={session.id}>
              <Cell>
                <code
                  className="block max-w-[11rem] truncate font-mono text-xs"
                  title={session.id}
                >
                  {session.id}
                </code>
              </Cell>
              <Cell className="break-all">
                {session.cwd ? baseName(session.cwd) : "unknown"}
              </Cell>
              <Cell className="whitespace-nowrap">{formatTs(session.lastTs)}</Cell>
              <Cell numeric>{exact(session.messages)}</Cell>
              <Cell numeric title={exact(tokensOf(session))}>
                {exact(tokensOf(session))}
              </Cell>
            </tr>
          ))}
        </DataTable>
      )}
    </Section>
  );
}

export function ModelsCard({ models }: { models: ModelRow[] }) {
  return (
    <Section title="By model" note="All indexed messages.">
      {models.length === 0 ? (
        <Muted>No messages in the index.</Muted>
      ) : (
        <DataTable
          caption="Tokens by model"
          columns={[
            { label: "Model" },
            { label: "Messages", numeric: true },
            { label: "Input", numeric: true },
            { label: "Output", numeric: true },
            { label: "Cache write", numeric: true },
            { label: "Cache read", numeric: true },
          ]}
        >
          {models.map((model) => (
            <tr key={model.name}>
              <Cell className="break-all">{model.name || "unknown"}</Cell>
              <Cell numeric>{exact(model.messages)}</Cell>
              <Cell numeric>{exact(model.input)}</Cell>
              <Cell numeric>{exact(model.output)}</Cell>
              <Cell numeric>{exact(model.cacheCreation)}</Cell>
              <Cell numeric>{exact(model.cacheRead)}</Cell>
            </tr>
          ))}
        </DataTable>
      )}
    </Section>
  );
}

export function CacheCard({ totals }: { totals: Counts }) {
  const all = tokensOf(totals);
  const rows: Array<[string, string, string]> = [
    [
      "Cache read",
      exact(totals.cacheRead),
      `${percent(share(totals.cacheRead, all))} of all tokens`,
    ],
    [
      "Cache write",
      exact(totals.cacheCreation),
      `${percent(share(totals.cacheCreation, all))} of all tokens`,
    ],
    ["Input, not cached", exact(totals.input), ""],
    ["Output", exact(totals.output), ""],
  ];
  const ratio = totals.cacheCreation > 0 ? totals.cacheRead / totals.cacheCreation : null;
  return (
    <Section
      title="Cache read and write"
      note="All indexed messages. Cache read is context Claude Code reused instead of sending again."
    >
      {all === 0 ? (
        <Muted>No tokens in the index.</Muted>
      ) : (
        <dl className="grid grid-cols-[minmax(7rem,max-content)_minmax(0,1fr)_minmax(0,1fr)] gap-x-4 gap-y-2 text-sm">
          {rows.map(([label, value, note]) => (
            <div key={label} className="contents">
              <dt className="text-muted-foreground">{label}</dt>
              <dd className="text-right tabular-nums">{value}</dd>
              <dd className="text-xs text-muted-foreground">{note}</dd>
            </div>
          ))}
          <div className="contents">
            <dt className="text-muted-foreground">Reads per write</dt>
            <dd className="text-right tabular-nums">
              {ratio === null ? "n/a" : `${ratio.toFixed(1)} to 1`}
            </dd>
            <dd className="text-xs text-muted-foreground">
              {compact(totals.cacheRead)} read, {compact(totals.cacheCreation)} written
            </dd>
          </div>
        </dl>
      )}
    </Section>
  );
}

export function FailuresCard({ failures }: { failures: McpFailure[] }) {
  if (failures.length === 0) return null;
  return (
    <Section
      title="MCP connection failures"
      note="Servers that failed to connect in a session, from the transcripts."
    >
      <DataTable
        caption="MCP connection failures"
        columns={[{ label: "Server" }, { label: "Session" }, { label: "When" }]}
      >
        {failures.map((failure, i) => (
          <tr key={`${failure.session}-${failure.server}-${i}`}>
            <Cell>{failure.server || "unknown"}</Cell>
            <Cell>
              <code className="block max-w-[11rem] truncate font-mono text-xs">
                {failure.session}
              </code>
            </Cell>
            <Cell className="whitespace-nowrap">{formatTs(failure.ts)}</Cell>
          </tr>
        ))}
      </DataTable>
    </Section>
  );
}

export function SourcesCard({
  view,
  newest,
  indexedAt,
}: {
  view: UsageView;
  newest: string | null;
  indexedAt: string | null;
}) {
  const { index, sources } = view;
  return (
    <Section title="Where these numbers come from">
      <p className="text-sm">
        Every assistant message is counted once, keyed by its message.id. Claude Code
        writes a message several times while it streams, and a resumed session repeats
        earlier turns: the last record of an id wins, and an id that shows up in more than
        one file still counts once.
      </p>
      <dl className="grid grid-cols-[minmax(8rem,max-content)_minmax(0,1fr)] gap-x-4 gap-y-2 text-sm">
        <dt className="text-muted-foreground">Indexed</dt>
        <dd>
          {exact(index.messages)} messages from {exact(index.files)} transcript files
        </dd>
        <dt className="text-muted-foreground">Newest message</dt>
        <dd>{newest ? formatTs(newest) : "none"}</dd>
        <dt className="text-muted-foreground">Index refreshed</dt>
        <dd>
          {indexedAt
            ? `${formatTs(indexedAt)}, by Refresh in this window`
            : "Not in this window: these are the stored figures. Refresh re-indexes."}
        </dd>
        <dt className="text-muted-foreground">OTel requests</dt>
        <dd>
          {exact(sources.otelRequests)} seen by the receiver, {exact(sources.otelOnly)}{" "}
          not in the transcripts (counted in the totals once)
        </dd>
      </dl>
    </Section>
  );
}
