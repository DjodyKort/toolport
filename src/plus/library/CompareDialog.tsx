import { useState } from "react";
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
import { toast } from "sonner";
import { toastError } from "@/lib/toast";
import { useLoad } from "../servers/useLoad";
import { errorText } from "../settings/useCtl";
import { compareSkill, keepFiles, takeLibraryCopy } from "./commands";
import type { CompareFile } from "./data";
import { Mono } from "./dialogs";
import { bytes, fileBadge, plural } from "./model";

const LINE_TONE: Record<string, string> = {
  "+": "bg-success/10 text-success",
  "-": "bg-destructive/10 text-destructive",
  "@": "text-muted-foreground",
};

/** A unified diff, line by line, with the sign of each line in words for the reader who cannot
 * tell green from red. */
function DiffView({ diff }: { diff: string }) {
  const lines = diff.replace(/\n$/, "").split("\n");
  return (
    <pre
      aria-label="Differences"
      className="max-h-80 overflow-auto rounded-md border bg-muted/30 py-1 font-mono text-xs"
    >
      {lines.map((line, index) => {
        const sign = line[0] ?? " ";
        const header = line.startsWith("---") || line.startsWith("+++");
        return (
          <span
            key={`${index}-${line}`}
            className={`block px-3 whitespace-pre-wrap break-all ${header ? "text-muted-foreground" : (LINE_TONE[sign] ?? "")}`}
          >
            {line || " "}
          </span>
        );
      })}
    </pre>
  );
}

function Verdict({ file }: { file: CompareFile }) {
  if (file.binary) {
    return (
      <p className="rounded-md border px-3 py-2 text-meta" role="note">
        Binary file. The library copy is {bytes(file.library.size)} and the installed copy
        is {bytes(file.installed.size)}; Toolport cannot show what differs.
      </p>
    );
  }
  if (!file.installed.present) {
    return <p className="text-meta">The installed copy is missing.</p>;
  }
  if (file.identical || !file.diff) {
    return <p className="text-meta">Both copies have the same text.</p>;
  }
  return (
    <>
      <p className="text-meta text-muted-foreground">
        Library copy (lines starting with −) against what is installed (+):{" "}
        {plural(file.added, "line")} added, {plural(file.removed, "line")} removed.
        {file.truncated ? " Only the first 512 KB of each copy is compared." : ""}
      </p>
      <DiffView diff={file.diff} />
    </>
  );
}

export interface CompareDialogProps {
  skill: string;
  /** Opens on this file of the skill folder; without it, on the first file that is held back. */
  file?: string;
  onClose: () => void;
  onDone: () => void;
}

/** A file the sync holds back, library copy against installed copy. Keep mine leaves the file
 * alone and stops the hold while it stays as it is; Use library copy replaces it after a
 * backup. Neither syncs anything else. */
export default function CompareDialog({
  skill,
  file,
  onClose,
  onDone,
}: CompareDialogProps) {
  const data = useLoad(() => compareSkill(skill, file), `${skill}:${file ?? ""}`);
  const [picked, setPicked] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const files = data.data?.files ?? [];
  const current = files.find((f) => f.path === picked) ?? files[0] ?? null;
  const badge = fileBadge({
    client: current?.client ?? null,
    op: null,
    path: current?.path ?? "",
    reason: null,
    size: 0,
    status: current?.status ?? "",
  });

  async function decide(kind: "keep" | "take") {
    if (!current) return;
    setBusy(true);
    try {
      if (kind === "keep") await keepFiles(skill, [current.path]);
      else await takeLibraryCopy(skill, current.path);
      toast.message(
        kind === "keep"
          ? `${current.path} stays as it is`
          : `${current.path} replaced with the library copy`,
        { description: kind === "take" ? "Your edit is kept as a backup." : undefined },
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
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>Compare {skill}</DialogTitle>
          <DialogDescription>
            The sync holds these files back because they differ from the library or were
            edited after the last sync. Nothing changes until you choose.
          </DialogDescription>
        </DialogHeader>
        {data.error && !data.data && <p role="alert">{data.error}</p>}
        {!data.data && !data.error && <p className="text-meta">Reading both copies…</p>}
        {data.data && files.length === 0 && (
          <p className="text-meta">
            No file of {skill} differs from the library copy any more.
          </p>
        )}
        {files.length > 1 && (
          <div role="tablist" aria-label="Files" className="flex flex-wrap gap-1">
            {files.map((item) => (
              <button
                key={`${item.client}:${item.path}`}
                role="tab"
                type="button"
                aria-selected={item.path === current?.path}
                onClick={() => setPicked(item.path)}
                className="rounded-md px-2 py-1 font-mono text-xs hover:bg-muted aria-selected:bg-muted aria-selected:font-semibold"
              >
                {item.path}
              </button>
            ))}
          </div>
        )}
        {current && (
          <div className="flex flex-col gap-2" data-testid="compare-file">
            <div className="flex flex-wrap items-center gap-2">
              <Mono>{current.path}</Mono>
              <Badge variant={badge.variant}>{badge.label}</Badge>
              <Mono>{current.installedPath}</Mono>
            </div>
            <Verdict file={current} />
          </div>
        )}
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Close
          </Button>
          <Button
            variant="outline"
            disabled={!current || busy}
            onClick={() => void decide("keep")}
          >
            Keep mine
          </Button>
          <Button disabled={!current || busy} onClick={() => void decide("take")}>
            Use library copy
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
