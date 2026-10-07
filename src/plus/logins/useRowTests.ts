import { useCallback, useState } from "react";
import { ctlData } from "../bridge/ctl";
import { errorText } from "../ui";
import { testResultOf, type RowTest } from "./model";

export interface RowTestsControl {
  tests: Record<string, RowTest>;
  running: boolean;
  /** Runs a forced probe of one server and keeps its result for the row. */
  run: (server: string) => Promise<void>;
}

/** `auth probe --server <id> --force`, once per row: each Test keeps its own result, and the
 * lists are read again when it is done. A probe only reads, so it runs at once. */
export function useRowTests(onSettled: () => void): RowTestsControl {
  const [tests, setTests] = useState<Record<string, RowTest>>({});
  const run = useCallback(
    async (server: string) => {
      setTests((all) => ({ ...all, [server]: { phase: "running" } }));
      let outcome: RowTest;
      try {
        const data = await ctlData<Parameters<typeof testResultOf>[0]>([
          "auth",
          "probe",
          "--server",
          server,
          "--force",
        ]);
        outcome = testResultOf(data, server, Math.floor(Date.now() / 1000));
      } catch (error) {
        outcome = { phase: "failed", message: errorText(error).message };
      }
      setTests((all) => ({ ...all, [server]: outcome }));
      onSettled();
    },
    [onSettled],
  );
  const running = Object.values(tests).some((test) => test.phase === "running");
  return { tests, running, run };
}
