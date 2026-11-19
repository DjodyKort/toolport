import { useState } from "react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { toastError } from "@/lib/toast";
import { errorText } from "../settings/useCtl";
import type { Loaded } from "../servers/useLoad";
import { keepFiles, takeLibraryCopy } from "../library/commands";
import type { LayerRow, LayerState, LayersData, OrgStatus, SyncData } from "./data";
import { intervalText, plural, shortPath, tokens, whenText } from "./model";
import { Group, RowShell } from "./StackRows";

const STATE: Record<LayerState, { label: string; tone: "success" | "warning" | "off" }> =
  {
    deployed: { label: "deployed", tone: "success" },
    "not-deployed": { label: "not deployed", tone: "off" },
    outdated: { label: "out of date", tone: "warning" },
    edited: { label: "hand-edited copy", tone: "warning" },
    shadowed: { label: "hand-written file in the way", tone: "warning" },
    nowhere: { label: "lands nowhere", tone: "warning" },
    problem: { label: "problem", tone: "warning" },
  };

function chip(layer: LayerRow): string {
  if (layer.scope === "global") return "everywhere";
  if (layer.scope === "glob") return `paths ${layer.globs.join(", ")}`;
  return `folder pattern ${layer.folderPatterns.join(", ")}`;
}

function landing(layer: LayerRow, home: string | null): string {
  const how = layer.delivery === "import" ? "import" : "text copied into the layer";
  if (layer.target.kind === "rules")
    return `${how} · lands in ${shortPath(layer.target.file, home)}`;
  const count = layer.target.folders.length;
  return `${how} · lands in ${layer.target.file} in ${plural(count, "folder")}`;
}

function Diff({ before, after }: { before: string; after: string }) {
  return (
    <div className="grid gap-2 text-meta @md:grid-cols-2">
      {[
        ["Now in the file", before],
        ["After a sync", after],
      ].map(([label, text]) => (
        <div key={label} className="min-w-0">
          <p className="font-medium">{label}</p>
          <pre className="overflow-x-auto whitespace-pre-wrap rounded-md bg-muted p-2 font-mono">
            {text}
          </pre>
        </div>
      ))}
    </div>
  );
}

