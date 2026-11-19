import { ctlData } from "../bridge/ctl";
import { runTool } from "./commands";

export type UndoStep =
  | { kind: "ctl"; argv: string[] }
  | { kind: "tool"; tool: string; args: Record<string, string> };

/** Splits the words of one line the way the CLI quotes them: single quotes keep a word whole,
 * `'\''` is a quote inside it, and `&&` ends a command. */
function words(line: string): string[] {
  const out: string[] = [];
  let word: string | null = null;
  let quoted = false;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (quoted) {
      if (c === "'") quoted = false;
      else word += c;
    } else if (c === "'") {
      quoted = true;
      word ??= "";
    } else if (c === "\\" && i + 1 < line.length) {
      word = (word ?? "") + line[++i];
    } else if (/\s/.test(c)) {
      if (word !== null) out.push(word);
      word = null;
    } else {
      word = (word ?? "") + c;
    }
  }
  if (word !== null) out.push(word);
  return out;
}

/** What the `undo` line of a plan runs. The plans word it as `toolportctl <command>` (several
 * joined with `&&`) or, for the discovery tool, as `client_discovery_set key=value ...`. */
export function undoSteps(undo: string): UndoStep[] {
  const steps: UndoStep[] = [];
  let current: string[] = [];
  const flush = () => {
    if (current.length === 0) return;
    const [head, ...rest] = current;
    if (head === "toolportctl") steps.push({ kind: "ctl", argv: rest });
    else if (rest.every((word) => word.includes("="))) {
      steps.push({
        kind: "tool",
        tool: head,
        args: Object.fromEntries(
          rest.map((word) => [
            word.slice(0, word.indexOf("=")),
            word.slice(word.indexOf("=") + 1),
          ]),
        ),
      });
    } else throw new Error(`Cannot undo with: ${undo}`);
    current = [];
  };
  for (const word of words(undo)) {
    if (word === "&&") flush();
    else current.push(word);
  }
  flush();
  return steps;
}

/** Runs the undo a plan offered, each command in turn; the first failure stops it. */
export async function runUndo(undo: string): Promise<void> {
  for (const step of undoSteps(undo)) {
    if (step.kind === "ctl") await ctlData(step.argv);
    else await runTool(step.tool, { ...step.args, dry_run: false, confirm: true });
  }
}
