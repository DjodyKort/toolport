import type { TaskDefinition } from "./data";

export const blankTask = (): TaskDefinition => ({
  id: "",
  title: "",
  description: "",
  enabled: true,
  requires: { servers: [], commands: [] },
  writesSecrets: [],
  steps: [],
  triggers: {
    manual: true,
    cli: false,
    selfMcp: { enabled: false, approval: "every-run" },
    schedule: null,
    onAuthFailure: [],
  },
  createdFrom: null,
});

/** The 1.x steps that named a saved routine become empty script steps the person fills in. */
export function fixRoutines(task: TaskDefinition): TaskDefinition {
  return {
    ...task,
    steps: task.steps.map((step) =>
      step.type === "routine" && step.routineId
        ? { id: step.id, title: step.title, type: "script", script: "", capture: [] }
        : step,
    ),
  };
}

export function duplicateOf(task: TaskDefinition): TaskDefinition {
  return {
    ...structuredClone(task),
    id: `${task.id}-copy`.slice(0, 64),
    title: `${task.title} (copy)`,
    enabled: false,
    createdFrom: { kind: "manual" },
  };
}
