import { useState } from "react";
import { ListTree, TriangleAlert } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import type { SourceItem, SourceRow, SourcesLsData } from "../../bridge/data";
import { AsyncView } from "../../ui";
import { useRead, type WriteControl } from "../hooks";
import { plural } from "../model";
import { PathLine, SourceBadge } from "../parts";
import { LibraryPanel } from "./LibraryPanel";
import {
  countsText,
  FOUND_BY,
  isGitTree,
  ITEM_NOUN,
  MISSING_ACTIONS,
  ownerLabel,
  plain,
  STATE_LABEL,
  STATE_TONE,
  timeText,
  tokenText,
} from "./model";
import { Row } from "./Row";

const AUDIT_TONE = {
  clean: "success",
  warn: "warning",
  high: "destructive",
  unchecked: "secondary",
} as const;

function ItemRow({ item }: { item: SourceItem }) {
  return (
    <li className="flex flex-col gap-1 rounded-md border px-3 py-2 text-sm">
      <span className="flex flex-wrap items-center gap-2">
        <b className="break-all">{item.name}</b>
        <Badge variant="outline">{ITEM_NOUN[item.kind]}</Badge>
        {item.lazy && <Badge variant="secondary">loads on demand</Badge>}
        <Badge variant={AUDIT_TONE[item.audit]}>
          {item.audit === "unchecked" ? "not audited" : `audit ${item.audit}`}
        </Badge>
        <span className="ml-auto text-xs text-muted-foreground">
          {tokenText(item.tokens)}
        </span>
      </span>
      <PathLine path={plain(item.path)} />
      {item.shadowedBy && (
        <span className="text-xs text-warning">
          Shares its name with {plain(item.shadowedBy)}; one of them wins and both are
          left alone.
        </span>
      )}
    </li>
  );
}

/** The items of one source, read only when asked for: `sources ls --source <id> --items`. Only
 * names, paths and sizes come back, never file contents. */
function Items({ id }: { id: string }) {
  const query = useRead<SourcesLsData>(["sources", "ls", "--source", id, "--items"]);
  return (
    <AsyncView
      query={query}
      errorTitle="Couldn't list the items of this source"
      isEmpty={(data) => (data.items ?? []).length === 0}
      empty={
        <p className="text-sm text-muted-foreground">Nothing found in this source.</p>
      }
      skeleton={
        <p role="status" className="text-sm text-muted-foreground">
          Reading the items…
        </p>
      }
    >
      {(data) => (
        <ul aria-label="Items" className="flex flex-col gap-1.5">
          {(data.items ?? [])
            .filter((item) => item.sourceId === id)
            .map((item) => (
              <ItemRow key={`${item.kind}:${item.name}:${item.path}`} item={item} />
            ))}
        </ul>
      )}
    </AsyncView>
  );
}

export function SourceDetail({ row, write }: { row: SourceRow; write: WriteControl }) {
  const [items, setItems] = useState(false);
  const missing = MISSING_ACTIONS[row.detector] ?? [];
  const fresh = row.freshness;
  const total = row.visible.skillTotal;
  return (
    <section
      aria-label={`Source ${row.origin.name}`}
      className="flex flex-col gap-4 rounded-lg border bg-card p-4"
    >
      <h4 className="flex flex-wrap items-center gap-2 text-base font-semibold">
        {row.origin.name}
        <SourceBadge origin={row.origin} />
        <Badge variant={row.writable ? "success" : "secondary"}>
          {row.writable ? "Toolport can edit" : "read-only"}
        </Badge>
      </h4>
      <dl className="grid grid-cols-[8rem_minmax(0,1fr)] gap-x-3 gap-y-2 text-sm">
        <Row label="Owner">{ownerLabel(row.owner)}</Row>
        <Row label="Where">
          {row.root === null ? (
            <span className="text-muted-foreground">no folder</span>
          ) : isGitTree(row) ? (
            <code className="font-mono text-xs">{row.root}</code>
          ) : (
            <PathLine path={plain(row.root)} />
          )}
        </Row>
        <Row label="Contains">{countsText(row.counts)}</Row>
        <Row label="Costs">
          {tokenText(row.tokens)}
          <span className="block text-xs text-muted-foreground">
            What it adds if everything loaded; a skill counts by its description only.
          </span>
        </Row>
        {total > 0 && (
          <Row label="Visible to Claude">
            <Badge variant={row.visible.skill === total ? "success" : "warning"}>
              {row.visible.skill} of {plural(total, "skill")}
            </Badge>
          </Row>
        )}
        <Row label="Found by">{FOUND_BY[row.detector]}</Row>
        <Row label="Status">
          <span className="flex flex-col gap-1">
            <Badge variant={STATE_TONE[row.status.state]} className="self-start">
              {STATE_LABEL[row.status.state]}
            </Badge>
            <span>{plain(row.status.detail)}</span>
            <span className="text-xs text-muted-foreground">
              Checked{" "}
              <time dateTime={row.status.checkedAt}>
                {timeText(row.status.checkedAt)}
              </time>
            </span>
          </span>
        </Row>
        {fresh && row.detector !== "library" && (
          <Row label="Against remote">
            <span className="flex flex-col gap-0.5">
              <span>
                <code className="font-mono text-xs">{fresh.ref}</code>: {fresh.behind}{" "}
                behind, {fresh.ahead} ahead
                {!fresh.inCheckout && " · the files are not in your checkout yet"}
              </span>
              <span className="text-xs text-muted-foreground">
                Last sync{" "}
                {fresh.lastSync ? (
                  <time dateTime={fresh.lastSync}>{timeText(fresh.lastSync)}</time>
                ) : (
                  "unknown"
                )}
              </span>
            </span>
          </Row>
        )}
        {row.managedBy && <Row label="Managed by">{plain(row.managedBy)}</Row>}
        {row.enabled !== undefined && (
          <Row label="Enabled">
            <Badge variant={row.enabled ? "success" : "secondary"}>
              {row.enabled ? "on in your settings" : "off in your settings"}
            </Badge>
          </Row>
        )}
      </dl>
      {row.detector === "library" && <LibraryPanel write={write} />}
      {row.warnings.length > 0 && (
        <Callout variant="warning" role="status">
          <ul aria-label="Warnings" className="flex flex-col gap-1">
            {row.warnings.map((warning) => (
              <li key={warning} className="flex items-start gap-1.5">
                <TriangleAlert className="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
                <span>{plain(warning)}</span>
              </li>
            ))}
          </ul>
        </Callout>
      )}
      <div className="flex flex-wrap gap-2">
        <Button
          size="sm"
          variant="outline"
          aria-expanded={items}
          onClick={() => setItems((open) => !open)}
        >
          <ListTree /> {items ? "Hide items" : "Show items"}
        </Button>
        {missing.map((action) => (
          <Button
            key={action.label}
            size="sm"
            variant="outline"
            disabled
            title={action.reason}
            aria-label={action.label}
          >
            {action.label}
          </Button>
        ))}
      </div>
      {missing.length > 0 && (
        <ul aria-label="Actions not available yet" className="flex flex-col gap-1">
          {missing.map((action) => (
            <li key={action.label} className="text-xs text-muted-foreground">
              <b>{action.label}</b>: {action.reason}
            </li>
          ))}
        </ul>
      )}
      {items && <Items id={row.id} />}
      {!row.writable && (
        <p className="text-xs text-muted-foreground">
          Toolport shows this source and never edits it.
        </p>
      )}
    </section>
  );
}
