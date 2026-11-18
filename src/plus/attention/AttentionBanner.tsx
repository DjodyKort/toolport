import { useState } from "react";
import { CircleAlert, Eye } from "lucide-react";
import { Button } from "@/components/ui/button";
import { StatusCard } from "@/components/ui/status-card";
import { toastError } from "@/lib/toast";
import { ctlData } from "../bridge/ctl";
import { useAttentionSnapshot, type AttentionItem } from "../shell/attentionFeed";
import { refreshAttention } from "./attentionCtl";
import { RowMenu } from "./RowMenu";

type Page = "servers" | "tasks" | "library";

function Fix({ item }: { item: AttentionItem }) {
  const [busy, setBusy] = useState(false);
  const command = item.action?.command;
  if (!command || !item.action) return null;
  return (
    <Button
      size="sm"
      disabled={busy}
      onClick={async () => {
        setBusy(true);
        try {
          await ctlData(command);
          refreshAttention();
        } catch (error) {
          toastError(`${item.action?.label ?? "The fix"} did not work`, {
            description: error instanceof Error ? error.message : String(error),
          });
        } finally {
          setBusy(false);
        }
      }}
    >
      {item.action.label}
    </Button>
  );
}

/** The rows of the attention list that belong to one page, on that page: each needs-you row with
 * its fix, the rest as one line. They are the bell's rows, so snoozing one hides it in both.
 * `except` names the id prefixes the page already draws itself. */
export function AttentionBanner({
  page,
  except = [],
  className = "",
}: {
  page: Page;
  except?: string[];
  className?: string;
}) {
  const snapshot = useAttentionSnapshot();
  const rows = (snapshot?.items ?? []).filter(
    (item) =>
      item.action?.view === page && !except.some((prefix) => item.id.startsWith(prefix)),
  );
  const urgent = rows.filter((item) => item.urgency === "needs-you");
  const other = rows.filter((item) => item.urgency !== "needs-you");
  if (rows.length === 0) return null;
  return (
    <div className={`flex flex-col gap-2 ${className}`} data-plus="attention-banner">
      {urgent.map((item) => (
        <StatusCard
          key={item.id}
          variant="danger"
          icon={<CircleAlert />}
          title={item.title}
          detail={
            <>
              {item.detail}{" "}
              <span className="text-warning">Attention · {item.source}</span>
            </>
          }
          actions={
            <>
              <Fix item={item} />
              <RowMenu item={item} />
            </>
          }
        />
      ))}
      {other.length > 0 && (
        <StatusCard
          variant="warning"
          icon={<Eye />}
          title={
            other.length === 1
              ? other[0].title
              : `${other.length} things are worth a look`
          }
          detail={
            <>
              {other.length === 1
                ? other[0].detail
                : other.map((item) => item.title).join("; ")}{" "}
              <span className="text-warning">Attention · {other[0].source}</span>
            </>
          }
          actions={
            <>
              {other.length === 1 && <Fix item={other[0]} />}
              {other.length === 1 && <RowMenu item={other[0]} />}
            </>
          }
        />
      )}
    </div>
  );
}
