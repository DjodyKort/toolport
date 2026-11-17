import { ArrowLeft, FileSearch, TriangleAlert } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { SkeletonRows } from "@/components/ui/skeleton";
import { StatusCard } from "@/components/ui/status-card";
import { useLoad } from "../servers/useLoad";
import { measureCompression } from "./commands";
import type {
  HooksSection,
  MeasureReport,
  OutcomeSection,
  ReworkSection,
  SourcesSection,
} from "./data";
import { HOOK_INFO, count, dayRange, intervalText, pct, windowText } from "./model";

const SHADES = [
  "bg-foreground/80",
  "bg-foreground/60",
  "bg-foreground/45",
  "bg-foreground/30",
  "bg-foreground/15",
];

function Card({
  title,
  lead,
  children,
  population,
  cannotTell,
}: {
  title: string;
  lead: string;
  children: React.ReactNode;
  population: string;
  cannotTell: string;
}) {
  return (
    <section
      aria-label={title}
      className="flex min-w-0 flex-col gap-3 rounded-xl border bg-card p-4"
    >
      <div className="flex flex-col gap-0.5">
        <h3 className="text-sm font-semibold">{title}</h3>
        <p className="text-meta text-muted-foreground">{lead}</p>
      </div>
      {children}
      <dl className="flex flex-col gap-1 border-t pt-2 text-meta">
        <div>
          <dt className="inline font-medium">Population: </dt>
          <dd className="inline">{population}</dd>
        </div>
        {cannotTell && (
          <div>
            <dt className="inline font-medium">Cannot tell you: </dt>
            <dd className="inline">{cannotTell}</dd>
          </div>
        )}
      </dl>
    </section>
  );
}

const Empty = ({ children }: { children: React.ReactNode }) => (
  <p className="text-sm text-muted-foreground" data-testid="card-empty">
    {children}
  </p>
);

function Sources({ section }: { section: SourcesSection }) {
  const { rows, population, bashCeiling } = section;
  return (
    <Card
      title="1. Where context tokens come from"
      lead="Turn-weighted share: estimated tokens times the turns they stayed in context. No counterfactual."
      population={
        section.status === "no_transcripts"
          ? "no transcripts were found"
          : `${count(population.sessions)} sessions on this machine, ${dayRange(population.from, population.to)}, read from Claude Code transcripts.`
      }
      cannotTell={section.cannotTell}
    >
      {section.status !== "data" ? (
        <Empty>
          {section.status === "no_transcripts"
            ? "No transcripts yet."
            : "The transcripts hold no tool results."}
        </Empty>
      ) : (
        <>
          <div
            className="flex h-3 w-full overflow-hidden rounded-full bg-muted"
            role="img"
            aria-label={rows.map((r) => `${r.kind} ${pct(r.share)}`).join(", ")}
          >
            {rows.map((row, i) => (
              <div
                key={row.kind}
                className={SHADES[i] ?? SHADES[4]}
                style={{ width: `${row.share * 100}%` }}
              />
            ))}
          </div>
          <ul className="grid grid-cols-2 gap-x-4 gap-y-1 text-sm">
            {rows.map((row, i) => (
              <li key={row.kind} className="flex items-center gap-2">
                <span
                  aria-hidden
                  className={`size-2.5 shrink-0 rounded-sm ${SHADES[i] ?? SHADES[4]}`}
                />
                <span className="min-w-0 flex-1">{row.kind}</span>
                <span className="tabular-nums">{pct(row.share)}</span>
              </li>
            ))}
          </ul>
          {bashCeiling !== null && (
            <p className="text-sm" data-testid="bash-ceiling">
              <strong>
                At most {pct(bashCeiling)} of context-turns came from Bash output.
              </strong>{" "}
              That is the ceiling for what rtk could save: an opportunity bound, not a
              saving.
            </p>
          )}
          <p className="text-meta text-muted-foreground">
            Basis: estimate. {section.method}.
          </p>
        </>
      )}
    </Card>
  );
}

