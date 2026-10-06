import { useEffect } from "react";
import { Badge } from "@/components/ui/badge";
import type { ContextComposeData } from "../types/context-layers";
import { useRead } from "./hooks";
import { formatTokens } from "./model";
import { basisWord } from "./stackModel";
import { Code, QuerySection } from "./parts";

/** What `context compose` returns for a folder: the instruction files in the order Claude
 * builds them. The text is shown as returned: instruction files hold no secrets by rule, and
 * nothing else of the folder is read. */
function folderName(dir: string): string {
  return dir.split(/[\\/]/).filter(Boolean).at(-1) ?? dir;
}

export function ComposedView({
  cwd,
  title = "Composed instructions",
  version = 0,
}: {
  cwd: string | null;
  title?: string;
  version?: number;
}) {
  return cwd ? <Composed cwd={cwd} title={title} version={version} /> : null;
}

function Composed({
  cwd,
  title,
  version,
}: {
  cwd: string;
  title: string;
  version: number;
}) {
  const query = useRead<ContextComposeData>(["context", "compose", "--cwd", cwd]);
  const { reload } = query;
  useEffect(() => {
    if (version > 0) reload();
  }, [version, reload]);
  return (
    <QuerySection
      title={title}
      hint="The instruction files Claude reads at the start in this folder, in load order."
      query={query}
      isEmpty={(data) => data.parts.length === 0}
      empty={<p className="text-sm text-muted-foreground">No instruction files here.</p>}
    >
      {(data) => (
        <div className="flex flex-col gap-3">
          <p className="text-sm">
            <b className="tabular-nums">{formatTokens(data.total.value)}</b> tokens in
            total, {basisWord(data.total.basis)}.
          </p>
          {data.levels.length > 0 && (
            <div className="flex flex-col gap-1 text-xs">
              <p className="text-muted-foreground">
                {`Folder levels Claude walks, from ${data.levels[0].dir} down:`}
              </p>
              <ol aria-label="Folder levels" className="flex flex-col gap-0.5">
                {data.levels.map((level, depth) => (
                  <li
                    key={level.dir}
                    title={level.dir}
                    className="flex flex-wrap items-baseline gap-2"
                    style={{ paddingLeft: `${depth}rem` }}
                  >
                    <span className="font-mono">{folderName(level.dir)}</span>
                    <span className="text-muted-foreground">
                      {level.files.length > 0 ? level.files.join(", ") : "nothing here"}
                    </span>
                  </li>
                ))}
              </ol>
            </div>
          )}
          <ol
            aria-label="Composed parts"
            className="flex flex-col divide-y rounded-lg border"
          >
            {data.parts.map((part, index) => (
              <li key={`${part.path}:${index}`} className="flex flex-col gap-1 px-3 py-2">
                <div className="flex flex-wrap items-center gap-2 text-sm">
                  <b>{part.name}</b>
                  {(part.origin.kind === "org" || part.origin.kind === "policy") && (
                    <Badge>{part.origin.kind === "org" ? "org" : "managed policy"}</Badge>
                  )}
                  <Badge variant="secondary">{part.origin.name}</Badge>
                  {part.lazy && <Badge variant="outline">on demand</Badge>}
                  {part.layers.length > 0 && (
                    <span className="text-xs text-muted-foreground">
                      layers: {part.layers.join(", ")}
                    </span>
                  )}
                  <span className="ml-auto text-xs tabular-nums text-muted-foreground">
                    {formatTokens(part.tokens.value)} tokens,{" "}
                    {basisWord(part.tokens.basis)}
                  </span>
                </div>
                <Code>{part.path}</Code>
                {part.via.length > 0 && (
                  <p className="text-xs text-muted-foreground">
                    imported through {part.via.join(" then ")}
                  </p>
                )}
                <details>
                  <summary className="cursor-pointer text-xs text-muted-foreground">
                    Show the text
                  </summary>
                  <pre className="mt-1 max-h-64 overflow-auto rounded bg-muted p-2 font-mono text-xs whitespace-pre-wrap">
                    {part.text}
                  </pre>
                </details>
              </li>
            ))}
          </ol>
          {data.skipped.length > 0 && (
            <ul className="list-disc pl-4 text-xs text-muted-foreground">
              {data.skipped.map((skip, index) => (
                <li key={index}>
                  {skip.path ? `${skip.path}: ` : ""}
                  {skip.reason}
                </li>
              ))}
            </ul>
          )}
          {data.notes.map((note) => (
            <p key={note} className="text-xs text-muted-foreground">
              {note}
            </p>
          ))}
        </div>
      )}
    </QuerySection>
  );
}
