import { useEffect, useRef, useState } from "react";
import { ArrowDown, ArrowLeft, ArrowUp, Plus, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { StatusCard } from "@/components/ui/status-card";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { toastError } from "@/lib/toast";
import { useLoad } from "../servers/useLoad";
import { errorText } from "../settings/useCtl";
import { fromCommand, saveTask, showTask } from "./commands";
import type { SavePlanData, TaskDefinition, TaskStep } from "./data";
import {
  cronOf,
  kindLabel,
  presetOf,
  SLUG,
  slugify,
  stamp,
  STEP_KINDS,
  utcHint,
  type Preset,
  type PresetFields,
} from "./model";
import { now } from "./clock";
import { SELECT_CLASS } from "./classes";
import { blankTask, fixRoutines } from "./draft";
import { Chip, Field, Group, Mono, Quiet } from "./ui";

export type FormMode =
  { kind: "new"; seed?: TaskDefinition } | { kind: "edit"; id: string; fix?: boolean };

function ListField({
  label,
  value,
  onChange,
  hint,
  rows = 2,
  separator = "\n",
}: {
  label: string;
  value: string[];
  onChange: (next: string[]) => void;
  hint?: string;
  rows?: number;
  separator?: string;
}) {
  const [raw, setRaw] = useState(value.join(separator));
  return (
    <Field label={label} hint={hint}>
      <Textarea
        aria-label={label}
        rows={rows}
        value={raw}
        onChange={(e) => {
          setRaw(e.target.value);
          onChange(
            e.target.value
              .split(separator === "\n" ? "\n" : /[\n,]/)
              .map((v) => v.trim())
              .filter(Boolean),
          );
        }}
      />
    </Field>
  );
}

function newStep(type: TaskStep["type"], taken: string[]): TaskStep {
  const base =
    type === "needs-you" ? "wait" : type === "restart-server" ? "restart" : type;
  let id = base;
  for (let n = 2; taken.includes(id); n++) id = `${base}-${n}`;
  const title = STEP_KINDS.find((k) => k.type === type)?.label ?? type;
  switch (type) {
    case "needs-you":
      return { id, title, type, instructions: "" };
    case "mcp":
      return { id, title, type, server: "", tool: "", args: {} };
    case "exec":
      return { id, title, type, program: "", args: [] };
    case "prompt":
      return { id, title, type, prompt: "", allowedTools: [] };
    case "secret-set":
      return { id, title, type, server: "", key: "", from: "" };
    case "restart-server":
      return { id, title, type, server: "" };
    default:
      return { id, title, type: "script", script: "", capture: [] };
  }
}

function StepFields({
  step,
  onChange,
  onFix,
}: {
  step: TaskStep;
  onChange: (next: TaskStep) => void;
  onFix: () => void;
}) {
  const set = (patch: Record<string, unknown>) =>
    onChange({ ...step, ...patch } as TaskStep);
  const text = (label: string, key: string, hint?: string) => (
    <Field label={label} hint={hint}>
      <Input
        aria-label={label}
        value={String((step as unknown as Record<string, unknown>)[key] ?? "")}
        onChange={(e) => set({ [key]: e.target.value })}
      />
    </Field>
  );
  return (
    <div className="grid gap-3">
      <Field label="Title">
        <Input
          aria-label="Step title"
          value={step.title}
          onChange={(e) => set({ title: e.target.value })}
        />
      </Field>
      {step.type === "needs-you" && (
        <>
          <Field label="Instructions">
            <Textarea
              aria-label="Instructions"
              rows={2}
              value={step.instructions}
              onChange={(e) => set({ instructions: e.target.value })}
            />
          </Field>
          <Field
            label="Time limit (seconds)"
            hint="Empty waits until you press Continue."
          >
            <Input
              aria-label="Time limit (seconds)"
              inputMode="numeric"
              value={step.waitFor ? String(step.waitFor.timeoutSec) : ""}
              onChange={(e) => {
                const n = Number(e.target.value);
                set({
                  waitFor:
                    e.target.value.trim() === "" || !Number.isFinite(n) || n <= 0
                      ? undefined
                      : { kind: "manual", timeoutSec: n },
                });
              }}
            />
          </Field>
        </>
      )}
      {step.type === "mcp" && (
        <>
          {text("Server", "server")}
          {text("Tool", "tool")}
        </>
      )}
      {step.type === "exec" && (
        <>
          {text(
            "Program",
            "program",
            "Must be listed under Needs commands. No shell is used.",
          )}
          <ListField
            label="Arguments"
            hint="One per line."
            value={step.args ?? []}
            onChange={(args) => set({ args })}
          />
        </>
      )}
      {step.type === "prompt" && (
        <Field label="Prompt">
          <Textarea
            aria-label="Prompt"
            rows={3}
            value={step.prompt}
            onChange={(e) => set({ prompt: e.target.value })}
          />
        </Field>
      )}
      {step.type === "secret-set" && (
        <>
          {text("Server", "server")}
          {text(
            "Secret name",
            "key",
            "The name only; the value comes from a kept result.",
          )}
          {text(
            "Kept result",
            "from",
            "The name an earlier step keeps its result under.",
          )}
        </>
      )}
      {step.type === "restart-server" && text("Server", "server")}
      {(step.type === "script" || step.type === "routine") &&
        (step.routineId ? (
          <>
            <Quiet>
              This step names the routine <Mono>{step.routineId}</Mono>. Routines were
              removed in Toolport 2.3, so it cannot run.
            </Quiet>
            <Button size="sm" variant="outline" className="w-fit" onClick={onFix}>
              Replace with a script
            </Button>
          </>
        ) : (
          <Field label="Script" hint="Runs in Code Mode, which is off by default.">
            <Textarea
              aria-label="Script"
              rows={4}
              value={step.script ?? ""}
              onChange={(e) => set({ script: e.target.value })}
            />
          </Field>
        ))}
    </div>
  );
}

function StepsEditor({
  steps,
  onChange,
}: {
  steps: TaskStep[];
  onChange: (next: TaskStep[]) => void;
}) {
  const [open, setOpen] = useState<number | null>(steps.length === 1 ? 0 : null);
  const [adding, setAdding] = useState(false);
  const move = (i: number, by: number) => {
    const next = [...steps];
    [next[i], next[i + by]] = [next[i + by], next[i]];
    onChange(next);
    setOpen(i + by);
  };
  return (
    <section aria-label="Steps" className="grid gap-2">
      <div className="flex items-center gap-3">
        <h3 className="text-xs font-semibold tracking-[0.05em] text-muted-foreground uppercase">
          Steps <span className="ml-2 font-normal tabular-nums">{steps.length}</span>
        </h3>
        <div className="relative ml-auto">
          <Button
            size="sm"
            variant="outline"
            aria-haspopup="menu"
            aria-expanded={adding}
            onClick={() => setAdding((v) => !v)}
          >
            <Plus /> Add step
          </Button>
          {adding && (
            <div
              role="menu"
              className="absolute right-0 z-10 mt-1 grid min-w-52 gap-0.5 rounded-lg border bg-popover p-1 shadow-md"
            >
              {STEP_KINDS.map((k) => (
                <Button
                  key={k.type}
                  role="menuitem"
                  variant="ghost"
                  size="sm"
                  className="justify-between"
                  onClick={() => {
                    onChange([
                      ...steps,
                      newStep(
                        k.type,
                        steps.map((s) => s.id),
                      ),
                    ]);
                    setOpen(steps.length);
                    setAdding(false);
                  }}
                >
                  {k.label}
                  {k.hint && (
                    <span className="text-xs font-normal text-muted-foreground">
                      {k.hint}
                    </span>
                  )}
                </Button>
              ))}
            </div>
          )}
        </div>
      </div>
      <Group>
        {steps.map((step, i) => (
          <div key={step.id} className="grid gap-3 px-4 py-3">
            <div className="flex items-center gap-2">
              <button
                type="button"
                className="flex min-w-0 flex-1 items-center gap-3 text-left"
                aria-expanded={open === i}
                onClick={() => setOpen(open === i ? null : i)}
              >
                <span className="w-5 text-meta text-muted-foreground tabular-nums">
                  {i + 1}
                </span>
                <span className="min-w-0 flex-1 truncate text-sm font-medium">
                  {step.title || "Untitled step"}
                </span>
                <span className="text-meta text-muted-foreground">
                  {kindLabel(step.type)}
                </span>
                {step.type === "routine" && "routineId" in step && step.routineId && (
                  <Chip tone="danger">Cannot run</Chip>
                )}
              </button>
              <Button
                size="icon-xs"
                variant="ghost"
                aria-label={`Move step ${i + 1} up`}
                disabled={i === 0}
                onClick={() => move(i, -1)}
              >
                <ArrowUp />
              </Button>
              <Button
                size="icon-xs"
                variant="ghost"
                aria-label={`Move step ${i + 1} down`}
                disabled={i === steps.length - 1}
                onClick={() => move(i, 1)}
              >
                <ArrowDown />
              </Button>
              <Button
                size="icon-xs"
                variant="ghost"
                aria-label={`Remove step ${i + 1}`}
                onClick={() => {
                  onChange(steps.filter((_, at) => at !== i));
                  setOpen(null);
                }}
              >
                <Trash2 />
              </Button>
            </div>
            {open === i && (
              <StepFields
                step={step}
                onChange={(next) => onChange(steps.map((s, at) => (at === i ? next : s)))}
                onFix={() =>
                  onChange(
                    steps.map((s, at) =>
                      at === i ? fixRoutines({ ...blankTask(), steps: [s] }).steps[0] : s,
                    ),
                  )
                }
              />
            )}
          </div>
        ))}
        {steps.length === 0 && (
          <p className="px-4 py-3 text-sm text-muted-foreground">
            No steps yet. Add the first one.
          </p>
        )}
      </Group>
    </section>
  );
}

const WEEKDAYS = [
  "Sunday",
  "Monday",
  "Tuesday",
  "Wednesday",
  "Thursday",
  "Friday",
  "Saturday",
];

function ScheduleEditor({
  draft,
  onChange,
  nextRuns,
}: {
  draft: TaskDefinition;
  onChange: (next: TaskDefinition) => void;
  nextRuns: string[] | null;
}) {
  const schedule = draft.triggers.schedule;
  const parsed = presetOf(schedule?.cron ?? "7 */4 * * *");
  const [preset, setPreset] = useState<Preset>(parsed.preset);
  const [fields, setFields] = useState<PresetFields>(parsed.fields);
  const [custom, setCustom] = useState(schedule?.cron ?? "0 7 * * 1");
  const push = (p: Preset, f: PresetFields, c: string) =>
    onChange({
      ...draft,
      triggers: {
        ...draft.triggers,
        schedule: { cron: cronOf(p, f, c), autoRun: schedule?.autoRun ?? false },
      },
    });
  const num = (label: string, key: keyof PresetFields, max: number, min = 0) => (
    <Field label={label}>
      <Input
        aria-label={label}
        type="number"
        min={min}
        max={max}
        value={fields[key]}
        onChange={(e) => {
          const next = {
            ...fields,
            [key]: Math.min(max, Math.max(min, Number(e.target.value) || 0)),
          };
          setFields(next);
          push(preset, next, custom);
        }}
      />
    </Field>
  );
  if (!schedule) return null;
  return (
    <div className="grid gap-3 border-t px-4 py-3">
      <Field label="Repeat">
        <select
          aria-label="Repeat"
          className={SELECT_CLASS}
          value={preset}
          onChange={(e) => {
            const next = e.target.value as Preset;
            setPreset(next);
            push(next, fields, custom);
          }}
        >
          <option value="hours">Every N hours</option>
          <option value="daily">Daily</option>
          <option value="weekly">Weekly</option>
          <option value="custom">Custom cron</option>
        </select>
      </Field>
      {preset === "hours" && (
        <div className="grid grid-cols-2 gap-3">
          {num("Every (hours)", "every", 23, 1)}
          {num("At minute", "minute", 59)}
        </div>
      )}
      {(preset === "daily" || preset === "weekly") && (
        <div className="grid grid-cols-2 gap-3">
          {num("Hour (UTC)", "hour", 23)}
          {num("At minute", "minute", 59)}
        </div>
      )}
      {preset === "weekly" && (
        <Field label="Day (UTC)">
          <select
            aria-label="Day (UTC)"
            className={SELECT_CLASS}
            value={fields.weekday}
            onChange={(e) => {
              const next = { ...fields, weekday: Number(e.target.value) };
              setFields(next);
              push(preset, next, custom);
            }}
          >
            {WEEKDAYS.map((d, i) => (
              <option key={d} value={i}>
                {d}
              </option>
            ))}
          </select>
        </Field>
      )}
      {(preset === "daily" || preset === "weekly") && (
        <Quiet>{utcHint(fields.hour, fields.minute, now())}</Quiet>
      )}
      <Field
        label="Cron, read in UTC"
        hint="Five fields. Edit it directly under Custom cron."
      >
        <Input
          aria-label="Cron, read in UTC"
          value={preset === "custom" ? custom : schedule.cron}
          readOnly={preset !== "custom"}
          onChange={(e) => {
            setCustom(e.target.value);
            push("custom", fields, e.target.value);
          }}
        />
      </Field>
      <div className="grid gap-1">
        <p className="text-sm font-medium">Next runs</p>
        {nextRuns === null ? (
          <Quiet>Shown once the task is valid.</Quiet>
        ) : nextRuns.length === 0 ? (
          <Quiet>The cron has no next run.</Quiet>
        ) : (
          <ul className="grid gap-0.5 text-sm">
            {nextRuns.map((run) => (
              <li key={run} className="flex gap-3">
                <span className="w-36 tabular-nums">{stamp(Date.parse(run), now())}</span>
                <span className="text-muted-foreground tabular-nums">
                  {run.slice(11, 16)} UTC
                </span>
              </li>
            ))}
          </ul>
        )}
        <Quiet>
          The cron has no time zone. When the clocks change, local times shift by an hour;
          the next runs are recalculated each time you open this page.
        </Quiet>
      </div>
      <div className="flex items-start gap-3">
        <div className="flex-1">
          <p className="text-sm font-medium">Run without asking</p>
          <Quiet>
            Needed for a run with nobody in front of the screen. A task with a Needs you
            step cannot have this: it asks you instead.
          </Quiet>
        </div>
        <Switch
          aria-label="Run without asking"
          checked={schedule.autoRun}
          disabled={draft.steps.some((s) => s.type === "needs-you")}
          onCheckedChange={(autoRun) =>
            onChange({
              ...draft,
              triggers: { ...draft.triggers, schedule: { ...schedule, autoRun } },
            })
          }
        />
      </div>
    </div>
  );
}

function PlanView({ plan }: { plan: SavePlanData }) {
  const step = plan.plan.steps[0];
  return (
    <div className="grid gap-3">
      <p className="text-sm font-semibold">{plan.plan.summary}?</p>
      <Quiet>
        Writes <Mono>{plan.path}</Mono>
      </Quiet>
      {plan.created ? (
        <Quiet>A new file; nothing is overwritten.</Quiet>
      ) : (
        <Quiet>Backs up the old file first.</Quiet>
      )}
      {plan.plan.warnings.map((w) => (
        <p key={w} className="text-sm text-warning">
          {w}
        </p>
      ))}
      {step?.diff && (
        <details>
          <summary className="cursor-pointer text-sm">Show the file</summary>
          <pre className="mt-2 max-h-72 overflow-auto rounded-md bg-muted p-2 text-[12px] whitespace-pre-wrap">
            {step.diff.after}
          </pre>
        </details>
      )}
      <Quiet>
        Undo: <Mono>{plan.plan.undo}</Mono>. A live run keeps going with the steps it
        started with.
      </Quiet>
    </div>
  );
}

export interface TaskFormProps {
  mode: FormMode;
  onDone: (id: string) => void;
  onCancel: () => void;
}

/** New task and Edit: one page, nothing is written until Review changes shows the plan. */
export function TaskForm({ mode, onDone, onCancel }: TaskFormProps) {
  const existing = useLoad(
    () => (mode.kind === "edit" ? showTask(mode.id) : Promise.resolve(null)),
    mode.kind === "edit" ? mode.id : "new",
  );
  const [edited, setDraft] = useState<TaskDefinition | null>(
    mode.kind === "new" ? (mode.seed ?? blankTask()) : null,
  );
  const loaded = mode.kind === "edit" ? existing.data?.task : undefined;
  const draft =
    edited ??
    (loaded ? (mode.kind === "edit" && mode.fix ? fixRoutines(loaded) : loaded) : null);
  const [idTouched, setIdTouched] = useState(mode.kind === "new" && Boolean(mode.seed));
  const [review, setReview] = useState<
    | { kind: "loading" }
    | { kind: "plan"; plan: SavePlanData }
    | { kind: "error"; message: string }
    | null
  >(null);
  const [preview, setPreview] = useState<{ cron: string; runs: string[] | null } | null>(
    null,
  );
  const [saving, setSaving] = useState(false);
  const create = mode.kind === "new";

  const cron = draft?.triggers.schedule?.cron;
  const ready =
    draft !== null &&
    SLUG.test(draft.id) &&
    draft.title.trim() !== "" &&
    draft.steps.length > 0;
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => {
    clearTimeout(timer.current);
    if (!draft || !cron || !ready) return;
    timer.current = setTimeout(() => {
      saveTask(draft, create, true).then(
        (plan) => setPreview({ cron, runs: plan.nextRuns ?? null }),
        () => setPreview({ cron, runs: null }),
      );
    }, 400);
    return () => clearTimeout(timer.current);
    // The preview follows the cron and the validity, not every keystroke elsewhere.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cron, ready]);

  if (existing.error && !draft)
    return (
      <StatusCard
        variant="danger"
        role="alert"
        icon={<ArrowLeft />}
        title="The task could not be read"
        detail={existing.error}
        actions={
          <Button size="sm" variant="outline" onClick={onCancel}>
            Back
          </Button>
        }
      />
    );
  if (!draft) return <Quiet role="status">Reading the task…</Quiet>;

  const nextRuns = ready && preview?.cron === cron ? (preview?.runs ?? null) : null;
  const routine = draft.steps.some(
    (s) => s.type === "routine" && "routineId" in s && s.routineId,
  );
  const patch = (next: Partial<TaskDefinition>) => setDraft({ ...draft, ...next });
  const trig = (next: Partial<TaskDefinition["triggers"]>) =>
    patch({ triggers: { ...draft.triggers, ...next } });

  async function reviewChanges() {
    setReview({ kind: "loading" });
    try {
      setReview({ kind: "plan", plan: await saveTask(draft!, create, true) });
    } catch (error) {
      setReview({ kind: "error", message: errorText(error) });
    }
  }

  async function save() {
    setSaving(true);
    try {
      await saveTask(draft!, create, false);
      toast.success(create ? `Created ${draft!.title}` : `Saved ${draft!.title}`);
      onDone(draft!.id);
    } catch (error) {
      toastError("The task was not saved", { description: errorText(error) });
      setReview({ kind: "error", message: errorText(error) });
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="grid gap-4">
      <Button variant="ghost" size="sm" className="w-fit" onClick={onCancel}>
        <ArrowLeft /> {create ? "Tasks" : "Back"}
      </Button>
      <h2 className="text-lg font-semibold">{create ? "New task" : "Edit task"}</h2>
      {mode.kind === "new" && mode.seed && (
        <Quiet>
          Review the copy. It starts Off, so nothing runs until you turn it on.
        </Quiet>
      )}

      <section className="grid gap-3" aria-label="Basics">
        <Field label="Name" htmlFor="task-name" hint="Shown in lists and in Attention.">
          <Input
            id="task-name"
            value={draft.title}
            onChange={(e) =>
              patch({
                title: e.target.value,
                ...(create && !idTouched ? { id: slugify(e.target.value) } : {}),
              })
            }
          />
        </Field>
        <Field
          label="ID"
          htmlFor="task-id"
          hint={
            <>
              Letters, digits and dashes. The terminal runs it as{" "}
              <Mono>{`toolportctl task run ${draft.id || "<id>"}`}</Mono>.
              {create ? "" : " Cannot change after saving."}
            </>
          }
        >
          <Input
            id="task-id"
            value={draft.id}
            readOnly={!create}
            aria-invalid={draft.id !== "" && !SLUG.test(draft.id)}
            onChange={(e) => {
              setIdTouched(true);
              patch({ id: e.target.value });
            }}
          />
        </Field>
        <Field label="Description" htmlFor="task-description">
          <Textarea
            id="task-description"
            rows={2}
            value={draft.description}
            onChange={(e) => patch({ description: e.target.value })}
          />
        </Field>
        <div className="flex items-start gap-3">
          <div className="flex-1">
            <p className="text-sm font-medium">On</p>
            <Quiet>A task that is off cannot run, by any trigger.</Quiet>
          </div>
          <Switch
            aria-label="On"
            checked={draft.enabled}
            onCheckedChange={(enabled) => patch({ enabled })}
          />
        </div>
      </section>

      <StepsEditor steps={draft.steps} onChange={(steps) => patch({ steps })} />

      <section aria-label="How it starts" className="grid gap-2">
        <h3 className="text-xs font-semibold tracking-[0.05em] text-muted-foreground uppercase">
          How it starts
        </h3>
        <Group>
          <div className="px-4 py-3">
            <p className="text-sm font-medium">Button in the app</p>
            <Quiet>Always available. Recorded as Manual.</Quiet>
          </div>
          <div className="flex items-start gap-3 px-4 py-3">
            <div className="flex-1">
              <p className="text-sm font-medium">From the terminal</p>
              <Quiet>
                <Mono>{`toolportctl task run ${draft.id || "<id>"}`}</Mono>
              </Quiet>
            </div>
            <Switch
              aria-label="From the terminal"
              checked={draft.triggers.cli}
              onCheckedChange={(cli) => trig({ cli })}
            />
          </div>
          <div className="flex items-start gap-3 px-4 py-3">
            <div className="flex-1">
              <p className="text-sm font-medium">Claude may ask</p>
              <Quiet>
                Through Toolport&apos;s own MCP. You approve every run; there is no
                setting for fewer prompts.
              </Quiet>
            </div>
            <Switch
              aria-label="Claude may ask"
              checked={draft.triggers.selfMcp.enabled}
              onCheckedChange={(enabled) =>
                trig({ selfMcp: { enabled, approval: "every-run" } })
              }
            />
          </div>
          <div>
            <div className="flex items-start gap-3 px-4 py-3">
              <div className="flex-1">
                <p className="text-sm font-medium">On a schedule</p>
                <Quiet>Starts the task by itself while Toolport runs.</Quiet>
              </div>
              <Switch
                aria-label="On a schedule"
                checked={draft.triggers.schedule !== null}
                onCheckedChange={(on) =>
                  trig({ schedule: on ? { cron: "7 */4 * * *", autoRun: false } : null })
                }
              />
            </div>
            <ScheduleEditor
              key={draft.triggers.schedule === null ? "off" : "on"}
              draft={draft}
              onChange={setDraft}
              nextRuns={nextRuns}
            />
          </div>
          <div className="px-4 py-3">
            <ListField
              label="When a login fails"
              hint="Servers whose failed sign-in offers this task as the fix (asks you, never runs alone). Saved, but nothing starts this yet: it waits for the login check in Attention."
              value={draft.triggers.onAuthFailure}
              onChange={(onAuthFailure) => trig({ onAuthFailure })}
            />
          </div>
        </Group>
      </section>

      <section aria-label="Access" className="grid gap-3">
        <h3 className="text-xs font-semibold tracking-[0.05em] text-muted-foreground uppercase">
          Access
        </h3>
        <ListField
          label="Needs servers"
          hint="Servers a Call a server tool step uses. Shown in the plan before each run."
          value={draft.requires.servers}
          onChange={(servers) => patch({ requires: { ...draft.requires, servers } })}
        />
        <ListField
          label="Needs commands"
          hint="Programs a Run a program step may start."
          value={draft.requires.commands}
          onChange={(commands) => patch({ requires: { ...draft.requires, commands } })}
        />
        <ListField
          label="Allowed to write"
          hint="The only secrets this task may store, as server/KEY. Values are never shown."
          value={draft.writesSecrets.map((s) => `${s.server}/${s.key}`)}
          onChange={(lines) =>
            patch({
              writesSecrets: lines.map((line) => {
                const [server, ...rest] = line.split("/");
                return { server, key: rest.join("/") };
              }),
            })
          }
        />
      </section>

      {routine && (
        <Quiet role="alert">Replace the routine step with a script before saving.</Quiet>
      )}

      {review ? (
        <section
          aria-label="Review changes"
          className="grid gap-3 rounded-xl border bg-card p-4"
        >
          {review.kind === "loading" && (
            <Quiet role="status">Preparing the review…</Quiet>
          )}
          {review.kind === "error" && <Quiet role="alert">{review.message}</Quiet>}
          {review.kind === "plan" && <PlanView plan={review.plan} />}
          <div className="flex gap-2">
            {review.kind === "plan" && (
              <Button size="sm" disabled={saving} onClick={() => void save()}>
                {create ? "Create task" : "Save task"}
              </Button>
            )}
            <Button size="sm" variant="ghost" onClick={() => setReview(null)}>
              Back to editing
            </Button>
          </div>
        </section>
      ) : (
        <div className="flex items-center gap-3">
          <Button
            size="sm"
            disabled={!ready || routine}
            onClick={() => void reviewChanges()}
          >
            Review changes
          </Button>
          <Button size="sm" variant="ghost" onClick={onCancel}>
            Cancel
          </Button>
          <Quiet>Nothing is saved yet.</Quiet>
        </div>
      )}
    </div>
  );
}

/** New from a command: a Claude command file becomes a draft task that starts Off. */
export function FromCommand({
  onDone,
  onCancel,
}: {
  onDone: (id: string) => void;
  onCancel: () => void;
}) {
  const [path, setPath] = useState("");
  const [id, setId] = useState("");
  const [idTouched, setIdTouched] = useState(false);
  const [review, setReview] = useState<
    { kind: "plan"; plan: SavePlanData } | { kind: "error"; message: string } | null
  >(null);
  async function preview() {
    try {
      setReview({ kind: "plan", plan: await fromCommand(id, path.trim(), true) });
    } catch (error) {
      setReview({ kind: "error", message: errorText(error) });
    }
  }
  async function create() {
    try {
      await fromCommand(id, path.trim(), false);
      toast.success(`Created ${id}, switched Off`);
      onDone(id);
    } catch (error) {
      setReview({ kind: "error", message: errorText(error) });
    }
  }
  return (
    <div className="grid gap-4">
      <Button variant="ghost" size="sm" className="w-fit" onClick={onCancel}>
        <ArrowLeft /> Tasks
      </Button>
      <h2 className="text-lg font-semibold">New task from a command</h2>
      <Quiet>
        Pick a Claude command that already does the work. Toolport makes a task from it,
        switched Off, for you to check.
      </Quiet>
      <Field
        label="Command file"
        htmlFor="command-path"
        hint="For example ~/.claude/commands/sync-notes.md"
      >
        <Input
          id="command-path"
          value={path}
          onChange={(e) => {
            setPath(e.target.value);
            if (!idTouched)
              setId(
                slugify((e.target.value.split("/").pop() ?? "").replace(/\.md$/, "")),
              );
          }}
        />
      </Field>
      <Field label="ID" htmlFor="command-id" hint="Letters, digits and dashes.">
        <Input
          id="command-id"
          value={id}
          onChange={(e) => {
            setIdTouched(true);
            setId(e.target.value);
          }}
        />
      </Field>
      {review?.kind === "error" && <Quiet role="alert">{review.message}</Quiet>}
      {review?.kind === "plan" && (
        <section
          aria-label="Review draft"
          className="grid gap-3 rounded-xl border bg-card p-4"
        >
          <PlanView plan={review.plan} />
          <div className="flex gap-2">
            <Button size="sm" onClick={() => void create()}>
              Create task
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setReview(null)}>
              Back
            </Button>
          </div>
        </section>
      )}
      {review?.kind !== "plan" && (
        <div className="flex gap-2">
          <Button
            size="sm"
            disabled={!SLUG.test(id) || path.trim() === ""}
            onClick={() => void preview()}
          >
            Review draft
          </Button>
          <Button size="sm" variant="ghost" onClick={onCancel}>
            Cancel
          </Button>
        </div>
      )}
    </div>
  );
}
