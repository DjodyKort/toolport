import { lazy, Suspense, useState } from "react";
import { GitBranch, Loader2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { CtlError } from "../bridge/ctl";
import { plural } from "../settings/model";
import { repoCheck, repoStatus } from "./commands";
import type { RepoRowData } from "./data";
import { OperationRow } from "./OperationRow";
import { useLoad } from "./useLoad";

const RepoUpdateDialog = lazy(() => import("./RepoUpdateDialog"));

async function loadRepo(serverId: string): Promise<RepoRowData | null> {
  try {
    const { repos } = await repoStatus(serverId);
    return repos[0] ?? null;
  } catch (error) {
    if (error instanceof CtlError && error.code === "not_found") return null;
    throw error;
  }
}

function describe(repo: RepoRowData): { sub: string; warn: boolean; badge?: string } {
  const place = `${repo.branch} → ${repo.upstream}`;
  switch (repo.state) {
    case "behind":
      return {
        sub: `${place} · ${plural(repo.behind, "new commit")}`,
        warn: false,
        badge: "Update available",
      };
    case "dirty":
      return {
        sub: `${place} · ${plural(repo.behind, "new commit")}, but local changes block the update`,
        warn: true,
      };
    case "diverged":
      return {
        sub: `${place} · diverged: ${repo.ahead} local and ${repo.behind} new commits. Rebase or merge it yourself`,
        warn: true,
      };
    case "detached":
      return { sub: "HEAD is detached, so there is no branch to update", warn: false };
    case "no_upstream":
      return { sub: `${repo.branch} tracks no remote branch`, warn: false };
    case "busy":
      return { sub: "A merge or rebase is open in this checkout", warn: true };
    default:
      return {
        sub: `${place} · up to date${repo.ahead > 0 ? `, ${plural(repo.ahead, "unpushed commit")}` : ""}`,
        warn: false,
      };
  }
}

function checkedText(repo: RepoRowData): string {
  if (repo.error) return `Last check failed: ${repo.error}`;
  if (!repo.checkedAt) return "Never checked";
  const at = new Date(repo.checkedAt);
  return `Checked ${at.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`;
}

/** The checkout a local server runs from: where it stands against the branch it tracks, a fetch
 * on request and the review of the new commits. A server that runs from no checkout has no row. */
export default function RepoRow({ serverId }: { serverId: string }) {
  const repo = useLoad(() => loadRepo(serverId), serverId);
  const [checking, setChecking] = useState(false);
  const [review, setReview] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const row = repo.data;
  if (!row) return null;
  const { sub, warn, badge } = describe(row);

  async function check() {
    setChecking(true);
    setFailure(null);
    try {
      const report = await repoCheck();
      const failed = report.repos.find((r) => !r.ok);
      if (failed) setFailure(failed.error ?? "The fetch failed");
    } catch (error) {
      setFailure(error instanceof Error ? error.message : String(error));
    } finally {
      setChecking(false);
      repo.reload();
    }
  }

  return (
    <>
      <OperationRow
        icon={<GitBranch />}
        title="Repo"
        tone={warn ? "warning" : undefined}
        sub={
          <>
            {sub}
            <span className="block">
              {failure ? `The fetch failed: ${failure}` : checkedText(row)}
            </span>
            <span className="block break-all">{row.root}</span>
          </>
        }
        badge={badge ? <Badge variant="warning">{badge}</Badge> : undefined}
        actions={
          <>
            {row.state === "behind" && (
              <Button size="sm" onClick={() => setReview(true)}>
                Review update
              </Button>
            )}
            <Button
              size="sm"
              variant="outline"
              disabled={checking}
              onClick={() => void check()}
            >
              {checking && <Loader2 className="animate-spin" />} Check now
            </Button>
          </>
        }
      />
      {review && (
        <Suspense fallback={null}>
          <RepoUpdateDialog
            serverName={row.server}
            open={review}
            onOpenChange={setReview}
            onChanged={repo.reload}
          />
        </Suspense>
      )}
    </>
  );
}
