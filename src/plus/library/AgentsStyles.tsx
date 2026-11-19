import { useState } from "react";
import { CheckCircle2, FileText, Plus, RefreshCw } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { SkeletonRows } from "@/components/ui/skeleton";
import { StatusCard } from "@/components/ui/status-card";
import { useLoad } from "../servers/useLoad";
import {
  agentAudit,
  agentLint,
  agentStatus,
  contentDiff,
  listAgents,
  listStyles,
  removeStyle,
  styleLint,
  styleStatus,
  syncAgents,
  syncStyles,
  uninstallAgent,
  type ContentKind,
} from "./commands";
import type { ContentDiffData } from "./data";
import { CleanContentDialog, EditBodyDialog, NewContentDialog } from "./ContentDialogs";
import { FileLines, Mono, WriteDialog } from "./dialogs";
import { plural } from "./model";
import StyleClients from "./StyleClients";

type Dialog =
  | { kind: "sync-agents" }
  | { kind: "uninstall"; name: string }
  | { kind: "sync-styles" }
  | { kind: "remove-styles" }
  | { kind: "new"; content: ContentKind }
  | { kind: "clean"; content: ContentKind }
  | { kind: "edit"; content: ContentKind; name: string }
  | null;

/** What changed in the library since the last sync, as a badge beside the item. */
function Changed({ diff, name }: { diff: ContentDiffData | null; name: string }) {
  if (!diff || diff.noLockfile) return null;
  const label = diff.new.includes(name)
    ? "New since the last sync"
    : diff.modified.includes(name)
      ? "Edited since the last sync"
      : null;
  return label ? <Badge variant="warning">{label}</Badge> : null;
}

function CheckTile({
  title,
  value,
  tone,
}: {
  title: string;
  value: string;
  tone: "ok" | "warn";
}) {
  return (
    <div className="flex items-center justify-between gap-2 rounded-xl border bg-card px-3 py-2.5">
      <span className="text-sm font-medium">{title}</span>
      <Badge variant={tone === "ok" ? "success" : "warning"}>{value}</Badge>
    </div>
  );
}

function Section({
  title,
  count,
  children,
}: {
  title: string;
  count: number;
  children?: React.ReactNode;
}) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-2">
      <h2 className="text-meta font-semibold tracking-wide text-muted-foreground uppercase">
        {title} <span className="ml-1 font-normal normal-case">{count}</span>
      </h2>
      {children}
    </div>
  );
}

async function optionalDiff(kind: ContentKind): Promise<ContentDiffData | null> {
  try {
    return await contentDiff(kind);
  } catch {
    return null;
  }
}

/** Agents and output styles: what the library holds and whether Claude Code has it. Each syncs
 * with its own command and preview; Apply on the Sync tab covers skills, rules and hooks. */
