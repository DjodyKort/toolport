import { useState } from "react";
import { FileSearch, Hammer } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { Textarea } from "@/components/ui/textarea";
import { shellQuote } from "../allcommands/model";
import { ctlData } from "../bridge/ctl";
import { ErrorState } from "../ui";
import { Card, Field, Intro, Mono, TerminalCommand, Tag, Toggle } from "./atoms";
import { rowsOf, useRegistry, useWrite } from "./hooks";
import {
  lines,
  nameMapOf,
  optionFlag,
  planOfImport,
  planOfRenameRefs,
  plural,
  refRows,
  type NameMap,
  type RefRow,
} from "./model";
import { WriteDialogs } from "./WriteDialogs";

function RefTable({ rows, label }: { rows: RefRow[]; label: string }) {
  return (
    <ul aria-label={label} className="flex flex-col divide-y text-sm">
      {rows.map((row, at) => (
        <li
          key={`${row.path}-${row.reference}-${at}`}
          className="flex flex-col gap-0.5 py-1.5"
        >
          <div className="flex flex-wrap items-center gap-2">
            <Mono>{row.reference}</Mono>
            {(row.rule === "ask" || row.rule === "deny") && (
              <Tag tone="destructive">in a {row.rule} rule</Tag>
            )}
          </div>
          <p className="text-xs text-muted-foreground">{row.reason}</p>
          <p className="text-xs text-muted-foreground">
            <Mono>{row.path}</Mono>
          </p>
        </li>
      ))}
    </ul>
  );
}

/** The orphan report of a rename-refs run: references it left as they are, and the ones that
 * name a server that is gone. A reference in an ask or deny rule is called out because the
 * rule then no longer matches the tool. */
function OrphanReport({
  data,
  previewed,
}: {
  data: Record<string, unknown>;
  previewed: boolean;
}) {
  const orphans = refRows(data.orphans);
  const dead = refRows(data.dead);
  return (
    <Card
      title={`Orphan report (${previewed ? "from the preview" : "from the rewrite"})`}
    >
      {orphans.length + dead.length === 0 ? (
        <p className="flex items-center gap-2 text-sm">
          <Tag tone="success">None</Tag> Every reference could be mapped.
        </p>
      ) : (
        <div className="flex flex-col gap-3">
          <Intro>
            A wildcard maps only when it ends a <code>server__prefix*</code> pattern; one
            that cuts a server name is listed here and left untouched.
          </Intro>
          {orphans.length > 0 && (
            <div className="flex flex-col gap-1">
              <h4 className="text-sm font-medium">Left as it is ({orphans.length})</h4>
              <RefTable rows={orphans} label="Orphans" />
            </div>
          )}
          {dead.length > 0 && (
            <div className="flex flex-col gap-1">
              <h4 className="text-sm font-medium">Server is gone ({dead.length})</h4>
              <RefTable rows={dead} label="References to removed servers" />
            </div>
          )}
        </div>
      )}
    </Card>
  );
}

/** Import: the mcpm importer with a preview that equals `import mcpm --dry-run`, the
 * reference rewrite with its orphan report, and the way back through the cutover backup. */
