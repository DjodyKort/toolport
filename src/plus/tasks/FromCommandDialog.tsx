import { useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Callout } from "@/components/Callout";
import type { SourcesLsData } from "../bridge/data";
import { Field } from "../system/atoms";
import { AsyncView, useCtlQuery } from "../ui";
import { ID_PATTERN, looksLikeTask, slugFromPath } from "./model";

/** Turns a Claude command file into a disabled draft task: one `prompt` step plus a
 * `needs-you` step for each place the command asks you to sign in. The commands come from
 * `sources ls --items`; a file the list does not know can be given by its path. */
export function FromCommandDialog({
  existing,
  onReview,
  onClose,
}: {
  existing: string[];
  onReview: (id: string, path: string) => void;
  onClose: () => void;
}) {
  const query = useCtlQuery<SourcesLsData>(["sources", "ls", "--items"]);
  const [path, setPath] = useState("");
  const [id, setId] = useState("");
  const [idTouched, setIdTouched] = useState(false);
  const pick = (next: string) => {
    setPath(next);
    if (!idTouched) setId(slugFromPath(next));
  };
  const clash = existing.includes(id);
  const idError =
    id && !ID_PATTERN.test(id)
      ? "Use 1 to 64 characters of a-z, 0-9 and -"
      : clash
        ? "A task with this id exists"
        : null;
  const ready = path.trim() !== "" && id !== "" && !idError;
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent aria-describedby={undefined} className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>Create a task from a command</DialogTitle>
        </DialogHeader>
        <div className="flex max-h-[60vh] flex-col gap-3 overflow-y-auto">
          <p className="text-sm text-muted-foreground">
            The draft is a prompt step with a step that needs you wherever the command
            says to sign in. It stays off until you edit and enable it.
          </p>
          <AsyncView
            query={query}
            errorTitle="Couldn't list your commands"
            context="sources ls --items"
            isEmpty={(data) =>
              !(data.items ?? []).some((item) => item.kind === "command")
            }
            empty={
              <Callout variant="info" role="status">
                No command files were found in the folders Toolport scans. Enter the path
                of one below.
              </Callout>
            }
          >
            {(data) => {
              const commands = (data.items ?? [])
                .filter((item) => item.kind === "command")
                .sort(
                  (a, b) =>
                    Number(looksLikeTask(b.name)) - Number(looksLikeTask(a.name)) ||
                    a.name.localeCompare(b.name),
                );
              return (
                <div
                  role="radiogroup"
                  aria-label="Commands"
                  className="flex flex-col gap-1"
                >
                  {commands.map((item) => (
                    <label
                      key={item.path}
                      className="flex cursor-pointer items-center gap-2 rounded-md border px-3 py-1.5 text-sm has-[:checked]:border-primary"
                    >
                      <input
                        type="radio"
                        name="command"
                        checked={path === item.path}
                        onChange={() => pick(item.path)}
                      />
                      <span className="min-w-0 flex-1">
                        <b className="font-medium">{item.name}</b>
                        <span className="block font-mono text-xs break-all text-muted-foreground">
                          {item.path}
                        </span>
                      </span>
                      {looksLikeTask(item.name) && (
                        <Badge variant="info">Looks like a task</Badge>
                      )}
                    </label>
                  ))}
                </div>
              );
            }}
          </AsyncView>
          <Field
            label="Command file"
            value={path}
            onChange={pick}
            placeholder="~/.claude/commands/refresh-login.md"
            hint="Pick one above or enter a path."
          />
          <Field
            label="Task id"
            value={id}
            onChange={(value) => {
              setIdTouched(true);
              setId(value);
            }}
            error={idError}
            hint="Letters, digits and dashes."
          />
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button disabled={!ready} onClick={() => onReview(id, path.trim())}>
            Review draft
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
