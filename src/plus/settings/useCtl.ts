import { useCallback, useEffect, useState } from "react";
import { CtlError, ctlData } from "../bridge/ctl";
import type {
  CommandsData,
  DoctorData,
  McpDoctorData,
  McpToolsData,
  StatusData,
} from "../bridge/data";

export interface Section<T> {
  data: T | null;
  /** Set when the command could not run or answered with an error and no data. */
  error: string | null;
  /** A failed envelope that still carried its data, such as a doctor with a failing check. */
  unhealthy: boolean;
}

export function errorText(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}

/** Runs one read command. A failed envelope that carries data (doctor with a failing check) still
 * returns that data, so the page can show which check failed. */
export async function readCtl<T>(argv: string[]): Promise<Section<T>> {
  try {
    return { data: await ctlData<T>(argv), error: null, unhealthy: false };
  } catch (error) {
    if (error instanceof CtlError && error.data !== undefined && error.data !== null) {
      return { data: error.data as T, error: null, unhealthy: true };
    }
    return { data: null, error: errorText(error), unhealthy: false };
  }
}

export interface Overview {
  status: Section<StatusData>;
  doctor: Section<DoctorData>;
  mcp: Section<McpDoctorData>;
  tools: Section<McpToolsData>;
  commands: Section<CommandsData>;
}

export const NO_SECTION = { data: null, error: null, unhealthy: false } as const;

async function loadOverview(): Promise<Overview> {
  const [status, doctor, mcp, tools, commands] = await Promise.all([
    readCtl<StatusData>(["status"]),
    readCtl<DoctorData>(["doctor"]),
    readCtl<McpDoctorData>(["mcp", "doctor"]),
    readCtl<McpToolsData>(["mcp", "tools"]),
    readCtl<CommandsData>(["commands"]),
  ]);
  return { status, doctor, mcp, tools, commands };
}

/** True when nothing could be read: the bundled toolportctl is missing or does not start. */
export function bridgeDown(overview: Overview): boolean {
  return Object.values(overview).every((section: Section<unknown>) => section.error);
}

export function useOverview() {
  const [overview, setOverview] = useState<Overview | null>(null);
  const [tick, setTick] = useState(0);
  const [refreshing, setRefreshing] = useState(false);

  useEffect(() => {
    let alive = true;
    void loadOverview().then((next) => {
      if (!alive) return;
      setOverview(next);
      setRefreshing(false);
    });
    return () => {
      alive = false;
    };
  }, [tick]);

  const reload = useCallback(() => {
    setRefreshing(true);
    setTick((value) => value + 1);
  }, []);

  return { overview, refreshing, reload };
}
