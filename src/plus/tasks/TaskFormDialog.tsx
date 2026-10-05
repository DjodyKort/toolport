import { useId, useState } from "react";
import { ArrowDown, ArrowUp, Plus, Trash2 } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { Field, Toggle } from "../system/atoms";
import {
  STEP_TYPES,
  blankStep,
  taskFromForm,
  type StepForm,
  type StepType,
  type TaskForm,
} from "./model";
import type { TaskDefinition } from "../types/tasks";

function Area({
  label,
  value,
  onChange,
  hint,
  rows = 2,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  hint?: string;
  rows?: number;
}) {
  const id = useId();
  return (
    <div className="flex min-w-0 flex-col gap-1">
      <label htmlFor={id} className="text-sm font-medium">
        {label}
      </label>
      <textarea
        id={id}
        rows={rows}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        spellCheck={false}
        autoComplete="off"
        className="w-full rounded-md border bg-background px-3 py-2 font-mono text-sm shadow-xs outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50"
      />
      {hint && <p className="text-xs text-muted-foreground">{hint}</p>}
    </div>
  );
}

function StepFields({
  step,
  n,
  patch,
}: {
  step: StepForm;
  n: number;
  patch: (change: Partial<StepForm>) => void;
}) {
  const f = (label: string, key: keyof StepForm, hint?: string) => (
    <Field
      label={`${label} (step ${n})`}
      value={step[key] as string}
      onChange={(value) => patch({ [key]: value })}
      hint={hint}
    />
  );
  switch (step.type) {
    case "needs-you":
      return (
        <>
          <Area
            label={`Instructions (step ${n})`}
            value={step.instructions}
            onChange={(instructions) => patch({ instructions })}
            hint="What you are asked to do. The run pauses here until you press Continue."
          />
          <div className="flex min-w-0 flex-col gap-1">
            <label className="text-sm font-medium">
              Waits for (step {n})
              <select
                value={step.waitKind}
                onChange={(event) =>
                  patch({ waitKind: event.target.value as StepForm["waitKind"] })
                }
                className="mt-1 block h-9 w-full rounded-md border bg-background px-2 text-sm"
              >
                <option value="">Until you continue</option>
                <option value="manual">You, with a time limit</option>
                <option value="url">A browser sign-in, with a time limit</option>
                <option value="secret-unset">The secrets it writes to be set</option>
              </select>
            </label>
          </div>
          {step.waitKind && f("Time limit in seconds", "timeoutSec")}
        </>
      );
    case "mcp":
      return (
        <>
          {f("Server", "server")}
          {f("Tool", "tool")}
          {f("Arguments as JSON", "args", 'For example {"name": "x"}')}
          {f("Keep these results", "capture", "Names, separated by commas")}
        </>
      );
    case "routine":
      return (
        <>
          {f("Routine id", "routineId")}
          {f("Script", "script")}
          {f("Keep these results", "capture", "Names, separated by commas")}
        </>
      );
    case "exec":
      return (
        <>
          {f(
            "Program",
            "program",
            "Must be listed under Needs commands. No shell is used.",
          )}
          <Area
            label={`Arguments (step ${n})`}
            value={step.argv}
            onChange={(argv) => patch({ argv })}
            hint="One per line."
          />
        </>
      );
    case "prompt":
      return (
        <>
          <Area
            label={`Prompt (step ${n})`}
            value={step.prompt}
            onChange={(prompt) => patch({ prompt })}
          />
          {f("Allowed tools", "allowedTools", "Separated by commas")}
          {f("Model", "model")}
        </>
      );
    case "secret-set":
      return (
        <>
          {f("Server", "server")}
          {f("Key", "key", "Must be listed under Allowed to write")}
          {f(
            "Value comes from",
            "from",
            "The name of a kept result. The value is never shown.",
          )}
        </>
      );
    case "restart-server":
      return f("Server", "server");
  }
}

/** The form of a task. The steps are edited one by one, each with the fields of its type;
 * the definition goes to the CLI as a file on stdin and is previewed before anything is saved. */
