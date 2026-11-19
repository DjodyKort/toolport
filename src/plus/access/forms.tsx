import { useId, useState, type ReactNode } from "react";
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
import { Textarea } from "@/components/ui/textarea";
import { toastError } from "@/lib/toast";
import { errorText } from "../settings/useCtl";
import type { ToolChange } from "./commands";
import type { AccessSetRow, NarrowedServer, NamedRef } from "./data";
import { absoluteFolder, nameProblem } from "./model";

const SELECT =
  "h-9 rounded-md border border-input bg-background px-2 text-sm focus-visible:ring-1 focus-visible:ring-ring focus-visible:outline-none";

function Shell({
  title,
  description,
  onCancel,
  onPreview,
  canPreview,
  children,
}: {
  title: string;
  description: string;
  onCancel: () => void;
  onPreview: () => void;
  canPreview: boolean;
  children: ReactNode;
}) {
  return (
    <Dialog open onOpenChange={(open) => !open && onCancel()}>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-3 text-meta">{children}</div>
        <DialogFooter>
          <Button variant="outline" onClick={onCancel}>
            Cancel
          </Button>
          <Button disabled={!canPreview} onClick={onPreview}>
            Preview the plan
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function Field({
  label,
  htmlFor,
  children,
  error,
}: {
  label: string;
  htmlFor?: string;
  children: ReactNode;
  error?: string | null;
}) {
  return (
    <div className="flex flex-col gap-1">
      <label htmlFor={htmlFor} className="font-medium">
        {label}
      </label>
      {children}
      {error && (
        <span role="alert" className="text-destructive">
          {error}
        </span>
      )}
    </div>
  );
}

export function NewSetForm({
  servers,
  existing,
  onCancel,
  onPreview,
}: {
  servers: Array<NamedRef & { enabled: boolean }>;
  existing: string[];
  onCancel: () => void;
  onPreview: (name: string, serverIds: string[]) => void;
}) {
  const id = useId();
  const [name, setName] = useState("");
  const [chosen, setChosen] = useState<string[]>([]);
  const problem = nameProblem(name, existing);
  return (
    <Shell
      title="New access set"
      description="A named list of servers. Bind a client or a folder to it afterwards; until then nothing uses it."
      onCancel={onCancel}
      canPreview={name.trim() !== "" && problem === null}
      onPreview={() => onPreview(name.trim(), chosen)}
    >
      <Field label="Name" htmlFor={`${id}-name`} error={problem}>
        <Input
          id={`${id}-name`}
          value={name}
          placeholder="Deep work"
          aria-invalid={problem !== null}
          onChange={(event) => setName(event.target.value)}
        />
      </Field>
      <fieldset className="flex flex-col gap-1">
        <legend className="font-medium">Servers in the set</legend>
        {servers.length === 0 && (
          <span className="text-muted-foreground">
            There are no servers yet; the set starts empty.
          </span>
        )}
        {servers.map((server) => (
          <label key={server.id} className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={chosen.includes(server.id)}
              onChange={(event) =>
                setChosen(
                  event.target.checked
                    ? [...chosen, server.id]
                    : chosen.filter((other) => other !== server.id),
                )
              }
            />
            <span>
              {server.name}
              {!server.enabled && <span className="text-muted-foreground"> (off)</span>}
            </span>
          </label>
        ))}
      </fieldset>
    </Shell>
  );
}

export function DeleteChoiceForm({
  set,
  onCancel,
  onPreview,
}: {
  set: AccessSetRow;
  onCancel: () => void;
  onPreview: (orphans: "detach" | "leave") => void;
}) {
  const name = useId();
  const [choice, setChoice] = useState<"detach" | "leave" | null>(null);
  const users = [
    set.clients.length > 0 &&
      `${set.clients.length === 1 ? "client" : "clients"} ${set.clients.join(", ")}`,
    set.folders.length > 0 &&
      `${set.folders.length === 1 ? "folder route" : "folder routes"} ${set.folders.join(", ")}`,
  ].filter(Boolean);
  return (
    <Shell
      title={`Delete ${set.name}`}
      description={`${set.name} is still used by ${users.join(" and ")}. Choose what happens to them.`}
      onCancel={onCancel}
      canPreview={choice !== null}
      onPreview={() => choice && onPreview(choice)}
    >
      <fieldset className="flex flex-col gap-2">
        <legend className="sr-only">What happens to what uses {set.name}</legend>
        {(
          [
            [
              "detach",
              "Take their binding away",
              "Clients follow the default access, which may list more servers; the folder routes are removed.",
            ],
            [
              "leave",
              "Delete the set only",
              "They keep naming it and reach no server until they are bound again.",
            ],
          ] as const
        ).map(([value, label, hint]) => (
          <div key={value} className="flex items-start gap-2">
            <input
              id={`${name}-${value}`}
              type="radio"
              name={name}
              checked={choice === value}
              onChange={() => setChoice(value)}
            />
            <div>
              <label htmlFor={`${name}-${value}`} className="font-medium">
                {label}
              </label>
              <span className="block text-muted-foreground">{hint}</span>
            </div>
          </div>
        ))}
      </fieldset>
    </Shell>
  );
}

/** The change a person types for the tools of one server in one set. */
export function ToolsForm({
  set,
  narrowed,
  onCancel,
  onPreview,
}: {
  set: AccessSetRow;
  narrowed: NarrowedServer[];
  onCancel: () => void;
  onPreview: (server: NamedRef, change: ToolChange) => void;
}) {
  const id = useId();
  const [serverId, setServerId] = useState(set.servers[0]?.id ?? "");
  const current = narrowed.find((entry) => entry.server.id === serverId)?.tools;
  const [scope, setScope] = useState<"all" | "only" | "none">(
    current === undefined ? "all" : current.length === 0 ? "none" : "only",
  );
  const [text, setText] = useState((current ?? []).join("\n"));
  const tools = [
    ...new Set(
      text
        .split(/[\n,]/)
        .map((tool) => tool.trim())
        .filter(Boolean),
    ),
  ];
  const server = set.servers.find((entry) => entry.id === serverId);

  function pick(next: string) {
    setServerId(next);
    const stored = narrowed.find((entry) => entry.server.id === next)?.tools;
    setScope(stored === undefined ? "all" : stored.length === 0 ? "none" : "only");
    setText((stored ?? []).join("\n"));
  }

  return (
    <Shell
      title={`Tools of ${set.name}`}
      description="Choose which tools of one server this set exposes. A server with no list exposes all its tools."
      onCancel={onCancel}
      canPreview={server !== undefined && (scope !== "only" || tools.length > 0)}
      onPreview={() =>
        server &&
        onPreview(
          server,
          scope === "all"
            ? { kind: "all" }
            : scope === "none"
              ? { kind: "none" }
              : { kind: "only", tools },
        )
      }
    >
      <Field label="Server" htmlFor={`${id}-server`}>
        <select
          id={`${id}-server`}
          value={serverId}
          className={SELECT}
          onChange={(event) => pick(event.target.value)}
        >
          {set.servers.map((entry) => (
            <option key={entry.id} value={entry.id}>
              {entry.name}
            </option>
          ))}
        </select>
      </Field>
      <fieldset className="flex flex-col gap-1">
        <legend className="font-medium">Tools it exposes</legend>
        {(
          [
            ["all", "Every tool"],
            ["only", "Only the tools below"],
            ["none", "No tool"],
          ] as const
        ).map(([value, label]) => (
          <label key={value} className="flex items-center gap-2">
            <input
              type="radio"
              name={`${id}-scope`}
              checked={scope === value}
              onChange={() => setScope(value)}
            />
            {label}
          </label>
        ))}
      </fieldset>
      {scope === "only" && (
        <Field label="Tool names" htmlFor={`${id}-tools`}>
          <Textarea
            id={`${id}-tools`}
            value={text}
            rows={4}
            placeholder={"read_file\nlist_dir"}
            onChange={(event) => setText(event.target.value)}
          />
          <span className="text-muted-foreground">
            One per line or separated by commas, as the server names them. They are not
            checked against what the server lists.
          </span>
        </Field>
      )}
    </Shell>
  );
}

/** One choice out of a list: a client's access set, a client's tool listing, a folder's set. */
export function ChoiceForm({
  title,
  description,
  label,
  options,
  initial,
  onCancel,
  onPreview,
}: {
  title: string;
  description: string;
  label: string;
  options: Array<{ value: string; label: string }>;
  initial: string;
  onCancel: () => void;
  onPreview: (value: string) => void;
}) {
  const id = useId();
  const [value, setValue] = useState(initial);
  return (
    <Shell
      title={title}
      description={description}
      onCancel={onCancel}
      canPreview
      onPreview={() => onPreview(value)}
    >
      <Field label={label} htmlFor={`${id}-choice`}>
        <select
          id={`${id}-choice`}
          value={value}
          className={SELECT}
          onChange={(event) => setValue(event.target.value)}
        >
          {options.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      </Field>
    </Shell>
  );
}

export function RouteForm({
  sets,
  onCancel,
  onPreview,
}: {
  sets: AccessSetRow[];
  onCancel: () => void;
  onPreview: (path: string, setId: string) => void;
}) {
  const id = useId();
  const [path, setPath] = useState("");
  const [setId, setSetId] = useState(sets[0]?.id ?? "");
  const problem =
    path.trim() !== "" && !absoluteFolder(path)
      ? "Give the full folder path, starting with / or a drive letter."
      : null;

  async function browse() {
    try {
      const picked = await openFolderDialog({ directory: true, multiple: false });
      if (typeof picked === "string") setPath(picked);
    } catch (error) {
      toastError("Could not open the folder dialog", { description: errorText(error) });
    }
  }

  return (
    <Shell
      title="Route a folder"
      description="Inside this folder, and below it, the gateway uses the set you pick for every client."
      onCancel={onCancel}
      canPreview={path.trim() !== "" && problem === null && setId !== ""}
      onPreview={() => onPreview(path.trim(), setId)}
    >
      <Field label="Folder" htmlFor={`${id}-path`} error={problem}>
        <div className="flex gap-2">
          <Input
            id={`${id}-path`}
            value={path}
            placeholder="/Users/you/work/project"
            aria-invalid={problem !== null}
            className="font-mono"
            onChange={(event) => setPath(event.target.value)}
          />
          <Button variant="outline" onClick={() => void browse()}>
            Browse…
          </Button>
        </div>
      </Field>
      <Field label="Access set" htmlFor={`${id}-set`}>
        <select
          id={`${id}-set`}
          value={setId}
          className={SELECT}
          onChange={(event) => setSetId(event.target.value)}
        >
          {sets.map((set) => (
            <option key={set.id} value={set.id}>
              {set.name}
            </option>
          ))}
        </select>
      </Field>
    </Shell>
  );
}
