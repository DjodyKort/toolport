import { useEffect, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { SectionHeader } from "@/components/ui/section-header";
import { ctlData } from "../bridge/ctl";
import { errorText } from "../settings/useCtl";
import { whenText } from "./syncModel";

interface Head {
  server: string;
  branch: string;
  head: string;
  pushed: boolean;
}

export interface ReposData {
  publishing: boolean;
  machine: string;
  mine: Array<Head & { unpublished: number }>;
  others: Array<{
    machine: string;
    madeAt: string;
    heads: Array<Head & { relation: string; sameBranch: boolean }>;
  }>;
}

const RELATION: Record<string, string> = {
  same: "same as here",
  newer: "ahead of this machine",
  older: "behind this machine",
  diverged: "diverged from this machine",
  unknown: "not fetched here yet",
  not_here: "not on this machine",
};

const short = (id: string) => id.slice(0, 8);

/** The server checkouts this machine publishes and the ones the other machines published, as the
 * last sync pull read them. Reads local files and git only. */
export default function RepoHeads({ active }: { active: boolean }) {
  const [data, setData] = useState<ReposData | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!active) return;
    void ctlData<ReposData>(["sync", "repos"]).then(setData, (e) =>
      setError(errorText(e)),
    );
  }, [active]);

  if (!active) {
    return (
      <section aria-label="Repos">
        <SectionHeader>Repos</SectionHeader>
        <p className="text-meta text-muted-foreground">
          This machine does not publish its server checkouts. Switch on Server
          repositories above to see where each machine stands.
        </p>
      </section>
    );
  }
  return (
    <section aria-label="Repos">
      <SectionHeader count={data ? data.mine.length : undefined}>Repos</SectionHeader>
      {error && (
        <p role="alert" className="text-meta text-destructive">
          {error}
        </p>
      )}
      {data && (
        <ul className="toolport-list">
          {data.mine.length === 0 && data.others.length === 0 && (
            <li className="px-4 py-3 text-meta text-muted-foreground">
              No server runs from a git checkout yet.
            </li>
          )}
          {data.mine.map((head) => (
            <li key={`mine-${head.server}`} className="flex items-center gap-3 px-4 py-3">
              <div className="min-w-0 flex-1">
                <p className="text-sm font-semibold">{head.server}</p>
                <p className="text-meta text-muted-foreground">
                  This machine · {head.branch} at {short(head.head)}
                  {head.unpublished > 0 &&
                    ` · ${head.unpublished} commit${head.unpublished === 1 ? "" : "s"} on no remote, so ${data.others[0]?.machine ?? "your other machine"} cannot see them`}
                </p>
              </div>
              {!head.pushed && <Badge variant="partial">Not pushed</Badge>}
            </li>
          ))}
          {data.others.flatMap((other) =>
            other.heads.map((head) => (
              <li
                key={`${other.machine}-${head.server}`}
                className="flex items-center gap-3 px-4 py-3"
              >
                <div className="min-w-0 flex-1">
                  <p className="text-sm font-semibold">{head.server}</p>
                  <p className="text-meta text-muted-foreground">
                    {other.machine} · {head.branch} at {short(head.head)} · published{" "}
                    {whenText(other.madeAt)}
                  </p>
                </div>
                {!head.pushed && <Badge variant="partial">Not pushed</Badge>}
                <Badge variant={head.relation === "same" ? "success" : "outline"}>
                  {!head.sameBranch && head.relation !== "not_here"
                    ? "other branch"
                    : (RELATION[head.relation] ?? head.relation)}
                </Badge>
              </li>
            )),
          )}
        </ul>
      )}
    </section>
  );
}
