import { useState, type ReactNode } from "react";
import { AlertTriangle, Info, RefreshCw, XCircle } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { useLoad } from "../servers/useLoad";
import { auditSkills } from "./commands";
import type { LintData, LintRow, SyncData } from "./data";
import { count, lintGroups, meter, plural } from "./model";
import { ChmodDialog, MoveLinkDialog, ResolveDialog } from "./writes";

type Tab = "lint" | "audit" | "collisions" | "drift";

function Tile({
  id,
  active,
  title,
  note,
  badges,
  onSelect,
}: {
  id: Tab;
  active: boolean;
  title: string;
  note: string;
  badges: ReactNode;
  onSelect: (id: Tab) => void;
}) {
  return (
    <button
      type="button"
      role="tab"
      aria-selected={active}
      aria-controls="checks-list"
      id={`check-${id}`}
      onClick={() => onSelect(id)}
      className={cn(
        "flex min-w-0 flex-col gap-1 rounded-xl border bg-card p-3 text-left hover:bg-muted/40",
        active && "border-primary ring-2 ring-primary/30",
      )}
    >
      <span className="flex flex-wrap items-center justify-between gap-1.5 text-sm font-medium">
        {title}
        <span className="flex flex-wrap gap-1">{badges}</span>
      </span>
      <span className="text-meta text-muted-foreground">{note}</span>
    </button>
  );
}

function Meter({
  length,
  limit,
  specLimit,
}: {
  length: number;
  limit: number;
  specLimit: number;
}) {
  const m = meter(length, limit, specLimit);
  return (
    <div
      role="img"
      aria-label={`${count(length)} characters; limits ${count(specLimit)} and ${count(limit)}`}
      className="relative mt-1.5 h-1.5 w-full max-w-md rounded-full bg-muted"
    >
      <div
        className={cn("h-full rounded-full", m.over ? "bg-destructive" : "bg-warning")}
        style={{ width: `${m.fill}%` }}
      />
      <span
        className="absolute -top-0.5 h-2.5 w-px bg-foreground/50"
        style={{ left: `${m.specTick}%` }}
      />
      <span
        className="absolute -top-0.5 h-2.5 w-px bg-foreground/50"
        style={{ left: `${m.limitTick}%` }}
      />
    </div>
  );
}

function LintRowView({
  row,
  onOpenSkill,
  onChmod,
  onMoveLink,
}: {
  row: LintRow;
  onOpenSkill: (name: string) => void;
  onChmod: (skill: string) => void;
  onMoveLink: (skill: string, link: string) => void;
}) {
  const Icon =
    row.level === "error" ? XCircle : row.level === "warning" ? AlertTriangle : Info;
  const tone =
    row.level === "error"
      ? "text-destructive"
      : row.level === "warning"
        ? "text-warning"
        : "text-muted-foreground";
  return (
    <li className="flex items-start gap-3 px-4 py-2.5">
      <Icon aria-hidden className={cn("mt-0.5 size-4 shrink-0", tone)} />
      <div className="min-w-0 flex-1">
        <p className="text-sm font-medium">
          {row.name}
          <span className="sr-only">
            {" "}
            {row.level === "error"
              ? "Error"
              : row.level === "warning"
                ? "Warning"
                : "Info"}
          </span>
        </p>
        <p className="text-meta text-muted-foreground">
          {row.message}
          {row.level === "error" && " Held back from sync."}
        </p>
        {row.meter && <Meter {...row.meter} specLimit={row.meter.specLimit} />}
      </div>
      {row.fixable === "chmod" ? (
        <Button variant="outline" size="xs" onClick={() => onChmod(row.name)}>
          Make executable
        </Button>
      ) : row.fixable === "move-link" && row.target ? (
        <Button
          variant="outline"
          size="xs"
          onClick={() => onMoveLink(row.name, row.target ?? "")}
        >
          Move into the skill…
        </Button>
      ) : (
        <Button variant="outline" size="xs" onClick={() => onOpenSkill(row.name)}>
          Open skill
        </Button>
      )}
    </li>
  );
}

export interface ChecksPanelProps {
  lint: LintData | null;
  lintError: string | null;
  sync: SyncData;
  onOpenSkill: (name: string) => void;
  onReload: () => void;
  onChanged: () => void;
}

