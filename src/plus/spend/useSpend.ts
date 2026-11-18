import { useCallback, useEffect, useRef, useState } from "react";
import { CtlError } from "../bridge/ctl";
import { errorText } from "../settings/useCtl";
import { now } from "../tasks/clock";
import { spendStatus, spendSummary } from "./commands";
import type { RangeId, StatusData, SummaryData } from "./data";
import { isFiltered, spendState, type Filter, type SpendState } from "./model";

const SLOW_MS = 30_000;
const PROGRESS_MS = 1_000;

export interface SpendLoad {
  status: StatusData | null;
  /** The report for exactly this range and filter; null while it is being read. */
  summary: SummaryData | null;
  /** The newest report read, even for another range or filter: the page keeps it on screen
   * while the new one loads. */
  shown: SummaryData | null;
  refreshing: boolean;
  /** The same range without the folder filter, for "41 of 142 sessions". */
  whole: SummaryData | null;
  state: SpendState | null;
  /** Set when `spend status` itself failed: nothing on the page can be shown. */
  error: string | null;
  /** Set when only the report failed; the status strip still shows. */
  summaryError: string | null;
  /** The person stopped the first read of the transcripts. */
  cancelled: boolean;
  elapsedMs: number;
  reload: () => void;
  cancel: () => void;
}

/** Everything the Spend page reads: the collecting status at once, the report next, and the
 * status again every second while the transcripts are read for the first time, so the page can
 * show the count. A slower, older answer never replaces a newer one. */
export function useSpend(range: RangeId, filter: Filter, version = 0): SpendLoad {
  const [status, setStatus] = useState<StatusData | null>(null);
  const [summary, setSummary] = useState<SummaryData | null>(null);
  const [whole, setWhole] = useState<SummaryData | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [summaryError, setSummaryError] = useState<string | null>(null);
  const [cancelledKey, setCancelledKey] = useState<string | null>(null);
  const [elapsedMs, setElapsedMs] = useState(0);
  const sequence = useRef(0);
  const controller = useRef<AbortController | null>(null);
  const wholeFor = useRef<RangeId | null>(null);
  const stopped = useRef(false);
  const lastKey = useRef("");
  const { project, model, task } = filter;
  const filtered = isFiltered(filter);
  const key = `${range}|${project ?? ""}|${model ?? ""}|${task ?? ""}`;

  const load = useCallback(async () => {
    const mine = ++sequence.current;
    controller.current?.abort();
    const abort = new AbortController();
    controller.current = abort;
    const current = () => mine === sequence.current;
    const started = now();
    const changed = lastKey.current !== key;
    lastKey.current = key;
    if (changed) stopped.current = false;
    let first: StatusData;
    try {
      first = await spendStatus(abort.signal);
    } catch (failure) {
      if (current()) setError(errorText(failure));
      return;
    }
    if (!current()) return;
    setError(null);
    setStatus(first);
    if (changed) {
      setSummaryError(null);
      setCancelledKey(null);
      setElapsedMs(0);
    }
    const report = spendSummary(range, { project, model, task }, abort.signal);
    const poll = setInterval(() => {
      spendStatus(abort.signal).then(
        (next) => {
          if (!current()) return;
          setStatus(next);
          setElapsedMs(now() - started);
        },
        () => {},
      );
    }, PROGRESS_MS);
    const unfiltered =
      filtered && wholeFor.current !== range
        ? spendSummary(range, {}, abort.signal).catch(() => null)
        : null;
    try {
      const data = await report;
      if (!current()) return;
      setSummary(data);
      setStatus(data.status);
      setSummaryError(null);
      if (!filtered) {
        setWhole(data);
        wholeFor.current = range;
      }
      const all = await unfiltered;
      if (current() && all) {
        setWhole(all);
        wholeFor.current = range;
      }
    } catch (failure) {
      if (!current()) return;
      if (failure instanceof CtlError && failure.code === "cancelled") return;
      setSummaryError(errorText(failure));
    } finally {
      clearInterval(poll);
    }
  }, [range, project, model, task, filtered, key]);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- Starts the first read of the status and report on mount and on each range or filter change.
    void load();
    const refresh = () => {
      if (!stopped.current) void load();
    };
    const timer = setInterval(refresh, SLOW_MS);
    const onFocus = refresh;
    window.addEventListener("focus", onFocus);
    return () => {
      sequence.current += 1;
      controller.current?.abort();
      clearInterval(timer);
      window.removeEventListener("focus", onFocus);
    };
  }, [load, version]);

  const cancel = useCallback(() => {
    stopped.current = true;
    sequence.current += 1;
    controller.current?.abort();
    setCancelledKey(lastKey.current);
  }, []);

  const cancelled = cancelledKey === key;
  const fresh =
    summary &&
    summary.range === range &&
    summary.project === project &&
    summary.model === model &&
    summary.task === task
      ? summary
      : null;
  return {
    status,
    summary: fresh,
    shown: summary,
    refreshing: summary !== null && fresh === null && !cancelled,
    whole,
    state: status ? spendState(status, fresh !== null || cancelled) : null,
    error,
    summaryError,
    cancelled,
    elapsedMs,
    reload: () => {
      lastKey.current = "";
      void load();
    },
    cancel,
  };
}
