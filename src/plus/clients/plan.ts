import { useEffect, useEffectEvent, useState } from "react";
import { errorText } from "../settings/useCtl";
import type { DirectOutcome } from "./data";

export interface PlanState {
  plan: DirectOutcome | null;
  error: string | null;
  loading: boolean;
}

const IDLE: PlanState = { plan: null, error: null, loading: false };
const LOADING: PlanState = { plan: null, error: null, loading: true };

interface Settled {
  id: string;
  plan: DirectOutcome | null;
  error: string | null;
}

/** Plans a write again whenever `key` or `retry` changes; `null` for `load` plans nothing. An
 * older, slower answer never replaces a newer one, and a plan is never shown for another key. */
export function usePlan(
  load: (() => Promise<DirectOutcome>) | null,
  key: string,
  retry = 0,
): PlanState {
  const id = `${key}#${retry}`;
  const [settled, setSettled] = useState<Settled | null>(null);
  const run = useEffectEvent(() => load?.());

  useEffect(() => {
    const pending = run();
    if (!pending) return;
    let alive = true;
    pending.then(
      (plan) => alive && setSettled({ id, plan, error: null }),
      (error: unknown) =>
        alive && setSettled({ id, plan: null, error: errorText(error) }),
    );
    return () => {
      alive = false;
    };
  }, [id]);

  if (!load) return IDLE;
  if (settled?.id !== id) return LOADING;
  return { plan: settled.plan, error: settled.error, loading: false };
}

/** Whether the plan changes anything; an unchanged one leaves nothing to confirm. */
export const changes = (plan: DirectOutcome | null) =>
  plan !== null && plan.action !== "unchanged" && plan.action !== "absent";
