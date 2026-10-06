import { useState } from "react";
import { Plus } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { SourcesLsData } from "../bridge/data";
import type { ContextClientListData } from "../types/context-layers";
import { ComposedView } from "./ComposedView";
import { FolderField, suggestions, useRecentFolders } from "./folder";
import { useRead, useWrite } from "./hooks";
import { LayerDeleteDialog, LayerFormDialog, type LayerForm } from "./LayerDialogs";
import { WriteDialogs } from "./WriteDialogs";
import { formatTokens } from "./model";
import { basisWord } from "./stackModel";
import { Code, Kv, QuerySection, Section, useDialog, useRows } from "./parts";

type Layer = ContextClientListData["layers"][number];
type Dialog = "add" | "edit" | "delete";

export function scopeWords(layer: Layer): string {
  if (layer.scope === "global") return "everywhere";
  if (layer.scope === "folder")
    return layer.folders.length === 0
      ? "chosen folders"
      : `${layer.folders.length} chosen folder(s)`;
  return layer.globs.length === 0
    ? "everywhere"
    : `folder pattern ${layer.globs.join(", ")}`;
}

function layerForm(layer: Layer): LayerForm {
  return {
    name: layer.name,
    scope: layer.scope,
    glob: layer.globs[0] ?? "",
    folders: layer.folders.join("\n"),
    imports: layer.imports.join("\n"),
    delivery: layer.delivery,
  };
}

function When({ iso }: { iso: string | null | undefined }) {
  if (!iso) return <span className="text-muted-foreground">unknown</span>;
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? (
    <span>{iso}</span>
  ) : (
    <time dateTime={iso} title={iso}>
      {date.toLocaleString()}
    </time>
  );
}

/** The org file, read-only: who owns it and when it was last overwritten. Toolport never
 * edits it; own text lives in layers. */
function OrgFile({ onPreview }: { onPreview: () => void }) {
  const query = useRead<SourcesLsData>(["sources", "ls", "--source", "org"]);
  return (
    <QuerySection
      title="Org file"
      hint="Read-only. Your own sections live in layers and load after it."
      query={query}
      isEmpty={(data) => data.sources.length === 0}
      empty={
        <div className="flex flex-col gap-1 text-sm text-muted-foreground">
          <p>No org file on this computer.</p>
          <p>
            Setup looks for a git clone under <Code>~/.local/share</Code> with{" "}
            <Code>claude/CLAUDE.md</Code> and records it; run{" "}
            <Code>toolportctl context sync</Code> after installing one.
          </p>
        </div>
      }
      actions={<Badge variant="secondary">read-only</Badge>}
    >
      {(data) => {
        const org = data.sources[0];
        return (
          <div className="flex flex-col gap-3">
            <Kv
              rows={[
                ["Owner", org.origin.name],
                ...(org.root
                  ? ([["Folder", <Code key="r">{org.root}</Code>]] as Array<
                      [string, React.ReactNode]
                    >)
                  : []),
                ["Managed by", org.managedBy ?? "unknown"],
                ["Last synced", <When key="w" iso={org.freshness?.lastSync} />],
                [
                  "Size",
                  `${formatTokens(org.tokens.value)} tokens, ${basisWord(org.tokens.basis)}`,
                ],
              ]}
            />
            <p className="text-sm text-muted-foreground">
              Toolport never edits this file; its owner replaces it on a schedule.
            </p>
            <div>
              <Button size="sm" variant="outline" onClick={onPreview}>
                Preview the stack for a folder
              </Button>
            </div>
          </div>
        );
      }}
    </QuerySection>
  );
}

/** The Context tab "Layers": your own sections on top of the org file, with scope, folders,
 * imports and delivery, and the composed text for a folder. */