function ConfirmReplace({
  name,
  onClose,
  onDone,
}: {
  name: string;
  onClose: () => void;
  onDone: () => void;
}) {
  const [busy, setBusy] = useState(false);
  async function go() {
    setBusy(true);
    try {
      await takeLibraryCopy(name, `${name}.md`);
      toast.success(
        `Replaced ${name}.md; the file it replaced is kept in the backup folder`,
      );
      onDone();
      onClose();
    } catch (error) {
      toastError("Nothing was changed", { description: errorText(error) });
    } finally {
      setBusy(false);
    }
  }
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Back up and replace {name}.md</DialogTitle>
          <DialogDescription>
            The file in <code>~/.claude/rules</code> is copied to the backup folder, then
            written again from the layer. Nothing is overwritten without that copy.
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button data-confirm autoFocus disabled={busy} onClick={() => void go()}>
            Back up and replace
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function OrgCard({ org, home }: { org: OrgStatus; home: string | null }) {
  if (!org.installed) {
    return (
      <Group title="Org file">
        <RowShell
          title="The org tool is not installed"
          subtitle="No org file loads and no coexistence rules apply. Layers still deploy. You see this on a machine without the org tool, such as a personal laptop."
        />
      </Group>
    );
  }
  const rows: Array<[string, string]> = [
    ["Owner", org.name ?? "org tool"],
    ["Folder", shortPath(org.folder, home)],
    [
      "Managed by",
      `${org.pull.source === "configured" ? "the pull command" : (org.name ?? "the org tool")} sync, about every ${intervalText(org.intervalSecs)}`,
    ],
    ["Last synced", org.lastSyncedAt ? whenText(org.lastSyncedAt) : "not recorded"],
    [
      "Size",
      org.fileTokens === null
        ? "no file"
        : `≈ ${tokens(org.fileTokens)} tokens, estimate`,
    ],
  ];
  return (
    <Group title="Org file" hint="read-only">
      <div className="flex flex-col gap-2 px-3 py-2.5">
        <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-meta">
          {rows.map(([label, value]) => (
            <div key={label} className="contents">
              <dt className="text-muted-foreground">{label}</dt>
              <dd className="break-words">{value}</dd>
            </div>
          ))}
        </dl>
        <p className="text-meta text-muted-foreground">
          Toolport never edits this file; its owner replaces it on a schedule. Your layers
          load after it.
        </p>
        <div className="flex flex-col gap-1 text-meta">
          <span className="text-muted-foreground">Pull command</span>
          <code className="break-words rounded-md bg-muted px-2 py-1 font-mono">
            {org.pull.display}
          </code>
          {org.pull.error && <span className="text-destructive">{org.pull.error}</span>}
          <span className="text-muted-foreground">
            {org.pull.source === "default"
              ? "Nothing is set, so the built-in pull runs. "
              : ""}
            To change it, edit <code className="font-mono">{org.pull.key}</code> in{" "}
            <code className="font-mono">{shortPath(org.pull.configFile, home)}</code>. It
            is a list: the program first, then one item per argument.
          </span>
        </div>
      </div>
    </Group>
  );
}

/** The layers of the skills repository with where each lands and what state it is in, and the
 * read-only org file. A hand-edited copy and a hand-written file in the way are decided here;
 * writing the text of a layer is done in the Library. */
export default function Layers({
  layers,
  org,
  sync,
  home,
  reload,
}: {
  layers: Loaded<LayersData>;
  org: OrgStatus | null;
  sync: SyncData | null;
  home: string | null;
  reload: () => void;
}) {
  const [diff, setDiff] = useState<string | null>(null);
  const [replace, setReplace] = useState<string | null>(null);
  const rows = layers.data?.layers ?? [];

  async function keepMine(name: string) {
    try {
      await keepFiles(name, [`${name}.md`]);
      toast.success(
        `Kept your ${name}.md; a sync leaves it alone until it changes again`,
      );
      reload();
    } catch (error) {
      toastError("Nothing was changed", { description: errorText(error) });
    }
  }

  return (
    <>
      <Group title="Layers" count={rows.length}>
        {layers.error && !layers.data && (
          <RowShell title="The layers could not be read" subtitle={layers.error} />
        )}
        {layers.data && rows.length === 0 && (
          <RowShell
            title="No layers yet"
            subtitle={`A layer is a rules/<name>/SKILL.md file in the skills repository (${shortPath(layers.data.library, home)}).`}
          />
        )}
        {rows.map((layer) => {
          const state = STATE[layer.state];
          const change = sync?.layers.rows.find(
            (r) => r.layer === layer.name && r.diff && r.action !== "add",
          );
          return (
            <RowShell
              key={layer.name}
              title={layer.name}
              badge={
                <>
                  <Badge variant="outline">{chip(layer)}</Badge>
                  <Badge variant={state.tone}>{state.label}</Badge>
                </>
              }
              subtitle={landing(layer, home)}
              trailing={
                <span
                  className="text-sm tabular-nums text-muted-foreground"
                  aria-label={`${layer.tokens.value} tokens, estimate`}
                >
                  ≈ {tokens(layer.tokens.value)}
                </span>
              }
            >
              {layer.detail && (
                <p className="text-meta text-muted-foreground">{layer.detail}</p>
              )}
              {layer.problems.map((problem) => (
                <p key={problem} role="status" className="text-meta text-warning">
                  {problem}
                </p>
              ))}
              <div className="flex flex-wrap items-center gap-2">
                {change?.diff && (
                  <Button
                    size="sm"
                    variant="ghost"
                    aria-expanded={diff === layer.name}
                    onClick={() => setDiff(diff === layer.name ? null : layer.name)}
                  >
                    Show diff
                  </Button>
                )}
                {layer.target.kind === "rules" &&
                  (layer.state === "shadowed" || layer.state === "edited") && (
                    <>
                      <Button
                        size="sm"
                        variant="outline"
                        onClick={() => setReplace(layer.name)}
                      >
                        Back up and replace
                      </Button>
                      <Button
                        size="sm"
                        variant="outline"
                        onClick={() => void keepMine(layer.name)}
                      >
                        Keep mine
                      </Button>
                    </>
                  )}
                <span className="break-all font-mono text-meta text-muted-foreground">
                  {shortPath(layer.path, home)}
                </span>
              </div>
              {change?.diff && diff === layer.name && (
                <Diff before={change.diff.before} after={change.diff.after} />
              )}
            </RowShell>
          );
        })}
      </Group>
      {org && <OrgCard org={org} home={home} />}
      {replace && (
        <ConfirmReplace name={replace} onClose={() => setReplace(null)} onDone={reload} />
      )}
    </>
  );
}