function Hooks({ section }: { section: HooksSection }) {
  const { population } = section;
  return (
    <Card
      title="2. What the hooks did"
      lead="Counted by Toolport at the hook. Basis: mechanical."
      population={
        section.status === "off"
          ? "no hook is written by Toolport"
          : `${population.hooks} ${population.hooks === 1 ? "hook" : "hooks"}, ${dayRange(population.from, population.to)}, ${count(population.calls)} calls counted by the hook wrapper (a small per-call cost, shown as latency).`
      }
      cannotTell={section.cannotTell}
    >
      {section.status === "off" ? (
        <Empty>
          Counters need the hook wrapper. Adopt the hooks on the previous page and
          counting starts with the next hook call.
        </Empty>
      ) : section.status === "no_calls" ? (
        <Empty>
          The hooks are wrapped, but no call was counted yet. Counting starts with the
          next Claude Code session.
        </Empty>
      ) : (
        <>
          <ul className="flex flex-col gap-2">
            {section.providers
              .filter((p) => p.calls > 0)
              .map((p) => (
                <li key={p.provider} className="flex flex-col text-sm">
                  <span className="font-medium">
                    {p.provider}{" "}
                    <span className="font-normal text-muted-foreground">
                      PreToolUse {HOOK_INFO[p.provider].matcher}
                    </span>
                  </span>
                  <span>
                    {count(p.answered)} answered of {count(p.calls)} calls
                    {p.failed > 0 && ` · ${count(p.failed)} failed`}
                    {p.p50Ms !== null && ` · p50 ${p.p50Ms} ms`}
                    {p.p95Ms !== null && ` · p95 ${p.p95Ms} ms`}
                  </span>
                </li>
              ))}
          </ul>
          <p className="text-meta" data-testid="output-bytes">
            Bytes saved: not counted. {section.outputBytes.reason}
          </p>
          {section.providerReported.length > 0 && (
            <div className="text-meta">
              <p className="font-medium">
                Provider-reported, never summed with the above
              </p>
              <ul>
                {section.providerReported.map((r) => (
                  <li key={r.provider}>
                    {r.provider}: {count(r.tokensBefore)} to {count(r.tokensAfter)} tokens
                    in {count(r.entries)} entries
                  </li>
                ))}
              </ul>
            </div>
          )}
        </>
      )}
    </Card>
  );
}

function Outcome({ section }: { section: OutcomeSection }) {
  const { verdict, population } = section;
  return (
    <Card
      title="3. Billed outcome, by day"
      lead="Only billed usage in transcripts counts as evidence. Days are grouped by what was active (epochs)."
      population={
        section.status === "no_sessions"
          ? `no session has ${population.minTurns} or more turns`
          : `Sessions with at least ${population.minTurns} turns; cost units are relative (hooks-on median = 100).`
      }
      cannotTell={section.cannotTell}
    >
      {section.status === "no_sessions" ? (
        <Empty>Nothing to compare yet.</Empty>
      ) : (
        <>
          <p className="text-meta text-muted-foreground" data-testid="timeline">
            {section.timeline
              .map((p) => `${p.label} ${intervalText({ from: p.from, to: p.to })}`)
              .join(" · ")}
          </p>
          <ul className="flex flex-col gap-1 text-sm">
            {section.groups.map((g) => (
              <li key={g.state}>
                <span className="font-medium">{g.label}</span>{" "}
                <span data-testid={`window-${g.state}`}>{windowText(g)}</span>
              </li>
            ))}
          </ul>
          <div className="flex flex-col gap-1 rounded-md bg-muted p-2.5 text-sm">
            <span className="font-medium">
              <Badge variant={verdict.status === "refused" ? "warning" : "off"}>
                {verdict.status === "compared" ? "Compared" : "No verdict"}
              </Badge>
            </span>
            <span data-testid="verdict">{verdict.reason}</span>
          </div>
        </>
      )}
    </Card>
  );
}

