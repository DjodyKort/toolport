import { createSpendWorld, type SpendWorldOptions } from "../fixtures/spendWorld";
import type { StatusData, SummaryData } from "./data";

/** The status and the report the fixture world answers with, for component tests that take
 * their data as props. */
export function readWorld(options: SpendWorldOptions = {}, ...argv: string[]) {
  const world = createSpendWorld(options);
  const data = (words: string[]) => {
    const reply = world.run(["spend", ...words]);
    if (!reply.ok) throw new Error(reply.error?.message);
    return reply.data;
  };
  return {
    status: data(["status"]) as StatusData,
    summary: data(["summary", ...argv]) as SummaryData,
  };
}