/** Four counters; each opens the list under it. Lint is grouped by what Claude does with the
 * skill, not by the name of the rule. */
export default function ChecksPanel({
  lint,
  lintError,
  sync,
  onOpenSkill,
  onReload,
  onChanged,
}: ChecksPanelProps) {
  const [tab, setTab] = useState<Tab>("lint");
  const [chmod, setChmod] = useState<string | null>(null);
  const [moving, setMoving] = useState<{ skill: string; link: string } | null>(null);
  const [resolve, setResolve] = useState<boolean | null>(null);
  const [showAdvice, setShowAdvice] = useState(false);
  const audit = useLoad(async () => {
    try {
      return await auditSkills();
    } catch {
      return null;
    }
  }, "audit");
  const drift = sync.plan.changes.filter((c) => c.op === "edited" || c.op === "differs");
  const collisions = sync.collisions;
  const groups = lintGroups(lint);
  const findings = audit.data?.findings.length ?? 0;

  return (
    <section aria-label="Checks" className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 className="text-meta font-semibold tracking-wide text-muted-foreground uppercase">
          Checks
        </h2>
        <Button
          variant="ghost"
          size="sm"
          onClick={() => {
            onReload();
            audit.reload();
          }}
        >
          <RefreshCw /> Run now
        </Button>
      </div>
      <div
        role="tablist"
        aria-label="Checks"
        className="grid grid-cols-2 gap-2 @xl:grid-cols-4"
      >
        <Tile
          id="lint"
          active={tab === "lint"}
          title="Lint"
          note="Skills Claude would hide or cut"
          onSelect={setTab}
          badges={
            lintError ? (
              <Badge variant="warning">Could not run</Badge>
            ) : lint && lint.errors + lint.warnings > 0 ? (
              <>
                {lint.errors > 0 && (
                  <Badge variant="destructive">{plural(lint.errors, "error")}</Badge>
                )}
                {lint.warnings > 0 && (
                  <Badge variant="warning">{plural(lint.warnings, "warning")}</Badge>
                )}
              </>
            ) : (
              <Badge variant="success">None</Badge>
            )
          }
        />
        <Tile
          id="audit"
          active={tab === "audit"}
          title="Audit"
          note="Risky instructions"
          onSelect={setTab}
          badges={
            audit.data === null && audit.error ? (
              <Badge variant="warning">Could not run</Badge>
            ) : findings > 0 ? (
              <Badge variant="warning">{plural(findings, "finding")}</Badge>
            ) : (
              <Badge variant="success">None</Badge>
            )
          }
        />
        <Tile
          id="collisions"
          active={tab === "collisions"}
          title="Collisions"
          note="Same name in two places"
          onSelect={setTab}
          badges={
            collisions.length > 0 ? (
              <Badge variant="warning">{collisions.length}</Badge>
            ) : (
              <Badge variant="success">None</Badge>
            )
          }
        />
        <Tile
          id="drift"
          active={tab === "drift"}
          title="Drift"
          note="Edited outside the library"
          onSelect={setTab}
          badges={
            drift.length > 0 ? (
              <Badge variant="warning">{drift.length}</Badge>
            ) : (
              <Badge variant="success">None</Badge>
            )
          }
        />
      </div>
      <div
        id="checks-list"
        role="tabpanel"
        aria-labelledby={`check-${tab}`}
        className="overflow-hidden rounded-xl border bg-card"
      >
        {tab === "lint" && (
          <>
            <p className="border-b px-4 py-2 text-meta text-muted-foreground">
              Checking for Claude Code. Rules for clients that are off are not shown.
            </p>
            {lintError && (
              <p role="alert" className="px-4 py-3 text-meta text-destructive">
                Could not run lint: {lintError}
              </p>
            )}
            {!lintError && groups.length === 0 && (
              <p className="px-4 py-3 text-meta">
                No findings. Every skill reads cleanly.
              </p>
            )}
            {groups.map((group) => (
              <div key={group.id} aria-label={group.title}>
                <div className="flex items-center justify-between border-b bg-muted/50 px-4 py-1.5 text-xs font-semibold tracking-wide text-muted-foreground uppercase">
                  <span>
                    {group.title}{" "}
                    <span className="ml-1 font-normal normal-case">
                      {group.rows.length + (group.notCopied ? 1 : 0)}
                    </span>
                  </span>
                  {group.id === "hides" && (
                    <span className="font-normal normal-case">Held back from sync</span>
                  )}
                  {group.id === "advice" && (
                    <button
                      type="button"
                      className="font-normal text-primary normal-case"
                      onClick={() => setShowAdvice((v) => !v)}
                    >
                      {showAdvice ? "Collapse" : "Show"}
                    </button>
                  )}
                </div>
                {(group.id !== "advice" || showAdvice) && (
                  <ul className="divide-y">
                    {group.rows.map((row, index) => (
                      <LintRowView
                        key={`${row.name}-${row.code}-${index}`}
                        row={row}
                        onOpenSkill={onOpenSkill}
                        onChmod={setChmod}
                        onMoveLink={(skill, link) => setMoving({ skill, link })}
                      />
                    ))}
                    {group.notCopied && (
                      <li className="flex items-start gap-3 px-4 py-2.5">
                        <Info
                          aria-hidden
                          className="mt-0.5 size-4 shrink-0 text-muted-foreground"
                        />
                        <div>
                          <p className="text-sm font-medium">
                            {plural(group.notCopied.files, "file")} in{" "}
                            {plural(group.notCopied.skills, "skill")} are not copied
                          </p>
                          <p className="text-meta text-muted-foreground">
                            Caches and system files from the deny-list. Informational;
                            each skill lists them under Files.
                          </p>
                        </div>
                      </li>
                    )}
                  </ul>
                )}
              </div>
            ))}
          </>
        )}
        {tab === "audit" && (
          <div className="px-4 py-3 text-meta">
            {audit.data === null ? (
              <p role="alert" className="text-destructive">
                Could not run the audit{audit.error ? `: ${audit.error}` : "."}
              </p>
            ) : findings === 0 ? (
              <p>
                Audit: no risky instructions in{" "}
                {plural(audit.data.skillCount, "library item")}. It looks for patterns
                that try to steer the model against you, such as hiding actions or
                ignoring earlier instructions.
              </p>
            ) : (
              <ul className="flex flex-col gap-1.5">
                {audit.data.findings.map((f, index) => (
                  <li key={index}>
                    <span className="font-medium capitalize">{f.severity}</span>
                    {f.line ? ` · line ${f.line}` : ""} · {f.skill}: {f.message}
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}
        {tab === "collisions" && (
          <div className="flex flex-col gap-2 px-4 py-3 text-meta">
            {collisions.length === 0 && (
              <p>No skill has the same name as a hand-written file.</p>
            )}
            {collisions.map((c) => (
              <p key={c.collisionPath}>
                <span className="font-medium">{c.skill}</span> exists twice:{" "}
                <code className="font-mono text-xs">{c.collisionPath}</code> is a
                hand-written file with the same name.
              </p>
            ))}
            {collisions.length > 0 && (
              <div className="flex gap-2">
                <Button variant="outline" size="xs" onClick={() => setResolve(true)}>
                  Replace the command (backup kept)
                </Button>
                <Button variant="outline" size="xs" onClick={() => setResolve(false)}>
                  Keep both
                </Button>
              </div>
            )}
          </div>
        )}
        {tab === "drift" && (
          <div className="flex flex-col gap-1.5 px-4 py-3 text-meta">
            {drift.length === 0 && (
              <p>Nothing was edited outside the library since the last sync.</p>
            )}
            {drift.map((c) => (
              <p key={c.path}>
                <span className="font-medium">{c.skill}</span>{" "}
                <code className="font-mono text-xs">{c.path}</code> was{" "}
                {c.op === "edited"
                  ? "edited after the last sync"
                  : "copied by hand and differs"}
                . The next sync replaces it with the library copy and keeps yours as a
                backup.
              </p>
            ))}
          </div>
        )}
      </div>
      {chmod && (
        <ChmodDialog skill={chmod} onClose={() => setChmod(null)} onDone={onChanged} />
      )}
      {moving && (
        <MoveLinkDialog
          skill={moving.skill}
          link={moving.link}
          onClose={() => setMoving(null)}
          onDone={onChanged}
        />
      )}
      {resolve !== null && (
        <ResolveDialog
          migrate={resolve}
          onClose={() => setResolve(null)}
          onDone={onChanged}
        />
      )}
    </section>
  );
}