export function ImportTab() {
  const registry = useRegistry();
  const rows = rowsOf(registry);
  const [root, setRoot] = useState("");
  const [shortIds, setShortIds] = useState("");
  const [skipClients, setSkipClients] = useState(false);
  const [prune, setPrune] = useState(false);
  const [tools, setTools] = useState("");
  const [paths, setPaths] = useState("");
  const [backup, setBackup] = useState("");
  const [map, setMap] = useState<NameMap | null>(null);
  const [mapError, setMapError] = useState<unknown>(null);
  const [mapping, setMapping] = useState(false);
  const [refs, setRefs] = useState<{
    data: Record<string, unknown>;
    previewed: boolean;
  } | null>(null);
  const write = useWrite(rows, () => {});

  const rootOk = root.trim() !== "";
  const rewriteOk = rootOk && tools.trim() !== "" && lines(paths).length > 0;
  const rollback = `scripts/cutover/rollback.sh --home ~ --backup ${backup.trim() ? shellQuote(backup.trim()) : "<backup>"}`;

  async function showMap() {
    setMapping(true);
    setMapError(null);
    try {
      setMap(
        nameMapOf(
          await ctlData([
            "import",
            "mcpm",
            root.trim(),
            "--dry-run",
            "--tools",
            tools.trim(),
            "--name-map",
            ...optionFlag("--short-ids", shortIds),
          ]),
        ),
      );
    } catch (error) {
      setMap(null);
      setMapError(error);
    } finally {
      setMapping(false);
    }
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="grid gap-4 lg:grid-cols-2">
        <Card title="Import from mcpm">
          <Intro>
            Reads an mcpm config folder and creates the matching servers, profiles and
            secrets here. The preview is the same plan as the dry run and writes nothing.
          </Intro>
          <Field
            label="mcpm config folder"
            value={root}
            onChange={setRoot}
            placeholder="~/.config/mcpm"
          />
          <Field
            label="Short ids file (optional)"
            value={shortIds}
            onChange={setShortIds}
            placeholder="short-ids.json"
            hint="Maps old server ids to the short ids they had."
          />
          <Toggle
            label="Do not write client configurations"
            checked={skipClients}
            onChange={setSkipClients}
          />
          <Toggle
            label="Also remove entries a previous import wrote that mcpm no longer has"
            checked={prune}
            onChange={setPrune}
          />
          <div>
            <Button
              size="sm"
              disabled={!rootOk}
              onClick={() =>
                write.begin({
                  command: "import mcpm",
                  title: "Import from mcpm",
                  argv: [
                    "import",
                    "mcpm",
                    root.trim(),
                    ...(skipClients ? ["--skip-clients"] : []),
                    ...(prune ? ["--prune-orphans"] : []),
                    ...optionFlag("--short-ids", shortIds),
                  ],
                  confirmLabel: "Import",
                  adapt: planOfImport,
                })
              }
            >
              Preview import…
            </Button>
          </div>
        </Card>

        <Card title="Rewrite tool references">
          <Intro>
            Notes, rules and settings that name old <code>mcp__mcpm_</code> tools get the
            new names. Uses the config folder on the left.
          </Intro>
          <Field
            label="Tools file"
            value={tools}
            onChange={setTools}
            placeholder="tools.json"
            hint="Lists the tool names of each mcpm server."
          />
          <div className="flex flex-col gap-1">
            <label htmlFor="import-paths" className="text-sm font-medium">
              Files or folders to rewrite
            </label>
            <Textarea
              id="import-paths"
              value={paths}
              onChange={(event) => setPaths(event.target.value)}
              placeholder={"~/notes\n~/project/.claude"}
              rows={3}
              spellCheck={false}
              aria-describedby="import-paths-hint"
            />
            <p id="import-paths-hint" className="text-xs text-muted-foreground">
              One per line.
            </p>
          </div>
          <div className="flex flex-wrap gap-2">
            <Button
              size="sm"
              disabled={!rewriteOk}
              onClick={() =>
                write.begin({
                  command: "import rename-refs",
                  title: "Rewrite tool references",
                  argv: [
                    "import",
                    "rename-refs",
                    root.trim(),
                    "--tools",
                    tools.trim(),
                    ...optionFlag("--short-ids", shortIds),
                    "--paths",
                    ...lines(paths),
                  ],
                  confirmLabel: "Rewrite",
                  adapt: planOfRenameRefs,
                  onResult: (data, previewed) =>
                    data && typeof data === "object"
                      ? setRefs({ data: data as Record<string, unknown>, previewed })
                      : undefined,
                })
              }
            >
              Preview rewrite…
            </Button>
            <Button
              size="sm"
              variant="outline"
              disabled={!rootOk || tools.trim() === "" || mapping}
              onClick={() => void showMap()}
            >
              <FileSearch /> Show name map
            </Button>
          </div>
          {mapError !== null && (
            <ErrorState
              error={mapError}
              title="Couldn't build the name map"
              onRetry={() => void showMap()}
            />
          )}
          {map && (
            <div className="flex flex-col gap-1">
              <h4 className="text-sm font-medium">
                Name map ({plural(map.count, "tool")})
              </h4>
              <ul
                aria-label="Name map"
                className="flex max-h-48 flex-col gap-1 overflow-auto text-xs"
              >
                {map.map.map(([from, to]) => (
                  <li key={from} className="flex flex-wrap gap-1">
                    <Mono>{from}</Mono> <span aria-hidden="true">to</span>
                    <span className="sr-only">becomes</span> <Mono>{to}</Mono>
                  </li>
                ))}
              </ul>
            </div>
          )}
        </Card>
      </div>

      {refs && <OrphanReport data={refs.data} previewed={refs.previewed} />}

      <div className="grid gap-4 lg:grid-cols-2">
        <Card title="Undo">
          <Intro>
            The cutover backup holds the client configs as they were. Restoring is one
            command and it checks the files first. Run it in a terminal.
          </Intro>
          <Field
            label="Backup folder (optional)"
            value={backup}
            onChange={setBackup}
            placeholder="the newest backup"
            hint="Leave empty to restore the newest one."
          />
          <TerminalCommand
            line={rollback}
            note="Add --dry-run to see what it would restore."
          />
        </Card>
        <Card
          title="Other tools"
          actions={
            <Badge variant="secondary">
              <Hammer /> Not implemented
            </Badge>
          }
        >
          <p className="text-sm text-muted-foreground">
            Importing from tools other than mcpm is not implemented. Use the client import
            on the Clients page for servers a client already has.
          </p>
        </Card>
      </div>

      {registry.status === "error" && (
        <Callout variant="danger" role="alert">
          The command list could not be loaded, so writes are off.
        </Callout>
      )}
      <WriteDialogs write={write} />
    </div>
  );
}
