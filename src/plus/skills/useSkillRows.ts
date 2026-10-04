import { useCallback, useEffect, useState } from "react";
import { runCtl } from "../bridge/ctl";
import type { SkillsLsData } from "../bridge/data";
import type { CtlQuery } from "../ui";
import { errorText } from "../ui";
import type { SkillRow } from "./model";

export interface RowsQuery {
  status: CtlQuery<unknown>["status"];
  rows: SkillRow[];
  /** A source stopped at its budget: the list is not the whole source. */
  skipped: Array<{ source: string; detector: string; reason: string }>;
  /** Sources whose list failed, with the CLI's words. */
  failed: Array<{ source: string; message: string }>;
  reload: () => void;
}

interface Part {
  source: string;
  data: SkillsLsData | null;
  error: string | null;
}

/** The rows of the source filter. `library` is the plain `skills ls` the screen already runs;
 * a source id is `skills ls --source <id>`; `all` is the library plus every other source. */
export function useSkillRows(
  filter: string,
  sourceIds: string[],
  library: CtlQuery<SkillsLsData>,
): RowsQuery {
  const others = filter === "library" ? [] : filter === "all" ? sourceIds : [filter];
  const key = JSON.stringify(others);
  const [tick, setTick] = useState(0);
  const [done, setDone] = useState<{ key: string; tick: number; parts: Part[] } | null>(
    null,
  );

  useEffect(() => {
    let alive = true;
    const ids = JSON.parse(key) as string[];
    void Promise.all(
      ids.map(async (source): Promise<Part> => {
        try {
          const result = await runCtl<SkillsLsData>(["skills", "ls", "--source", source])
            .result;
          const data = result.envelope?.ok ? (result.envelope.data ?? null) : null;
          return {
            source,
            data,
            error: data
              ? null
              : (result.envelope?.error?.message ?? result.parseError ?? "failed"),
          };
        } catch (error) {
          return { source, data: null, error: errorText(error).message };
        }
      }),
    ).then((parts) => alive && setDone({ key, tick, parts }));
    return () => {
      alive = false;
    };
  }, [key, tick]);

  const reload = useCallback(() => {
    setTick((n) => n + 1);
    library.reload();
  }, [library]);

  const loaded = others.length === 0 || (done?.key === key && done.tick === tick);
  const parts = loaded && others.length > 0 ? (done?.parts ?? []) : [];
  const withLibrary = filter === "library" || filter === "all";
  const status =
    withLibrary && library.status === "loading"
      ? "loading"
      : !loaded
        ? "loading"
        : withLibrary && library.status === "error" && !library.data
          ? "error"
          : "ready";
  return {
    status,
    rows: [
      ...(withLibrary ? (library.data?.skills ?? []) : []),
      ...parts.flatMap((part) => part.data?.skills ?? []),
    ],
    skipped: parts.flatMap((part) =>
      part.data?.partial
        ? (part.data.skipped ?? []).map((s) => ({ source: part.source, ...s }))
        : [],
    ),
    failed: parts
      .filter((part) => part.error)
      .map((part) => ({ source: part.source, message: part.error ?? "" })),
    reload,
  };
}
