import { lazy, Suspense, useEffect, useState } from "react";
import { CircleAlert, Eye } from "lucide-react";
import { Button } from "@/components/ui/button";
import { StatusCard } from "@/components/ui/status-card";
import { toastError } from "@/lib/toast";
import { ctlData } from "../bridge/ctl";
import { useAttentionSnapshot, type AttentionItem } from "../shell/attentionFeed";
import { refreshAttention } from "./attentionCtl";
import { reviewedServer, type Reviewed } from "./reviewTarget";
import { ReviewDialog, ReviewUpdatesDialog } from "./reviewUpdate";
import { RowMenu } from "./RowMenu";
import { registerShownPage } from "./shownPages";

type Page = "servers" | "tasks" | "library";

function ReviewUpdate({ item, reviewed }: { item: AttentionItem; reviewed: Reviewed }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button size="sm" onClick={() => setOpen(true)}>
        {item.action?.label ?? "Review update"}
      </Button>
      {open && <ReviewDialog reviewed={reviewed} open={open} onOpenChange={setOpen} />}
    </>
  );
}

/** "Review updates": the plan of every package row of the list in one dialog. */
function ReviewAll({ servers }: { servers: string[] }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button size="sm" onClick={() => setOpen(true)}>
        Review updates
      </Button>
      {open && (
        <ReviewUpdatesDialog
          servers={servers}
          open={open}
          onOpenChange={setOpen}
          onChanged={refreshAttention}
        />
      )}
    </>
  );
}

/** The servers whose row's button reviews a package or release update. */
function packageServers(items: AttentionItem[]): string[] {
  return items.flatMap((item) => {
    const reviewed = item.action?.command ? reviewedServer(item.action.command) : null;
    return reviewed?.verb === "package" ? [reviewed.server] : [];
  });
}

function Fix({ item }: { item: AttentionItem }) {
  const [busy, setBusy] = useState(false);
  const command = item.action?.command;
  if (!command || !item.action) return null;
  const reviewed = reviewedServer(command);
  if (reviewed) return <ReviewUpdate item={item} reviewed={reviewed} />;
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
  useEffect(() => registerShownPage(page), [page]);
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
              {packageServers(other).length > 1 && (
                <ReviewAll servers={packageServers(other)} />
              )}
              {other.length === 1 && <RowMenu item={other[0]} />}
            </>
          }
        />
      )}
    </div>
  );
}