export function LayersTab({ openTab }: { openTab?: (id: string) => void }) {
  const rows = useRows();
  const list = useRead<ContextClientListData>(["context", "client", "list"]);
  const [selected, setSelected] = useState<string | null>(null);
  const [version, setVersion] = useState(0);
  const { reload } = list;
  const write = useWrite(rows, () => {
    reload();
    setVersion((n) => n + 1);
  });
  const dialog = useDialog<Dialog>();
  const { recent, remember } = useRecentFolders();
  const [draft, setDraft] = useState("");
  const [folder, setFolder] = useState<string | null>(null);
  const layers = list.data?.layers ?? [];
  const layer = layers.find((one) => one.name === selected) ?? layers[0] ?? null;
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center gap-2">
        <Button onClick={() => dialog.show("add")} disabled={write.busy}>
          <Plus /> Add layer…
        </Button>
        <span className="text-sm text-muted-foreground">
          Layers are your own sections on top of the org file. Each has a scope:
          everywhere, a folder pattern, or chosen folders.
        </span>
      </div>
      <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,24rem)]">
        <QuerySection
          title="Layers"
          query={list}
          isEmpty={(data) => data.layers.length === 0}
          empty={
            <p className="text-sm text-muted-foreground">
              No layer yet. Add one for text that should load in some folders only.
            </p>
          }
        >
          {() => (
            <div className="flex flex-col gap-3">
              <ul
                aria-label="Layer list"
                className="flex flex-col divide-y rounded-lg border"
              >
                {layers.map((one) => (
                  <li key={one.name}>
                    <button
                      type="button"
                      aria-pressed={one.name === layer?.name}
                      onClick={() => setSelected(one.name)}
                      className="flex w-full flex-wrap items-center gap-2 px-3 py-2 text-left text-sm hover:bg-muted aria-pressed:bg-muted"
                    >
                      <b>{one.name}</b>
                      <span className="text-xs text-muted-foreground">
                        {scopeWords(one)}
                      </span>
                      <Badge variant="outline">{one.delivery}</Badge>
                      {one.issues.length > 0 && (
                        <Badge variant="warning">needs a look</Badge>
                      )}
                      <Badge
                        variant={one.deployedTo.length > 0 ? "success" : "secondary"}
                        className="ml-auto"
                      >
                        {one.deployedTo.length > 0 ? "deployed" : "not deployed"}
                      </Badge>
                    </button>
                  </li>
                ))}
              </ul>
              {layer && (
                <section
                  aria-label={`Layer ${layer.name}`}
                  className="flex flex-col gap-2"
                >
                  <p className="text-sm text-muted-foreground">{layer.description}</p>
                  <Kv
                    rows={[
                      ["File", <Code key="p">{layer.path}</Code>],
                      ["Scope", scopeWords(layer)],
                      [
                        "Folders",
                        layer.folders.length ? layer.folders.join(", ") : "none",
                      ],
                      [
                        "Imports",
                        layer.imports.length ? layer.imports.join(", ") : "none",
                      ],
                      [
                        "Delivery",
                        layer.delivery === "copy"
                          ? "text copied into the layer"
                          : "an @import line",
                      ],
                      [
                        "Deployed to",
                        layer.deployedTo.length
                          ? layer.deployedTo.join(", ")
                          : "nowhere yet",
                      ],
                    ]}
                  />
                  {layer.issues.length > 0 && (
                    <ul
                      aria-label="Layer problems"
                      className="list-disc pl-4 text-xs text-amber-700 dark:text-amber-400"
                    >
                      {layer.issues.map((issue) => (
                        <li key={`${issue.key}:${issue.message}`}>
                          {issue.key}: {issue.message}
                        </li>
                      ))}
                    </ul>
                  )}
                  <div className="flex gap-2">
                    <Button
                      size="sm"
                      variant="outline"
                      disabled={write.busy}
                      onClick={() => dialog.show("edit")}
                    >
                      Edit
                    </Button>
                    <Button
                      size="sm"
                      variant="destructive"
                      disabled={write.busy || !layer.name.startsWith("client-")}
                      title={
                        layer.name.startsWith("client-")
                          ? undefined
                          : "Only client layers are deleted here"
                      }
                      onClick={() => dialog.show("delete")}
                    >
                      Delete…
                    </Button>
                  </div>
                </section>
              )}
            </div>
          )}
        </QuerySection>
        <OrgFile onPreview={() => openTab?.("here")} />
      </div>
      <Section
        title="Composed preview"
        hint="The instruction text Claude builds in a folder: the org file first, then your layers."
      >
        <FolderField
          value={draft}
          onChange={setDraft}
          options={suggestions(
            recent,
            layers.flatMap((one) => one.folders),
          )}
          placeholder="Folder where Claude starts"
          submitLabel="Preview"
          onSubmit={() => {
            setFolder(draft.trim() || null);
            remember(draft);
          }}
        />
        <ComposedView cwd={folder} title="Composed text" version={version} />
      </Section>
      {dialog.open === "add" && (
        <LayerFormDialog
          write={write}
          taken={layers.map((one) => one.name)}
          onClose={dialog.close}
        />
      )}
      {dialog.open === "edit" && layer && (
        <LayerFormDialog
          write={write}
          initial={layerForm(layer)}
          taken={[]}
          onClose={dialog.close}
        />
      )}
      {dialog.open === "delete" && layer && (
        <LayerDeleteDialog write={write} name={layer.name} onClose={dialog.close} />
      )}
      <WriteDialogs write={write} />
    </div>
  );
}
