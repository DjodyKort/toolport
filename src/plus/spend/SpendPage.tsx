import { useState } from "react";
import { CircleAlert, CircleCheck, Info, TriangleAlert } from "lucide-react";
import { Button } from "@/components/ui/button";
import { SkeletonRows } from "@/components/ui/skeleton";
import { StatusCard } from "@/components/ui/status-card";
import { now } from "../tasks/clock";
import type { RangeId } from "./data";
import { DEFAULT_RANGE, NO_FILTER, notices, progress, strip, type Tone } from "./model";
import { useSpend } from "./useSpend";

const ICON = {
  ok: <CircleCheck />,
  info: <Info />,
  warning: <TriangleAlert />,
  danger: <CircleAlert />,
  off: <Info />,
} as const;

const VARIANT: Record<Tone, "success" | "info" | "warning" | "danger"> = {
  ok: "success",
  info: "info",
  off: "info",
  warning: "warning",
  danger: "danger",
};

/** The Spend page. This is the data layer's page: it shows the state of collecting from the real
 * commands; the report cards arrive with the page components. */
export default function SpendPage() {
  const [range] = useState<RangeId>(DEFAULT_RANGE);
  const load = useSpend(range, NO_FILTER);
  const { status, state } = load;

  if (load.error)
    return (
      <StatusCard
        variant="danger"
        icon={<CircleAlert />}
        title="Spend could not be read"
        detail={load.error}
        actions={<Button onClick={load.reload}>Retry</Button>}
      />
    );
  if (!status || !state) return <SkeletonRows label="Reading the collecting status" />;

  const bar = state === "computing" ? progress(status, load.elapsedMs) : null;
  const line = bar
    ? { tone: "info" as const, title: bar.title, detail: bar.detail }
    : strip(state, status, now());
  return (
    <div className="flex flex-col gap-3" data-state={state}>
      <StatusCard
        variant={VARIANT[line.tone]}
        icon={ICON[line.tone]}
        title={line.title}
        detail={line.detail}
        actions={
          state === "computing" ? (
            <Button onClick={load.cancel}>Cancel</Button>
          ) : undefined
        }
        testId="spend-strip"
      />
      {notices(load.summary, status, now()).map((notice) => (
        <StatusCard
          key={notice.id}
          variant={VARIANT[notice.tone]}
          icon={ICON[notice.tone]}
          title={notice.title}
          detail={notice.detail}
        />
      ))}
      {load.summaryError && (
        <StatusCard
          variant="danger"
          icon={<CircleAlert />}
          title="The report could not be read"
          detail={load.summaryError}
          actions={<Button onClick={load.reload}>Retry</Button>}
        />
      )}
    </div>
  );
}
