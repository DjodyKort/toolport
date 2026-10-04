import { useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { PathField } from "./fields";
import { byClient, clientName, nameProblem } from "./model";

const WHY: Record<string, string> = {
  lock: "the clients of the last sync",
  default: "no sync has chosen clients yet, so Toolport starts with Claude Code only",
  requested: "the clients you asked for",
};

/** The clients of a sync are always shown and chosen here: Toolport never syncs to every
 * client by itself. The first choice is what a bare `skills sync` would do. */
export function SyncDialog({
  known,
  initial,
  source,
  onSubmit,
  onClose,
}: {
  known: string[];
  initial: string[];
  source: string;
  onSubmit: (clients: string[]) => void;
  onClose: () => void;
}) {
  const [picked, setPicked] = useState<string[]>(initial);
  const clients = [...new Set([...known, ...initial])].sort(byClient);
  const toggle = (client: string) =>
    setPicked((now) =>
      now.includes(client) ? now.filter((c) => c !== client) : [...now, client],
    );
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Sync skills</DialogTitle>
          <DialogDescription>
            Pick the clients that get your skills and rules. Preselected:{" "}
            {WHY[source] ?? source}. The next sync starts from this choice.
          </DialogDescription>
        </DialogHeader>
        <fieldset className="grid gap-1.5 sm:grid-cols-2">
          <legend className="sr-only">Clients</legend>
          {clients.map((client) => (
            <label key={client} className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={picked.includes(client)}
                onChange={() => toggle(client)}
              />
              {clientName(client)}
            </label>
          ))}
        </fieldset>
        {picked.length === 0 && (
          <p role="status" className="text-xs text-destructive">
            Pick at least one client
          </p>
        )}
        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            disabled={picked.length === 0}
            onClick={() => onSubmit(clients.filter((c) => picked.includes(c)))}
          >
            Preview
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** Asks for the name and kind of a new skill or rule; the template, the preview and the
 * confirmation are the CLI's (`skills add`). */
export function NewDialog({
  onSubmit,
  onClose,
}: {
  onSubmit: (name: string, type: "skill" | "rule", progressive: boolean) => void;
  onClose: () => void;
}) {
  const [name, setName] = useState("");
  const [type, setType] = useState<"skill" | "rule">("skill");
  const [progressive, setProgressive] = useState(false);
  const [touched, setTouched] = useState(false);
  const problem = nameProblem(name);
  const submit = () => {
    setTouched(true);
    if (!problem) onSubmit(name, type, progressive);
  };
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>New skill</DialogTitle>
          <DialogDescription>
            Toolport creates a SKILL.md from its template in your skills repository. You
            see what it will write before anything is written.
          </DialogDescription>
        </DialogHeader>
        <form
          className="flex flex-col gap-3"
          onSubmit={(event) => {
            event.preventDefault();
            submit();
          }}
        >
          <label className="flex flex-col gap-1.5 text-sm">
            Name
            <Input
              autoFocus
              value={name}
              onChange={(event) => setName(event.target.value.trim())}
              autoComplete="off"
              spellCheck={false}
              aria-invalid={touched && problem ? true : undefined}
            />
          </label>
          <p role="status" className="min-h-4 text-xs text-destructive">
            {touched && problem ? problem : ""}
          </p>
          <fieldset className="flex gap-4 text-sm">
            <legend className="sr-only">Kind</legend>
            {(["skill", "rule"] as const).map((kind) => (
              <label key={kind} className="flex items-center gap-1.5">
                <input
                  type="radio"
                  name="kind"
                  checked={type === kind}
                  onChange={() => setType(kind)}
                />
                {kind === "skill"
                  ? "Skill (Claude chooses when to use it)"
                  : "Rule (always on)"}
              </label>
            ))}
          </fieldset>
          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={progressive}
              onChange={() => setProgressive((now) => !now)}
            />
            With progressive-disclosure reference files
          </label>
        </form>
        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={submit}>Preview</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** Creates the skills repository (`skills init`): where, and under which name. */
export function InitDialog({
  onSubmit,
  onClose,
}: {
  onSubmit: (path: string, name: string) => void;
  onClose: () => void;
}) {
  const [path, setPath] = useState("");
  const [name, setName] = useState("");
  const problem = name.trim() && nameProblem(name.trim());
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Create a skills repository</DialogTitle>
          <DialogDescription>
            Toolport creates the folders for skills, rules, agents, styles and profiles.
            You see the list before anything is written.
          </DialogDescription>
        </DialogHeader>
        <PathField
          label="Folder"
          kind="folder"
          value={path}
          onChange={setPath}
          placeholder="Empty: the configured location"
        />
        <label className="flex flex-col gap-1.5 text-sm">
          Name (optional)
          <Input
            value={name}
            onChange={(event) => setName(event.target.value)}
            autoComplete="off"
            spellCheck={false}
            aria-invalid={problem ? true : undefined}
          />
        </label>
        <p role="status" className="min-h-4 text-xs text-destructive">
          {problem || ""}
        </p>
        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button disabled={!!problem} onClick={() => onSubmit(path.trim(), name.trim())}>
            Preview
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