export default function AgentsStyles() {
  const agents = useLoad(listAgents, "agents");
  const status = useLoad(agentStatus, "agent-status");
  const lint = useLoad(agentLint, "agent-lint");
  const audit = useLoad(agentAudit, "agent-audit");
  const styles = useLoad(listStyles, "styles");
  const styleChecks = useLoad(styleLint, "style-lint");
  const styleState = useLoad(styleStatus, "style-status");
  const agentDiff = useLoad(() => optionalDiff("agents"), "agent-diff");
  const styleDiff = useLoad(() => optionalDiff("styles"), "style-diff");
  const [dialog, setDialog] = useState<Dialog>(null);

  const reload = () => {
    agentDiff.reload();
    styleDiff.reload();
    agents.reload();
    status.reload();
    lint.reload();
    audit.reload();
    styles.reload();
    styleChecks.reload();
    styleState.reload();
  };
  const list = agents.data?.agents ?? [];
  const styleList = styles.data?.styles ?? [];
  const inSync = status.data ? status.data.lockfilePresent && !status.data.drift : null;
  const lintTotal =
    (lint.data?.errors ?? 0) +
    (lint.data?.warnings ?? 0) +
    (styleChecks.data?.errors ?? 0) +
    (styleChecks.data?.warnings ?? 0);
  const findings = audit.data?.findings.length ?? 0;
  const findingRows = [
    ...(lint.data?.messages ?? []).map((m) => ({ ...m, kind: "agent" })),
    ...(styleChecks.data?.messages ?? []).map((m) => ({ ...m, kind: "style" })),
  ].filter((m) => m.level === "error" || m.level === "warning");

  if (agents.error && !agents.data) {
    return (
      <p role="alert" className="text-meta text-destructive">
        {agents.error}
      </p>
    );
  }
  if (!agents.data) return <SkeletonRows label="Reading agents and styles" />;

  return (
    <div className="flex flex-col gap-5">
      <StatusCard
        variant={list.length === 0 || inSync ? "success" : "warning"}
        icon={inSync || list.length === 0 ? <CheckCircle2 /> : <RefreshCw />}
        title={
          list.length === 0
            ? "No agents in your library"
            : inSync
              ? "Agents are in sync"
              : `${plural(list.length, "agent")} not in Claude Code yet`
        }
        detail="Agents belong to the same Apply as skills: one plan, one snapshot, one Undo. This tab is for looking at an agent, editing it and seeing how Claude Code gets it."
        actions={
          list.length > 0 && (
            <Button
              size="sm"
              variant={inSync ? "outline" : "default"}
              onClick={() => setDialog({ kind: "sync-agents" })}
            >
              <RefreshCw /> Sync agents
            </Button>
          )
        }
      />

      <section aria-label="Agents" className="flex flex-col gap-2">
        <Section title="Agents" count={list.length}>
          <div className="flex flex-wrap gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => setDialog({ kind: "new", content: "agents" })}
            >
              <Plus /> New agent…
            </Button>
            {list.length > 0 && (
              <Button
                variant="outline"
                size="sm"
                onClick={() => setDialog({ kind: "clean", content: "agents" })}
              >
                Remove synced copies…
              </Button>
            )}
          </div>
        </Section>
        {list.map((agent) => (
          <div key={agent.name} className="rounded-xl border bg-card p-4">
            <div className="flex flex-wrap items-center gap-2">
              <h3 className="text-sm font-semibold">{agent.name}</h3>
              <Badge variant="outline">{agent.model}</Badge>
              {agent.tools.map((tool) => (
                <Badge key={tool} variant="outline">
                  {tool}
                </Badge>
              ))}
              <Changed diff={agentDiff.data} name={agent.name} />
              <span className="ml-auto flex gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() =>
                    setDialog({ kind: "edit", content: "agents", name: agent.name })
                  }
                >
                  Edit body
                </Button>
                <Button
                  variant="destructive"
                  size="sm"
                  onClick={() => setDialog({ kind: "uninstall", name: agent.name })}
                >
                  Uninstall…
                </Button>
              </span>
            </div>
            <p className="mt-1 text-meta">{agent.description}</p>
            <p className="mt-1">
              <Mono>{agent.path}</Mono>
            </p>
            <div className="mt-3 flex flex-wrap items-center gap-2 rounded-lg border px-3 py-2 text-meta">
              <span className="font-medium">Claude Code</span>
              <Mono>~/.claude/agents/{agent.name}.md</Mono>
              <Badge variant={inSync ? "success" : "warning"} className="ml-auto">
                {inSync ? "In sync" : "Not synced"}
              </Badge>
            </div>
          </div>
        ))}
        {list.length === 0 && (
          <p className="rounded-xl border bg-card px-4 py-4 text-meta text-muted-foreground">
            Agents live in the agents folder of your library and are delivered to Claude
            Code like skills.
          </p>
        )}
      </section>

      <section aria-label="Output styles" className="flex flex-col gap-2">
        <Section title="Output styles" count={styleList.length}>
          <div className="flex flex-wrap gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => setDialog({ kind: "new", content: "styles" })}
            >
              <Plus /> New style…
            </Button>
            {styleList.length > 0 && (
              <>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => setDialog({ kind: "sync-styles" })}
                >
                  <RefreshCw /> Sync styles
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => setDialog({ kind: "clean", content: "styles" })}
                >
                  Remove synced copies…
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => setDialog({ kind: "remove-styles" })}
                >
                  Remove applied style…
                </Button>
              </>
            )}
          </div>
        </Section>
        {styleList.length === 0 ? (
          <EmptyState
            icon={<FileText />}
            title="No output styles"
            description="An output style changes how Claude Code words its replies. Keep them in your library like skills and they are delivered the same way. You have none."
          />
        ) : (
          <ul className="divide-y overflow-hidden rounded-xl border bg-card">
            {styleList.map((style) => (
              <li
                key={style.name}
                className="flex flex-wrap items-center gap-3 px-4 py-3"
              >
                <div className="min-w-48 flex-1">
                  <p className="text-sm font-medium">{style.name}</p>
                  <p className="text-meta text-muted-foreground">{style.description}</p>
                </div>
                <Changed diff={styleDiff.data} name={style.name} />
                <Badge variant={style.synced ? "success" : "warning"}>
                  {style.synced ? "In sync" : "Not synced"}
                </Badge>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() =>
                    setDialog({ kind: "edit", content: "styles", name: style.name })
                  }
                >
                  Edit body
                </Button>
              </li>
            ))}
          </ul>
        )}
        {styleList.length > 0 && styleState.data && !styleState.data.lockfilePresent && (
          <p className="text-meta text-muted-foreground">
            Styles were never synced to Claude Code.
          </p>
        )}
      </section>

      <StyleClients
        status={styleState.data}
        error={styleState.error}
        styles={styleList.map((style) => style.name)}
        onChanged={reload}
      />

      <section aria-label="Agent checks" className="flex flex-col gap-2">
        <h2 className="text-meta font-semibold tracking-wide text-muted-foreground uppercase">
          Checks
        </h2>
        <div className="grid grid-cols-1 gap-2 sm:grid-cols-3">
          <CheckTile
            title="Lint"
            value={
              lint.data ? (lintTotal === 0 ? "None" : String(lintTotal)) : "Could not run"
            }
            tone={lint.data && lintTotal === 0 ? "ok" : "warn"}
          />
          <CheckTile
            title="Audit"
            value={
              audit.data ? (findings === 0 ? "None" : String(findings)) : "Could not run"
            }
            tone={audit.data && findings === 0 ? "ok" : "warn"}
          />
          <CheckTile
            title="Drift"
            value={
              status.data ? (status.data.drift ? "Edited" : "None") : "Could not run"
            }
            tone={status.data && !status.data.drift ? "ok" : "warn"}
          />
        </div>
        {findingRows.length > 0 && (
          <ul
            aria-label="Lint findings"
            className="divide-y overflow-hidden rounded-xl border bg-card"
          >
            {findingRows.map((row, index) => (
              <li
                key={`${row.kind}-${row.name}-${index}`}
                className="flex flex-wrap items-start gap-x-3 gap-y-1 px-4 py-2.5 text-meta"
              >
                <Badge variant={row.level === "error" ? "destructive" : "warning"}>
                  {row.level === "error" ? "Error" : "Warning"}
                </Badge>
                <span className="min-w-0 flex-1 break-words">
                  <span className="font-medium">{row.name}</span> ({row.kind}):{" "}
                  {row.message}
                </span>
              </li>
            ))}
          </ul>
        )}
      </section>

      {dialog?.kind === "new" && (
        <NewContentDialog
          kind={dialog.content}
          onClose={() => setDialog(null)}
          onDone={reload}
        />
      )}
      {dialog?.kind === "clean" && (
        <CleanContentDialog
          kind={dialog.content}
          onClose={() => setDialog(null)}
          onDone={reload}
        />
      )}
      {dialog?.kind === "edit" && (
        <EditBodyDialog
          kind={dialog.content}
          name={dialog.name}
          onClose={() => setDialog(null)}
          onDone={reload}
        />
      )}
      {dialog?.kind === "sync-agents" && (
        <WriteDialog
          title="Sync agents"
          description="Writes each agent into ~/.claude/agents."
          confirmLabel="Sync agents"
          preview={() => syncAgents(true)}
          render={(data) => (
            <FileLines
              title={plural(data.agentCount, "agent")}
              files={data.agents.flatMap((a) => a.outputFiles.flatMap((o) => o.files))}
            />
          )}
          confirm={() => syncAgents(false)}
          onClose={() => setDialog(null)}
          onDone={reload}
        />
      )}
      {dialog?.kind === "uninstall" && (
        <WriteDialog
          title={`Uninstall ${dialog.name}`}
          description="Removes the agent from your library and its copy in ~/.claude/agents."
          confirmLabel="Uninstall"
          destructive
          preview={() => uninstallAgent(dialog.name, true)}
          render={(data) => <FileLines title="Copies removed" files={data.outputs} />}
          confirm={() => uninstallAgent(dialog.name, false)}
          onClose={() => setDialog(null)}
          onDone={reload}
        />
      )}
      {dialog?.kind === "sync-styles" && (
        <WriteDialog
          title="Sync styles"
          confirmLabel="Sync styles"
          preview={() => syncStyles(true)}
          render={(data) => (
            <FileLines title={plural(data.styleCount, "style")} files={data.outputs} />
          )}
          confirm={() => syncStyles(false)}
          onClose={() => setDialog(null)}
          onDone={reload}
        />
      )}
      {dialog?.kind === "remove-styles" && (
        <WriteDialog
          title="Remove the applied style"
          description="Takes the style off the clients that got it as an always-on rule. The style stays in your library."
          confirmLabel="Remove applied style"
          destructive
          preview={() => removeStyle(true)}
          render={(data) =>
            data.removed.length === 0 ? (
              <p className="text-meta">No client has an applied style.</p>
            ) : (
              <FileLines
                title={`${plural(data.removed.length, "rule file")} removed`}
                files={data.removed.map((r) => r.path)}
              />
            )
          }
          confirm={() => removeStyle(false)}
          onClose={() => setDialog(null)}
          onDone={reload}
        />
      )}
    </div>
  );
}