function Rework({ section }: { section: ReworkSection }) {
  const thin = section.groups.some(
    (g) => g.state === "paused" && g.sessions > 0 && g.sessions < 8,
  );
  const figure = (value: number | null) => (value === null ? "-" : value.toFixed(1));
  return (
    <Card
      title="4. Rework check"
      lead="A compressor that hides detail makes the model run things twice."
      population={
        section.status === "no_sessions"
          ? "no session has enough turns"
          : "Same sessions as 3."
      }
      cannotTell={section.cannotTell}
    >
      {section.status === "no_sessions" ? (
        <Empty>Nothing to check yet.</Empty>
      ) : (
        <>
          <table className="w-full text-left text-sm">
            <caption className="sr-only">Repeats per session by epoch</caption>
            <thead className="text-meta text-muted-foreground">
              <tr>
                <th className="py-1 font-medium">Epoch</th>
                <th className="py-1 text-right font-medium">Repeated commands</th>
                <th className="py-1 text-right font-medium">Files read again</th>
                <th className="py-1 text-right font-medium">Sessions</th>
              </tr>
            </thead>
            <tbody>
              {section.groups.map((g) => (
                <tr key={g.state} className="border-t">
                  <td className="py-1.5">{g.label}</td>
                  <td className="py-1.5 text-right tabular-nums">
                    {figure(g.repeatedCommands)}
                  </td>
                  <td className="py-1.5 text-right tabular-nums">
                    {figure(g.repeatedReads)}
                  </td>
                  <td className="py-1.5 text-right tabular-nums">{g.sessions}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {thin && (
            <p className="text-meta text-muted-foreground">
              The paused window holds few sessions: treat both figures as hints.
            </p>
          )}
        </>
      )}
    </Card>
  );
}

function Report({ report }: { report: MeasureReport }) {
  const noTranscripts = report.sources.status === "no_transcripts";
  return (
    <div className="flex flex-col gap-4">
      {noTranscripts && (
        <StatusCard
          variant="info"
          icon={<FileSearch />}
          title="No transcripts yet"
          detail={
            <>
              Questions 1, 3 and 4 read Claude Code transcripts from{" "}
              <code>{report.transcripts.root}</code>. None were found. Question 2 starts
              counting after the next hook call. Each card degrades alone; one missing
              source never blanks the page.
            </>
          }
        />
      )}
      <div className="grid gap-4 lg:grid-cols-2">
        <Sources section={report.sources} />
        <Hooks section={report.hooks} />
        <Outcome section={report.outcome} />
        <Rework section={report.rework} />
      </div>
      <p className="text-meta text-muted-foreground" data-testid="upstream-note">
        <strong>Upstream&apos;s number stays separate.</strong> {report.upstream.note}
      </p>
    </div>
  );
}

/** The measuring of compression, one level down from Hooks & compression: four honest
 * questions, each with the population it was computed over and what it cannot tell. */
export default function MeasurementsPage({ onBack }: { onBack: () => void }) {
  const report = useLoad(measureCompression, "measure");
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center gap-3">
        <Button variant="ghost" size="sm" onClick={onBack}>
          <ArrowLeft data-icon="inline-start" />
          Back to Hooks and compression
        </Button>
        <h2 className="text-base font-semibold">Compression measurements</h2>
        <Button size="sm" variant="outline" className="ml-auto" onClick={report.reload}>
          Recompute
        </Button>
      </div>
      <p className="text-meta text-muted-foreground">
        No headline number: every figure carries its population and one line on what it
        cannot tell you, two kinds of figure are never added together, and every input is
        a file on this machine.
      </p>
      {report.error && !report.data && (
        <StatusCard
          role="alert"
          variant="danger"
          icon={<TriangleAlert />}
          title="The measurements could not be computed"
          detail={report.error}
          actions={
            <Button size="sm" variant="outline" onClick={report.reload}>
              Try again
            </Button>
          }
        />
      )}
      {!report.data && !report.error && (
        <SkeletonRows label="Reading the transcripts…" rows={4} />
      )}
      {report.data && <Report report={report.data} />}
    </div>
  );
}
