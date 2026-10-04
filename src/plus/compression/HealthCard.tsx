import { useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { Input } from "@/components/ui/input";
import type { CompressionDoctorData, CompressionVerifyData } from "../types/compression";
import { AsyncView, JobProgress, useCtlJob, type CtlQuery } from "../ui";
import { Field } from "./forms";
import { ctl, flagArgs } from "./model";

type Check = CompressionDoctorData["checks"][number];

function Checks({ checks }: { checks: Check[] }) {
  return (
    <ul className="divide-y rounded-lg border">
      {checks.map((check) => (
        <li key={check.name} className="flex flex-wrap items-center gap-3 px-3 py-2">
          <b className="w-16 text-sm capitalize">{check.name}</b>
          <Badge variant={check.ok ? "success" : "destructive"}>
            {check.ok ? "ok" : "failed"}
          </Badge>
          <span className="min-w-0 flex-1 text-sm break-words text-muted-foreground">
            {check.detail}
          </span>
        </li>
      ))}
    </ul>
  );
}

export function HealthCard({ query }: { query: CtlQuery<CompressionDoctorData> }) {
  return (
    <section aria-label="Health checks" className="flex flex-col gap-2">
      <h3 className="text-sm font-semibold">Health checks</h3>
      <AsyncView
        query={query}
        errorTitle="Couldn't run the health checks"
        context="compression doctor"
      >
        {(data) => (
          <>
            <Checks checks={data.checks} />
            {data.migrated.length > 0 && (
              <Callout variant="info">
                Moved from an older layout: {data.migrated.map(String).join(", ")}
              </Callout>
            )}
            <p className="text-xs text-muted-foreground">
              Doctor only reads. Verify reads your local Claude transcripts and makes no
              model requests.
            </p>
          </>
        )}
      </AsyncView>
    </section>
  );
}

type Bucket = NonNullable<CompressionVerifyData["buckets"]>["plain"];

const BUCKETS = [
  ["proxied", "Through the proxy"],
  ["plain", "Plain"],
  ["unattributed", "Unattributed"],
] as const;

const NUMBER = new Intl.NumberFormat("en-US");

function ratio(value: number | null) {
  return value === null ? "n/a" : value.toFixed(2);
}

function BucketRow({ label, bucket }: { label: string; bucket: Bucket }) {
  return (
    <tr className="border-t">
      <th scope="row" className="py-1.5 pr-3 text-left font-medium">
        {label}
      </th>
      <td className="px-2 text-right">{bucket.sessions}</td>
      <td className="px-2 text-right">{bucket.turns}</td>
      <td className="px-2 text-right">{NUMBER.format(bucket.cacheRead)}</td>
      <td className="px-2 text-right">{NUMBER.format(bucket.cacheCreate)}</td>
      <td className="px-2 text-right">{ratio(bucket.readRatio)}</td>
    </tr>
  );
}

export function VerifyResult({ data }: { data: CompressionVerifyData }) {
  const { buckets, transcripts } = data;
  return (
    <div className="flex flex-col gap-3 text-sm">
      <Checks checks={data.checks} />
      {buckets ? (
        <table aria-label="Cache behaviour by launch" className="w-full text-xs">
          <thead>
            <tr className="text-muted-foreground">
              <th scope="col" className="text-left font-normal">
                Launch
              </th>
              <th scope="col" className="px-2 text-right font-normal">
                Sessions
              </th>
              <th scope="col" className="px-2 text-right font-normal">
                Turns
              </th>
              <th scope="col" className="px-2 text-right font-normal">
                Cache read
              </th>
              <th scope="col" className="px-2 text-right font-normal">
                Cache write
              </th>
              <th scope="col" className="px-2 text-right font-normal">
                Read ratio
              </th>
            </tr>
          </thead>
          <tbody>
            {BUCKETS.map(([key, label]) => (
              <BucketRow key={key} label={label} bucket={buckets[key]} />
            ))}
          </tbody>
        </table>
      ) : (
        <Callout variant="warning">
          No transcripts to measure under {transcripts.root}. Start Claude with
          compression on, then verify again.
        </Callout>
      )}
      {data.verdict != null && (
        <p>
          Verdict:{" "}
          <b>
            {typeof data.verdict === "string"
              ? data.verdict
              : JSON.stringify(data.verdict)}
          </b>
        </p>
      )}
      <p className="text-xs text-muted-foreground">
        {transcripts.count} transcript{transcripts.count === 1 ? "" : "s"} read from{" "}
        {transcripts.root}
      </p>
    </div>
  );
}

/** Verify: reads local transcripts and checks the engine. It exits 1 with its data when a
 * check fails, so a failed envelope that carries data still shows the checks. */
export function VerifyDialog({ onClose }: { onClose: () => void }) {
  const job = useCtlJob();
  const [transcripts, setTranscripts] = useState("");
  const [minTurns, setMinTurns] = useState("");
  const [limit, setLimit] = useState("");
  const [byPin, setByPin] = useState(false);
  const numbers = [minTurns, limit].every((n) => n === "" || /^\d+$/.test(n));
  const { state } = job;
  const started = state.phase !== "idle";
  const envelope = state.result?.envelope;
  const failedWithData =
    state.phase === "done" && envelope && !envelope.ok && envelope.data != null;
  const run = () =>
    void job.start(
      ctl(
        "verify",
        ...flagArgs([
          { flag: "--transcripts", value: transcripts },
          { flag: "--min-turns", value: minTurns },
          { flag: "--limit", value: limit },
        ]),
        ...(byPin ? ["--by-pin"] : []),
      ),
    );
  return (
    <Dialog open onOpenChange={(open) => !open && state.phase !== "running" && onClose()}>
      <DialogContent className="sm:max-w-2xl" aria-describedby="verify-note">
        <DialogHeader>
          <DialogTitle>Verify compression</DialogTitle>
        </DialogHeader>
        <p id="verify-note" className="text-sm text-muted-foreground">
          Reads local transcripts, makes no model requests. It compares how Claude's
          prompt cache behaved with and without the proxy.
        </p>
        {!started && (
          <form
            className="flex flex-col gap-3"
            onSubmit={(event) => {
              event.preventDefault();
              if (numbers) run();
            }}
          >
            <Field
              label="Transcripts folder"
              hint="Optional. Defaults to Claude's own projects folder."
            >
              {(id, hint) => (
                <Input
                  id={id}
                  aria-describedby={hint}
                  value={transcripts}
                  onChange={(e) => setTranscripts(e.target.value)}
                />
              )}
            </Field>
            <div className="grid gap-3 sm:grid-cols-2">
              <Field label="Minimum turns" hint="Skip sessions shorter than this.">
                {(id, hint) => (
                  <Input
                    id={id}
                    aria-describedby={hint}
                    inputMode="numeric"
                    aria-invalid={!/^\d*$/.test(minTurns)}
                    value={minTurns}
                    onChange={(e) => setMinTurns(e.target.value)}
                  />
                )}
              </Field>
              <Field label="Newest sessions" hint="Look at this many at most.">
                {(id, hint) => (
                  <Input
                    id={id}
                    aria-describedby={hint}
                    inputMode="numeric"
                    aria-invalid={!/^\d*$/.test(limit)}
                    value={limit}
                    onChange={(e) => setLimit(e.target.value)}
                  />
                )}
              </Field>
            </div>
            <label className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={byPin}
                onChange={(e) => setByPin(e.target.checked)}
              />
              Split the result by engine pin
            </label>
            <DialogFooter>
              <Button type="button" variant="ghost" onClick={onClose}>
                Cancel
              </Button>
              <Button type="submit" disabled={!numbers}>
                Run verify
              </Button>
            </DialogFooter>
          </form>
        )}
        {started && (
          <>
            <JobProgress
              state={state}
              onCancel={() => void job.cancel()}
              title="Verifying"
              renderResult={(data) => (
                <VerifyResult data={data as CompressionVerifyData} />
              )}
            />
            {failedWithData && (
              <VerifyResult data={envelope.data as CompressionVerifyData} />
            )}
            {state.phase === "done" && (
              <DialogFooter>
                <Button variant="ghost" onClick={() => job.reset()}>
                  Run again
                </Button>
                <Button onClick={onClose}>Close</Button>
              </DialogFooter>
            )}
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
