import { useEffect, useState } from "react";
import { plusWhatLoads } from "./api";
import { FolderProfilesSection } from "./FolderProfiles";
import type { LoadItem, WhatLoads as WhatLoadsData } from "./api";

const kindLabel: Record<string, string> = {
  memory: "Memory",
  rule: "Rules",
  skill: "Skills",
  mcp: "MCP servers",
  settings: "Settings",
};

function groupByKind(items: LoadItem[]): [string, LoadItem[]][] {
  const groups = new Map<string, LoadItem[]>();
  for (const item of items) {
    const list = groups.get(item.kind) ?? [];
    list.push(item);
    groups.set(item.kind, list);
  }
  return [...groups.entries()];
}

export function WhatLoads({ data }: { data: WhatLoadsData }) {
  return (
    <section aria-label="What loads" className="flex flex-col gap-3">
      <header className="flex items-baseline gap-2">
        <h2 className="text-sm font-medium">What loads</h2>
        <span className="text-xs text-muted-foreground">
          {data.profile ?? "default"} in {data.cwd}
        </span>
        <span className="ml-auto text-xs tabular-nums" data-testid="total-tokens">
          ~{data.total_tokens} tokens
        </span>
      </header>
      {groupByKind(data.items).map(([kind, items]) => (
        <ul
          key={kind}
          aria-label={kindLabel[kind] ?? kind}
          className="divide-y rounded-md border"
        >
          {items.map((item) => (
            <li
              key={`${item.name}:${item.path ?? item.source}`}
              data-loaded={item.loaded}
              className="flex items-center gap-3 px-3 py-1.5"
            >
              <span className="font-mono text-sm">{item.name}</span>
              <span className="text-xs text-muted-foreground">
                {item.loaded ? "Loads" : "Skipped"}: {item.reason} ({item.source})
              </span>
              <span className="ml-auto text-xs tabular-nums">{item.tokens}</span>
            </li>
          ))}
        </ul>
      ))}
      {data.clobbers.length > 0 && (
        <ul aria-label="Overrides" className="text-xs text-muted-foreground">
          {data.clobbers.map((c) => (
            <li key={`${c.kind}:${c.key}`}>
              {c.key}: {c.winner} {c.relation} {c.overridden.join(", ")}
            </li>
          ))}
        </ul>
      )}
      {data.notes.map((note) => (
        <p key={note} className="text-xs text-muted-foreground">
          {note}
        </p>
      ))}
    </section>
  );
}

export function WhatLoadsPanel() {
  const [data, setData] = useState<WhatLoadsData | null>(null);
  useEffect(() => {
    let alive = true;
    plusWhatLoads()
      .then((d) => alive && setData(d))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, []);
  return data ? (
    <div className="flex flex-col gap-4">
      <WhatLoads data={data} />
      <FolderProfilesSection />
    </div>
  ) : null;
}
