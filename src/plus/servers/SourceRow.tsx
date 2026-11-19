import { lazy, Suspense, useState } from "react";
import { CircleHelp, Loader2, Package } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { checkNow } from "../attention/attentionCtl";
import { sourceRows } from "./commands";
import type { SourceRowData } from "./data";
import { shortVersion, since } from "./format";
import { OperationRow } from "./OperationRow";
import { useLoad } from "./useLoad";

const PackageUpdateDialog = lazy(() => import("./PackageUpdateDialog"));

const KIND: Record<string, string> = {
  npm: "npm",
  pypi: "PyPI",
  docker: "Docker image",
  github: "GitHub release",
};

function checked(row: SourceRowData): string {
  if (!row.checkedAt) return "not checked yet";
  return `checked ${since(Date.parse(row.checkedAt))}`;
}

function describe(row: SourceRowData): {
  sub: string;
  warn: boolean;
  badge?: string;
} {
  const what =
    `${KIND[row.kind] ?? row.kind} ${row.package ?? ""}${row.tracks ? `:${row.tracks}` : ""} ${shortVersion(row.pinned ?? "")}`.trim();
  switch (row.state) {
    case "update_available":
      return {
        sub: `${what} · ${shortVersion(row.latest ?? "")} is out${row.manual ? `. Toolport cannot update it itself: ${row.manual}` : ""}`,
        warn: false,
        badge: "Update available",
      };
    case "up_to_date":
      return {
        sub: `${what} · up to date, ${checked(row)}`,
        warn: false,
        badge: "Up to date",
      };
    case "unchecked":
      return { sub: `${what} · not checked yet`, warn: false };
    case "error":
      return { sub: `${what} · the last check failed: ${row.error ?? ""}`, warn: true };
    case "automatic":
      return {
        sub: `${KIND[row.kind] ?? row.kind} · fetches the newest version on each start; nothing to apply`,
        warn: false,
        badge: "Automatic",
      };
    default:
      return { sub: "No update source is known for this server", warn: false };
  }
}

/** A remote server has no code of ours to update: the provider does it. */
export function RemoteSourceRow({ url }: { url: string }) {
  const where = (() => {
    try {
      const { host, pathname } = new URL(url);
      return `${host}${pathname === "/" ? "" : pathname}`;
    } catch {
      return url;
    }
  })();
  return (
    <OperationRow
      icon={<CircleHelp />}
      title="Source and updates"
      sub={`Remote · ${where} · the provider updates it`}
      badge={<Badge variant="off">Nothing to check</Badge>}
    />
  );
}

/** Where a server's code comes from and whether a newer version is known: the Source row of the
 * Operations list. A checkout has its own Repo row, so this one stays out for git servers. */
export default function SourceRow({ serverId }: { serverId: string }) {
  const sources = useLoad(() => sourceRows(serverId).catch(() => null), serverId);
  const [review, setReview] = useState(false);
  const [checking, setChecking] = useState(false);
  const row = sources.data?.sources[0];
  if (!row || row.kind === "git") return null;
  const { sub, warn, badge } = describe(row);
  const pinned = row.state !== "automatic" && row.state !== "unknown";

  async function check() {
    setChecking(true);
    try {
      await checkNow();
    } finally {
      setChecking(false);
      sources.reload();
    }
  }

  return (
    <>
      <OperationRow
        icon={pinned ? <Package /> : <CircleHelp />}
        title="Source and updates"
        tone={warn || row.state === "update_available" ? "warning" : undefined}
        sub={sub}
        badge={
          badge ? (
            <Badge variant={row.state === "update_available" ? "warning" : "off"}>
              {badge}
            </Badge>
          ) : undefined
        }
        actions={
          <>
            {row.state === "update_available" && !row.manual && (
              <Button size="sm" onClick={() => setReview(true)}>
                Review update
              </Button>
            )}
            {pinned && (
              <Button
                size="sm"
                variant="outline"
                disabled={checking}
                onClick={() => void check()}
              >
                {checking && <Loader2 className="animate-spin" />} Check
              </Button>
            )}
          </>
        }
      />
      {review && (
        <Suspense fallback={null}>
          <PackageUpdateDialog
            serverName={row.server}
            open={review}
            onOpenChange={setReview}
            onChanged={sources.reload}
          />
        </Suspense>
      )}
    </>
  );
}
