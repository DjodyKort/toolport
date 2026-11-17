import { useRef, useState } from "react";
import { ArrowLeft, Gauge } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { StatusCard } from "@/components/ui/status-card";
import { toastError } from "@/lib/toast";
import { errorText } from "../settings/useCtl";
import { useLoad } from "../servers/useLoad";
import { listBundles, measureFolder, type MeasureOptions } from "./commands";
import type { MeasureData } from "./data";
import { plural, tokens } from "./model";

type Variant = Pick<MeasureOptions, "without" | "bundle" | "force">;

function Report({ report }: { report: MeasureData }) {
  const verified = report.verified;
  return (
    <section
      aria-label="Measured result"
      className="flex flex-col gap-3 rounded-xl border bg-card p-4"
    >
      <div className="flex flex-wrap items-center gap-3">
        <p className="text-2xl font-semibold tabular-nums">
          {tokens(report.total ?? 0)} tokens
        </p>
        <Badge variant={report.stale ? "warning" : "success"}>
          {report.stale ? "Measured, stale" : "Measured"}
        </Badge>
        {report.cached && <Badge variant="outline">cached, cost nothing</Badge>}
      </div>
      <p className="text-meta text-muted-foreground">
        Claude Code {report.claudeCodeVersion} · {report.model} · {report.windowPercent}%
        of a {tokens(report.contextWindow ?? 0)}-token window
      </p>
      {report.itemised && report.notItemised && (
        <p className="text-meta">
          Itemised ≈ {tokens(report.itemised.tokens)} (estimate,{" "}
          {plural(report.itemised.rows, "row")}) · Not itemised{" "}
          {tokens(report.notItemised.tokens)} ({report.notItemised.percent}%):{" "}
          {report.notItemised.includes}.
        </p>
      )}
      {verified && (
        <p className="text-meta" data-testid="verified">
          Claude Code's own log: {verified.loaded} loaded, {verified.absent} absent,{" "}
          {plural(verified.mismatches.length, "disagreement")}
          {verified.unknown.length > 0 &&
            `, ${plural(verified.unknown.length, "file")} without a row`}
          .
        </p>
      )}
      {report.variants && report.variants.length > 0 && (
        <table className="w-full text-left text-sm">
          <caption className="sr-only">What each change saves, measured</caption>
          <thead className="text-meta text-muted-foreground">
            <tr>
              <th className="py-1 font-medium">Variant</th>
              <th className="py-1 text-right font-medium">Total</th>
              <th className="py-1 text-right font-medium">Change</th>
            </tr>
          </thead>
          <tbody>
            {report.variants.map((v) => (
              <tr key={v.label} className="border-t">
                <td className="py-1.5">{v.label}</td>
                <td className="py-1.5 text-right tabular-nums">{tokens(v.total)}</td>
                <td className="py-1.5 text-right tabular-nums">
                  {v.deltaTokens > 0 ? "+" : ""}
                  {tokens(v.deltaTokens)} ({v.deltaPercent})
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {report.notes?.map((note) => (
        <p key={note} className="text-meta text-muted-foreground">
          {note}
        </p>
      ))}
    </section>
  );
}

/** Measuring, one level down from This folder: the plan and its cost first, the run with Cancel,
 * the result, and Measure without this. A cached answer is served and costs nothing. */
export default function MeasurePage({
  folder,
  onBack,
}: {
  folder: string;
  onBack: () => void;
}) {
  const [variant, setVariant] = useState<Variant>({});
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<MeasureData | null>(null);
  const abort = useRef<AbortController | null>(null);
  const [skill, setSkill] = useState("");
  const answer = useLoad(
    () => measureFolder(folder, variant),
    JSON.stringify([folder, variant]),
  );
  const bundles = useLoad(listBundles, folder);
  const shown = result ?? (answer.data?.ran ? answer.data : null);
  const plan = result ? null : answer.data?.ran === false ? answer.data.plan : null;
  const known = shown?.plugins ?? [];

  async function start() {
    const controller = new AbortController();
    abort.current = controller;
    setRunning(true);
    try {
      const data = await measureFolder(folder, {
        ...variant,
        start: true,
        signal: controller.signal,
      });
      setResult(data);
    } catch (error) {
      if (!controller.signal.aborted) {
        toastError("The measurement failed", { description: errorText(error) });
      }
    } finally {
      abort.current = null;
      setRunning(false);
    }
  }

  function choose(next: Variant) {
    setResult(null);
    setVariant(next);
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center gap-3">
        <Button variant="ghost" size="sm" onClick={onBack}>
          <ArrowLeft data-icon="inline-start" />
          Back to this folder
        </Button>
        <h2 className="text-base font-semibold">Measure for real</h2>
      </div>

      {answer.error && !running && (
        <StatusCard
          role="alert"
          variant="danger"
          icon={<Gauge />}
          title="The measurement could not be prepared"
          detail={`${answer.error}. The last good number stays on the page.`}
          actions={
            <Button size="sm" variant="outline" onClick={answer.reload}>
              Try again
            </Button>
          }
        />
      )}

      {running && (
        <StatusCard
          variant="info"
          icon={<Gauge />}
          title="Measuring…"
          detail="One request is running, up to three minutes. The estimate stays, and the page stays usable."
          actions={
            <Button size="sm" variant="outline" onClick={() => abort.current?.abort()}>
              Cancel
            </Button>
          }
        />
      )}

      {plan && !running && (
        <section
          aria-label="Measure plan"
          className="flex flex-col gap-2 rounded-xl border bg-card p-4"
        >
          <h3 className="text-sm font-semibold">
            {plural(plan.requests, "request")} on {plan.model}
          </h3>
          <ul className="flex list-disc flex-col gap-1 pl-5 text-meta">
            <li>Uses {plan.cost}</li>
            <li>Sends: {plan.sends}</li>
            <li>Writes: {plan.writes}</li>
            <li>Measures: {plan.variants.join(", ")}</li>
          </ul>
          <div>
            <Button onClick={() => void start()}>
              Start {plural(plan.requests, "request")}
            </Button>
          </div>
        </section>
      )}

      {shown && <Report report={shown} />}

      <section
        aria-label="Measure without this"
        className="flex flex-col gap-3 rounded-xl border bg-card p-4"
      >
        <h3 className="text-sm font-semibold">Measure without this</h3>
        <p className="text-meta text-muted-foreground">
          One more request per change, with a count and a confirmation. It shows what a
          plugin, a skill or a whole bundle really saves.
        </p>
        <div className="flex flex-wrap gap-2">
          {known.map((id) => (
            <Button
              key={id}
              size="sm"
              variant="outline"
              onClick={() => choose({ without: [`plugin:${id}`] })}
            >
              Without {id}
            </Button>
          ))}
          {(bundles.data?.bundles ?? []).map((b) => (
            <Button
              key={b.name}
              size="sm"
              variant="outline"
              onClick={() => choose({ bundle: b.name })}
            >
              With bundle {b.name}
            </Button>
          ))}
          {variant.without || variant.bundle ? (
            <Button size="sm" variant="ghost" onClick={() => choose({})}>
              As is
            </Button>
          ) : null}
        </div>
        <form
          className="flex gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            if (skill.trim()) choose({ without: [`skill:${skill.trim()}`] });
          }}
        >
          <Input
            aria-label="Skill name"
            placeholder="skill name or glob"
            value={skill}
            onChange={(e) => setSkill(e.target.value)}
          />
          <Button type="submit" variant="outline" disabled={!skill.trim()}>
            Without this skill
          </Button>
        </form>
      </section>
    </div>
  );
}
