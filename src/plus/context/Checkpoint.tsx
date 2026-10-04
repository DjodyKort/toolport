import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { ctlData } from "../bridge/ctl";
import type { ContextCheckpointStatusData } from "../types/context";
import { ErrorState } from "../ui";
import { formatTokens, type ProfileRow } from "./model";
import { Field, Section, SELECT_CLASS } from "./parts";

type Run =
  | { phase: "idle" }
  | { phase: "running" }
  | { phase: "error"; error: unknown }
  | { phase: "done"; data: ContextCheckpointStatusData };

export function Gauge({ data }: { data: ContextCheckpointStatusData }) {
  const pct = (n: number) => `${Math.min(100, Math.max(0, (n / data.window) * 100))}%`;
  return (
    <div className="flex flex-col gap-2">
      <div
        role="meter"
        aria-label="Context used"
        aria-valuemin={0}
        aria-valuemax={data.window}
        aria-valuenow={data.used_tokens}
        className="relative h-3 overflow-hidden rounded-full bg-muted"
      >
        <div
          className={`h-full rounded-full ${data.at_checkpoint ? "bg-warning" : "bg-primary"}`}
          style={{ width: pct(data.used_tokens) }}
        />
        <div
          className="absolute inset-y-0 w-0.5 bg-foreground"
          style={{ left: pct(data.checkpoint_point) }}
        />
      </div>
      <p className="text-sm">
        {formatTokens(data.used_tokens)} of {formatTokens(data.window)} tokens used.{" "}
        {data.at_checkpoint
          ? "The checkpoint point is reached: write the handoff now."
          : `${formatTokens(data.remaining_to_checkpoint)} to the checkpoint at ${formatTokens(data.checkpoint_point)}; ${formatTokens(data.remaining_to_compact)} to compaction.`}
      </p>
      <p className="text-xs text-muted-foreground">Window from {data.window_source}.</p>
    </div>
  );
}

/** The checkpoint gauge. Claude Code's statusline JSON goes to the command's stdin through the
 * bridge; it stays in this component and is never shown back. */
export function CheckpointSection({ profiles }: { profiles: ProfileRow[] | undefined }) {
  const [json, setJson] = useState("");
  const [profile, setProfile] = useState("");
  const [window, setWindow] = useState("");
  const [at, setAt] = useState("");
  const [run, setRun] = useState<Run>({ phase: "idle" });
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  const number = (value: string) =>
    value.trim() === "" || /^[1-9]\d*$/.test(value.trim());
  const valid = json.trim() !== "" && number(window) && number(at);
  const check = () => {
    setRun({ phase: "running" });
    const argv = [
      "context",
      "checkpoint-status",
      ...(profile ? ["--profile", profile] : []),
      ...(window.trim() ? ["--window", window.trim()] : []),
      ...(at.trim() ? ["--checkpoint-at", at.trim()] : []),
    ];
    ctlData<ContextCheckpointStatusData>(argv, { stdinSecret: json }).then(
      (data) => alive.current && setRun({ phase: "done", data }),
      (error) => alive.current && setRun({ phase: "error", error }),
    );
  };
  return (
    <Section
      title="Checkpoint"
      hint="How far a session is from its checkpoint point. Paste the statusline JSON Claude Code gives a status line script."
    >
      <form
        className="flex flex-col gap-3"
        onSubmit={(event) => {
          event.preventDefault();
          if (valid) check();
        }}
      >
        <Field
          label="Statusline JSON"
          hint="Used for this check only. It is not saved and not shown again."
        >
          {(id, describedBy) => (
            <Textarea
              id={id}
              aria-describedby={describedBy}
              className="min-h-20 font-mono text-xs"
              value={json}
              spellCheck={false}
              onChange={(event) => setJson(event.target.value)}
            />
          )}
        </Field>
        <div className="grid gap-3 sm:grid-cols-3">
          <Field label="Launch profile">
            {(id) => (
              <select
                id={id}
                className={SELECT_CLASS}
                value={profile}
                onChange={(e) => setProfile(e.target.value)}
              >
                <option value="">None</option>
                {(profiles ?? []).map((p) => (
                  <option key={p.name} value={p.name}>
                    {p.name}
                  </option>
                ))}
              </select>
            )}
          </Field>
          <Field label="Window (tokens)" hint="Optional">
            {(id, describedBy) => (
              <Input
                id={id}
                aria-describedby={describedBy}
                inputMode="numeric"
                value={window}
                onChange={(e) => setWindow(e.target.value)}
              />
            )}
          </Field>
          <Field label="Checkpoint at (tokens left)" hint="Optional">
            {(id, describedBy) => (
              <Input
                id={id}
                aria-describedby={describedBy}
                inputMode="numeric"
                value={at}
                onChange={(e) => setAt(e.target.value)}
              />
            )}
          </Field>
        </div>
        <div>
          <Button type="submit" size="sm" disabled={!valid || run.phase === "running"}>
            Check
          </Button>
        </div>
      </form>
      {run.phase === "running" && (
        <p role="status" className="text-sm text-muted-foreground">
          Checking…
        </p>
      )}
      {run.phase === "error" && (
        <ErrorState
          error={run.error}
          title="Couldn't read the statusline"
          onRetry={check}
        />
      )}
      {run.phase === "done" && <Gauge data={run.data} />}
    </Section>
  );
}
