import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";
import { resetPlusFixture } from "../fixtures/ipc";
import { PROJECT_WEB, SPEND_NOW } from "../fixtures/spendWorld";
import { invokeLog, ranArgv } from "../servers/testkit";
import { setClock } from "../tasks/clock";
import { NO_FILTER, type Filter } from "./model";
import type { RangeId } from "./data";
import { useSpend } from "./useSpend";

vi.mock("@tauri-apps/api/core", async () => {
  const { plusFixtureIpc } = await import("../fixtures/ipc");
  const { invokeLog } = await import("../servers/testkit");
  return {
    invoke: async (command: string, args: Record<string, unknown>) => {
      invokeLog.push({ command, args });
      return plusFixtureIpc(command, args);
    },
  };
});
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));

beforeEach(() => {
  invokeLog.length = 0;
  setClock(SPEND_NOW);
});

afterEach(() => setClock(null));

describe("useSpend", () => {
  it("test01_a_folder_filter_asks_for_that_folder_and_keeps_the_whole_for_the_share", async () => {
    resetPlusFixture({ spend: {} });
    const { result, rerender } = renderHook(
      ({ range, filter }: { range: RangeId; filter: Filter }) => useSpend(range, filter),
      { initialProps: { range: "7d" as RangeId, filter: NO_FILTER } },
    );
    await waitFor(() => expect(result.current.summary).not.toBeNull());
    const whole = result.current.summary?.totals.cost.usd;
    rerender({ range: "7d", filter: { project: PROJECT_WEB } });
    await waitFor(() => expect(result.current.summary?.project).toBe(PROJECT_WEB));
    expect(
      ranArgv().some((a) => a.includes("--project") && a.includes(PROJECT_WEB)),
    ).toBe(true);
    expect(result.current.whole?.totals.cost.usd).toBe(whole);
    expect(result.current.summary?.totals.cost.usd).toBeLessThan(whole ?? 0);
  });

  it("test02_changing_the_range_reads_the_report_again", async () => {
    resetPlusFixture({ spend: {} });
    const { result, rerender } = renderHook(
      ({ range }: { range: RangeId }) => useSpend(range, NO_FILTER),
      { initialProps: { range: "7d" as RangeId } },
    );
    await waitFor(() => expect(result.current.summary?.range).toBe("7d"));
    rerender({ range: "30d" });
    await waitFor(() => expect(result.current.summary?.range).toBe("30d"));
  });

  it("test03_a_failed_status_is_the_page_error_and_a_failed_report_is_not", async () => {
    resetPlusFixture({ spend: { summaryError: "unreadable" } });
    const { result } = renderHook(() => useSpend("7d", NO_FILTER));
    await waitFor(() => expect(result.current.summaryError).toBe("unreadable"));
    expect(result.current.error).toBeNull();
    expect(result.current.status).not.toBeNull();
  });

  it("test04_cancel_ends_the_computing_state_and_pauses_refreshing", async () => {
    resetPlusFixture({ spend: { scenario: "computing", scanMs: 600_000 } });
    const { result } = renderHook(() => useSpend("7d", NO_FILTER));
    await waitFor(() => expect(result.current.state).toBe("computing"));
    act(() => result.current.cancel());
    await waitFor(() => expect(result.current.state).toBe("offWithTranscripts"));
    expect(result.current.cancelled).toBe(true);
  });
});
