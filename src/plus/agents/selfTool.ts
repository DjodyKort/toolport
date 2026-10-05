import { useCallback, useEffect, useState } from "react";
import { CtlError, runCtl, type CtlResult } from "../bridge/ctl";
import type { CommandsData } from "../bridge/data";
import { planTool, toolCallArgv, type ToolRow } from "../allcommands/model";
import type { CtlQuery } from "../ui";

/** `data` of `toolportctl mcp call <tool>` (contract section 15): the tool's own answer. */
export interface ToolAnswer<T> {
  isError: boolean;
  result: T;
  tier: number;
  tool: string;
}

function failure(result: CtlResult): CtlError {
  const error = result.envelope?.error;
  return new CtlError(
    error?.message ?? result.parseError ?? `toolportctl exited with ${result.exitCode}`,
    error?.code ?? "bridge",
    result,
  );
}

/** One tool call. The arguments travel on stdin only, so a skill body never reaches argv or a
 * log; a failed envelope (a tool error or a refusal) throws and never counts as applied. */
export async function callTool<T>(
  tool: string,
  stdin: Record<string, unknown>,
): Promise<T> {
  const result = await runCtl<ToolAnswer<T>>(toolCallArgv(tool), {
    stdinSecret: JSON.stringify(stdin),
  }).result;
  const data = result.envelope?.data;
  if (!result.envelope?.ok || !data) throw failure(result);
  if (data.isError) throw new CtlError("The tool reported an error", "tool", result);
  return data.result;
}

/** The registry row of a tool: its tier and whether it has a dry run. */
export const toolRow = (data: CommandsData | null, tool: string): ToolRow | null =>
  data?.tools.find((row) => row.name === tool) ?? null;

/** What a write tool is called with, from its policy row: `confirm` for tier 3 and up, and
 * the dry run first only when the tool has one. */
export function applyArgs(row: ToolRow, args: Record<string, unknown>): string {
  const plan = planTool(row, args);
  return plan.kind === "preview"
    ? plan.applyStdin
    : plan.kind === "direct"
      ? plan.stdin
      : JSON.stringify(args);
}

export const hasDryRun = (row: ToolRow) => planTool(row, {}).kind === "preview";

/** A read tool (tier 1) as a query: loading, error and ready like every other read. */
export function useToolRead<T>(
  tool: string,
  args: Record<string, unknown>,
  enabled = true,
): CtlQuery<T> {
  const key = JSON.stringify([tool, args]);
  const [tick, setTick] = useState(0);
  const [settled, setSettled] = useState<{
    key: string;
    tick: number;
    data: T | null;
    error: unknown;
  } | null>(null);

  useEffect(() => {
    if (!enabled) return;
    let alive = true;
    const [name, stdin] = JSON.parse(key) as [string, Record<string, unknown>];
    callTool<T>(name, stdin).then(
      (data) => alive && setSettled({ key, tick, data, error: null }),
      (error) => alive && setSettled({ key, tick, data: null, error }),
    );
    return () => {
      alive = false;
    };
  }, [key, tick, enabled]);

  const reload = useCallback(() => setTick((n) => n + 1), []);
  const same = settled?.key === key;
  const fresh = same && settled.tick === tick;
  const status = !fresh ? "loading" : settled.error ? "error" : "ready";
  return {
    status,
    data: same ? settled.data : null,
    error: fresh ? settled.error : null,
    reload,
  };
}
