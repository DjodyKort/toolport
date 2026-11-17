import { useState } from "react";
import { FolderOpen } from "lucide-react";
import { open as openFolderDialog } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { toastError } from "@/lib/toast";
import { errorText } from "../settings/useCtl";
import { shortPath } from "./model";

export interface FolderPickerProps {
  folder: string | null;
  home: string | null;
  /** Folders chosen before, then folders a bundle is applied in. */
  suggestions: Array<{ path: string; note?: string }>;
  onChoose: (folder: string) => void;
}

/** The folder chip of the page header. The list holds the folders chosen before and the folders
 * a bundle is applied in; any other folder is typed or picked with the system dialog. */
export default function FolderPicker({
  folder,
  home,
  suggestions,
  onChoose,
}: FolderPickerProps) {
  const [open, setOpen] = useState(false);
  const [typed, setTyped] = useState("");

  async function browse() {
    try {
      const picked = await openFolderDialog({ directory: true, multiple: false });
      if (typeof picked === "string") choose(picked);
    } catch (error) {
      toastError("Could not open the folder dialog", { description: errorText(error) });
    }
  }

  function choose(path: string) {
    onChoose(path);
    setOpen(false);
    setTyped("");
  }

  return (
    <>
      <Button
        variant="outline"
        size="sm"
        aria-label={folder ? `Folder: ${folder}. Change folder` : "Choose a folder"}
        onClick={() => setOpen(true)}
        className="max-w-full"
      >
        <FolderOpen data-icon="inline-start" />
        <span className="truncate font-mono text-xs">
          {folder ? shortPath(folder, home) : "Choose folder…"}
        </span>
      </Button>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Choose a folder</DialogTitle>
            <DialogDescription>
              The folder where Claude Code starts. Everything on this page is about
              sessions started there.
            </DialogDescription>
          </DialogHeader>
          {suggestions.length > 0 && (
            <ul className="flex max-h-60 flex-col gap-1 overflow-y-auto">
              {suggestions.map((s) => (
                <li key={s.path}>
                  <button
                    type="button"
                    className="flex w-full items-center justify-between gap-3 rounded-md px-2 py-1.5 text-left hover:bg-muted"
                    onClick={() => choose(s.path)}
                  >
                    <span className="truncate font-mono text-xs">
                      {shortPath(s.path, home)}
                    </span>
                    {s.note && (
                      <span className="shrink-0 text-meta text-muted-foreground">
                        {s.note}
                      </span>
                    )}
                  </button>
                </li>
              ))}
            </ul>
          )}
          <form
            className="flex gap-2"
            onSubmit={(event) => {
              event.preventDefault();
              if (typed.trim()) choose(typed);
            }}
          >
            <Input
              aria-label="Folder path"
              placeholder="/path/to/project"
              value={typed}
              onChange={(e) => setTyped(e.target.value)}
            />
            <Button type="submit" variant="secondary" disabled={!typed.trim()}>
              Use
            </Button>
          </form>
          <DialogFooter>
            <Button variant="outline" onClick={() => void browse()}>
              Browse…
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
