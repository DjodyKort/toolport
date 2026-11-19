import type { WriteOutcome } from "./data";

/** Why a name cannot be used for a new set, or `null`. The CLI reads a leading `-` as a flag and
 * reserves a leading `@`. */
export function nameProblem(name: string, taken: string[]): string | null {
  const text = name.trim();
  if (text === "") return null;
  if (text.startsWith("@")) return "Names beginning with @ are reserved.";
  if (text.startsWith("-")) return "A name cannot begin with a hyphen.";
  if (taken.some((other) => other.toLowerCase() === text.toLowerCase())) {
    return "An access set with this name exists already.";
  }
  return null;
}

/** The gateway matches the absolute folder a client reports, so a relative one is refused. */
export function absoluteFolder(path: string): boolean {
  const text = path.trim().replace(/\\/g, "/");
  return text.startsWith("/") || /^[A-Za-z]:\//.test(text);
}

/** A plan that only says nothing changes: every step is a note. */
export const nothingToChange = (outcome: WriteOutcome) =>
  outcome.plan.steps.every((step) => step.op === "note");
