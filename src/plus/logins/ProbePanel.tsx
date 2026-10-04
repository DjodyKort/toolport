import { Button } from "@/components/ui/button";
import { JobProgress } from "../ui";
import type { AuthProbeData } from "../types/auth";
import type { ProbeControl } from "./useProbe";

function failuresOf(data: AuthProbeData): Array<{ server: string; error: string }> {
  return (data.failures as Array<Partial<{ server: string; error: string }>>).flatMap(
    (failure) =>
      typeof failure.server === "string"
        ? [{ server: failure.server, error: String(failure.error ?? "failed") }]
        : [],
  );
}

function ProbeResult({ data }: { data: AuthProbeData }) {
  const ran = data.probes.filter((probe) => probe.ran).length;
  const skipped = data.probes.length - ran;
  const failures = failuresOf(data);
  return (
    <div className="flex flex-col gap-1.5 text-sm">
      <p>
        Probed {ran}, skipped {skipped} (a recent result was cached), failed{" "}
        {failures.length}.
      </p>
      {failures.length > 0 && (
        <ul aria-label="Probe failures" className="list-disc pl-4">
          {failures.map((failure) => (
            <li key={failure.server}>
              <b className="font-medium">{failure.server}</b>: {failure.error}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/** The progress and the result of the running or last probe. */
export function ProbePanel({ probe }: { probe: ProbeControl }) {
  const { job, target } = probe;
  if (job.state.phase === "idle") return null;
  const done = job.state.phase === "done";
  return (
    <section aria-label="Probe result" className="flex flex-col gap-2">
      <JobProgress
        state={job.state}
        onCancel={() => void job.cancel()}
        title={target && target !== "all" ? `Probing ${target}` : "Probing every server"}
        doneLabel="Probe finished"
        renderResult={(data) => <ProbeResult data={data as AuthProbeData} />}
      />
      {done && (
        <div>
          <Button size="sm" variant="outline" onClick={probe.dismiss}>
            Dismiss
          </Button>
        </div>
      )}
    </section>
  );
}
