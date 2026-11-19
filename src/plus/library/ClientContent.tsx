import { useState } from "react";
import { ChevronRight } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { SkeletonRows } from "@/components/ui/skeleton";
import { cn } from "@/lib/utils";
import { useLoad } from "../servers/useLoad";
import { agentStatus, styleStatus } from "./commands";
import {
  CONTENT_KINDS,
  clientContent,
  missing,
  total,
  type ClientContentRow,
  type ContentKindName,
} from "./contentPerClient";
import type { StatusData } from "./data";
import { plural } from "./model";

const LABEL: Record<ContentKindName, string> = {
  skills: "Skills",
  rules: "Rules",
  agents: "Agents",
  styles: "Styles",
};

function Names({ row }: { row: ClientContentRow }) {
  return (
    <dl className="grid gap-x-3 gap-y-1 text-meta sm:grid-cols-[6rem_minmax(0,1fr)]">
      {CONTENT_KINDS.filter((kind) => row.items[kind].length > 0).map((kind) => (
        <div key={kind} className="contents">
          <dt className="text-muted-foreground">{LABEL[kind]}</dt>
          <dd className="min-w-0 break-words">
            {row.items[kind].map((item, index) => (
              <span key={item.name}>
                {index > 0 && ", "}
                {item.name}
                {!item.present && <span className="text-warning"> (file missing)</span>}
              </span>
            ))}
          </dd>
        </div>
      ))}
    </dl>
  );
}

/** What each client holds of the library: skills, rules, agents and output styles, one row per
 * client with the counts and, one click down, the names. Read only: it shows the lock and the
 * files it recorded, and the plan above is where a change is made. */
export default function ClientContent({ skills }: { skills: StatusData | null }) {
  const agents = useLoad(agentStatus, "client-content-agents");
  const styles = useLoad(styleStatus, "client-content-styles");
  const [open, setOpen] = useState<string | null>(null);

  const failed = (agents.error && !agents.data) || (styles.error && !styles.data);
  const loading = !failed && (!agents.data || !styles.data);
  const rows = clientContent(skills, agents.data, styles.data);
  const lines = rows.filter(
    (row) => total(row) > 0 || skills?.targetedClients.includes(row.client),
  );

  return (
    <section aria-label="Installed per client" className="flex flex-col gap-2">
      <h2 className="text-meta font-semibold tracking-wide text-muted-foreground uppercase">
        Installed per client
      </h2>
      {failed && (
        <div role="alert" className="flex flex-wrap items-center gap-3 text-meta">
          <span className="text-destructive">
            Could not read what each client has: {agents.error ?? styles.error}
          </span>
          <Button
            variant="outline"
            size="xs"
            onClick={() => {
              agents.reload();
              styles.reload();
            }}
          >
            Try again
          </Button>
        </div>
      )}
      {loading && <SkeletonRows label="Reading what each client has" />}
      {!failed && !loading && lines.length === 0 && (
        <p className="rounded-xl border bg-card px-4 py-4 text-meta text-muted-foreground">
          No client holds anything of your library yet. Apply the plan to deliver it.
        </p>
      )}
      {!failed && !loading && lines.length > 0 && (
        <div className="overflow-x-auto rounded-xl border bg-card">
          <table className="w-full text-left text-sm">
            <caption className="sr-only">
              Skills, rules, agents and styles installed in each client
            </caption>
            <thead>
              <tr className="border-b text-meta text-muted-foreground">
                <th scope="col" className="px-3 py-2 font-medium">
                  Client
                </th>
                {CONTENT_KINDS.map((kind) => (
                  <th
                    key={kind}
                    scope="col"
                    className="px-1.5 py-2 text-right text-xs font-medium"
                  >
                    {LABEL[kind]}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {lines.map((row) => {
                const shown = open === row.client;
                const gone = missing(row);
                return (
                  <ClientLine
                    key={row.client}
                    row={row}
                    shown={shown}
                    gone={gone}
                    onToggle={() => setOpen(shown ? null : row.client)}
                  />
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

function ClientLine({
  row,
  shown,
  gone,
  onToggle,
}: {
  row: ClientContentRow;
  shown: boolean;
  gone: number;
  onToggle: () => void;
}) {
  const count = total(row);
  return (
    <>
      <tr className="border-b last:border-b-0">
        <th scope="row" className="px-3 py-2 font-medium">
          <button
            type="button"
            aria-expanded={shown}
            aria-label={`${row.label}, ${plural(count, "item")}`}
            disabled={count === 0}
            onClick={onToggle}
            className="flex items-center gap-1.5 text-left disabled:opacity-100"
          >
            <ChevronRight
              aria-hidden
              className={cn(
                "size-4 shrink-0 text-muted-foreground transition-transform",
                shown && "rotate-90",
                count === 0 && "invisible",
              )}
            />
            {row.label}
            {gone > 0 && <Badge variant="warning">{gone} missing</Badge>}
          </button>
        </th>
        {CONTENT_KINDS.map((kind) => (
          <td key={kind} className="px-1.5 py-2 text-right tabular-nums">
            {row.items[kind].length}
          </td>
        ))}
      </tr>
      {shown && (
        <tr className="border-b bg-muted/30 last:border-b-0">
          <td colSpan={CONTENT_KINDS.length + 1} className="px-4 py-3">
            <Names row={row} />
          </td>
        </tr>
      )}
    </>
  );
}
