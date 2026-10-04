import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const SKIPPED_DIRS = new Set([
  "fixtures",
  "__screenshots__",
  "hook-payloads",
  "node_modules",
]);
const IS_SOURCE = /\.tsx?$/;
const IS_TEST = /\.test\.tsx?$/;

/** `plus.<area>.<name>` with any number of segments, camelCase, digits and underscores. */
const PLUS_COMMAND = /["'`](plus(?:\.[A-Za-z][A-Za-z0-9_]*)+)["'`]/g;
const CTL_CALL = /\b(?:ctlData|runCtl)(?:<[^()]*?>)?\(\s*\[([^\]]*)\]/g;

export interface CtlCall {
  /** The leading string literals of the argv. */
  words: string[];
  /** True when the whole argv is literal; otherwise only the words are known. */
  complete: boolean;
}

export function sourceFiles(dir: string): string[] {
  const found: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (!SKIPPED_DIRS.has(entry.name)) found.push(...sourceFiles(path));
    } else if (IS_SOURCE.test(entry.name) && !IS_TEST.test(entry.name)) {
      found.push(path);
    }
  }
  return found;
}

export function plusInvokeNames(source: string): string[] {
  return [...source.matchAll(PLUS_COMMAND)].map((m) => m[1]);
}

export function ctlCalls(source: string): CtlCall[] {
  return [...source.matchAll(CTL_CALL)].map((m) => {
    const items = m[1].split(",").map((item) => item.trim());
    const words: string[] = [];
    for (const item of items) {
      const literal = /^"([^"]*)"$/.exec(item);
      if (!literal) break;
      words.push(literal[1]);
    }
    return { words, complete: words.length === items.filter((i) => i !== "").length };
  });
}

/** Everything under `dir` (recursively, tests and fixtures excluded) that calls the app. */
export function scanPlusSources(dir: string) {
  const invoked = new Set<string>();
  const ctl: CtlCall[] = [];
  for (const file of sourceFiles(dir)) {
    const source = readFileSync(file, "utf8");
    for (const name of plusInvokeNames(source)) invoked.add(name);
    ctl.push(...ctlCalls(source));
  }
  return { invoked, ctl };
}

export function ctlCallIsCovered(call: CtlCall, keys: Iterable<string>): boolean {
  const joined = call.words.join(" ");
  return [...keys].some((key) =>
    call.complete ? key === joined : key === joined || key.startsWith(`${joined} `),
  );
}