export function TaskFormDialog({
  mode,
  form,
  onChange,
  onReview,
  onClose,
}: {
  mode: "add" | "edit";
  form: TaskForm;
  onChange: (form: TaskForm) => void;
  onReview: (task: TaskDefinition) => void;
  onClose: () => void;
}) {
  const [errors, setErrors] = useState<string[]>([]);
  const [next, setNext] = useState<StepType>("exec");
  const set = (change: Partial<TaskForm>) => onChange({ ...form, ...change });
  const patchStep = (i: number, change: Partial<StepForm>) =>
    set({ steps: form.steps.map((s, at) => (at === i ? { ...s, ...change } : s)) });
  const move = (i: number, by: number) => {
    const steps = [...form.steps];
    [steps[i], steps[i + by]] = [steps[i + by], steps[i]];
    set({ steps });
  };

  function review() {
    const built = taskFromForm(form);
    setErrors(built.errors);
    if (built.task) onReview(built.task);
  }

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent aria-describedby={undefined} className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{mode === "add" ? "New task" : `Edit ${form.id}`}</DialogTitle>
        </DialogHeader>
        <div className="flex max-h-[65vh] flex-col gap-4 overflow-y-auto pr-1">
          <Field
            label="Id"
            value={form.id}
            onChange={(id) => set({ id })}
            disabled={mode === "edit"}
            hint="Letters, digits and dashes. The CLI runs it as: toolportctl task run <id>"
          />
          <Field label="Title" value={form.title} onChange={(title) => set({ title })} />
          <Field
            label="Description"
            value={form.description}
            onChange={(description) => set({ description })}
          />
          <Toggle
            label="Enabled"
            checked={form.enabled}
            onChange={(enabled) => set({ enabled })}
            hint="A task that is off cannot be run."
          />
          <div className="grid gap-3 sm:grid-cols-2">
            <Field
              label="Needs servers"
              value={form.servers}
              onChange={(servers) => set({ servers })}
              hint="Separated by commas"
            />
            <Field
              label="Needs commands"
              value={form.commands}
              onChange={(commands) => set({ commands })}
              hint="Programs an exec step may run"
            />
          </div>
          <Area
            label="Allowed to write"
            value={form.secrets}
            onChange={(secrets) => set({ secrets })}
            hint="The only secrets this task may store, one per line as server/KEY. Values are never shown."
          />
          <fieldset className="flex flex-col gap-2 rounded-lg border p-3">
            <legend className="px-1 text-sm font-semibold">Triggers</legend>
            <Toggle
              label="From the command line"
              checked={form.cli}
              onChange={(cli) => set({ cli })}
            />
            <Toggle
              label="Claude may ask to run it"
              checked={form.selfMcp}
              onChange={(selfMcp) => set({ selfMcp })}
              hint="Through the self-management MCP. You approve every run."
            />
            <Toggle
              label="On a schedule"
              checked={form.schedule}
              onChange={(schedule) => set({ schedule })}
            />
            {form.schedule && (
              <div className="grid gap-3 sm:grid-cols-2">
                <Field
                  label="Cron expression"
                  value={form.cron}
                  onChange={(cron) => set({ cron })}
                  hint="Five fields, for example 0 8 * * *"
                />
                <Toggle
                  label="Run without asking"
                  checked={form.autoRun}
                  onChange={(autoRun) => set({ autoRun })}
                  hint="A task with a step that needs you never runs on its own."
                />
              </div>
            )}
            <Field
              label="When the login fails for"
              value={form.onAuthFailure}
              onChange={(onAuthFailure) => set({ onAuthFailure })}
              hint="Servers, separated by commas. It then asks you whether to run."
            />
          </fieldset>
          <div className="flex flex-col gap-2">
            <h3 className="text-sm font-semibold">Steps</h3>
            {form.steps.map((step, i) => (
              <section
                key={i}
                role="group"
                aria-label={`Step ${i + 1}`}
                className="flex flex-col gap-2 rounded-lg border p-3"
              >
                <div className="flex flex-wrap items-center gap-2">
                  <b className="text-sm">
                    {i + 1}. {STEP_TYPES.find((t) => t.id === step.type)?.label}
                  </b>
                  <span className="ml-auto flex gap-1">
                    <Button
                      size="icon-sm"
                      variant="ghost"
                      aria-label={`Move step ${i + 1} up`}
                      disabled={i === 0}
                      onClick={() => move(i, -1)}
                    >
                      <ArrowUp />
                    </Button>
                    <Button
                      size="icon-sm"
                      variant="ghost"
                      aria-label={`Move step ${i + 1} down`}
                      disabled={i === form.steps.length - 1}
                      onClick={() => move(i, 1)}
                    >
                      <ArrowDown />
                    </Button>
                    <Button
                      size="icon-sm"
                      variant="ghost"
                      aria-label={`Remove step ${i + 1}`}
                      onClick={() =>
                        set({ steps: form.steps.filter((_, at) => at !== i) })
                      }
                    >
                      <Trash2 />
                    </Button>
                  </span>
                </div>
                <div className="grid gap-3 sm:grid-cols-2">
                  <Field
                    label={`Step id (step ${i + 1})`}
                    value={step.id}
                    onChange={(id) => patchStep(i, { id })}
                  />
                  <Field
                    label={`Step title (step ${i + 1})`}
                    value={step.title}
                    onChange={(title) => patchStep(i, { title })}
                  />
                </div>
                <StepFields
                  step={step}
                  n={i + 1}
                  patch={(change) => patchStep(i, change)}
                />
              </section>
            ))}
            <div className="flex flex-wrap items-center gap-2">
              <label className="flex items-center gap-2 text-sm">
                Add a step
                <select
                  value={next}
                  onChange={(event) => setNext(event.target.value as StepType)}
                  className="h-8 rounded-md border bg-background px-2 text-sm"
                >
                  {STEP_TYPES.map((type) => (
                    <option key={type.id} value={type.id}>
                      {type.label}
                    </option>
                  ))}
                </select>
              </label>
              <Button
                size="sm"
                variant="outline"
                onClick={() => set({ steps: [...form.steps, blankStep(next)] })}
              >
                <Plus /> Add step
              </Button>
            </div>
          </div>
          {errors.length > 0 && (
            <Callout variant="danger" role="alert">
              <ul className="list-disc pl-4">
                {errors.map((error) => (
                  <li key={error}>{error}</li>
                ))}
              </ul>
            </Callout>
          )}
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={review}>Review changes</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
