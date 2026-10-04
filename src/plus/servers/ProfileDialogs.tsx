import { useEffect, useState, type FormEvent } from "react";
import { Loader2 } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { Input } from "@/components/ui/input";
import { Code, Field, Problems } from "./atoms";
import {
  inspectProfile,
  summarize,
  type InspectRow,
  type InspectRun,
} from "./inspectProfile";
import { profileEditArgv, type ProfileEdit } from "./forms";
import type { ProfileData, ServerView } from "./model";

export function CreateProfileDialog({
  open,
  onOpenChange,
  existing,
  onSubmit,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  existing: string[];
  onSubmit: (name: string) => void;
}) {
  const [name, setName] = useState("");
  const [tried, setTried] = useState(false);
  const trimmed = name.trim();
  const problems = !trimmed
    ? ["Give the profile a name"]
    : existing.some((other) => other.toLowerCase() === trimmed.toLowerCase())
      ? [`A profile called ${trimmed} already exists`]
      : [];
  function submit(event: FormEvent) {
    event.preventDefault();
    setTried(true);
    if (problems.length === 0) onSubmit(trimmed);
  }
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>New profile</DialogTitle>
          <DialogDescription>
            A profile is a named set of servers. It starts empty; add servers to it next.
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={submit} className="flex flex-col gap-3">
          <Field label="Name">
            {(id) => (
              <Input
                id={id}
                value={name}
                autoComplete="off"
                onChange={(event) => setName(event.target.value)}
              />
            )}
          </Field>
          {tried && <Problems items={problems} />}
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit">Review</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export function EditProfileDialog({
  profile,
  servers,
  users,
  existing,
  open,
  onOpenChange,
  onSubmit,
}: {
  profile: ProfileData;
  servers: ServerView[];
  /** Clients that follow this profile. */
  users: string[];
  existing: string[];
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (edit: ProfileEdit, argv: string[]) => void;
}) {
  const [name, setName] = useState(profile.name);
  const [chosen, setChosen] = useState(() => profile.servers.map((server) => server.id));
  const edit = { name, servers: chosen };
  const argv = profileEditArgv(profile, edit);
  const changed = argv.length > 3;
  const trimmed = name.trim();
  const problems = !trimmed
    ? ["Give the profile a name"]
    : trimmed.toLowerCase() !== profile.name.toLowerCase() &&
        existing.some((other) => other.toLowerCase() === trimmed.toLowerCase())
      ? [`A profile called ${trimmed} already exists`]
      : [];
  const toggle = (id: string, on: boolean) =>
    setChosen((prev) => (on ? [...prev, id] : prev.filter((entry) => entry !== id)));
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Edit {profile.name}</DialogTitle>
          <DialogDescription>
            {users.length > 0
              ? `Used by ${users.join(", ")}. A change reaches them with their next session.`
              : "No client uses this profile yet."}
          </DialogDescription>
        </DialogHeader>
        <form
          onSubmit={(event) => {
            event.preventDefault();
            if (changed && problems.length === 0) onSubmit(edit, argv);
          }}
          className="flex flex-col gap-3"
        >
          <Field label="Name">
            {(id) => (
              <Input
                id={id}
                value={name}
                autoComplete="off"
                onChange={(event) => setName(event.target.value)}
              />
            )}
          </Field>
          <fieldset className="flex flex-col gap-1">
            <legend className="mb-1 text-sm font-medium">Servers in this profile</legend>
            <ul className="flex max-h-64 flex-col overflow-auto rounded-lg border">
              {servers.map((server) => (
                <li key={server.id} className="border-b last:border-b-0">
                  <label className="flex items-center gap-2 px-3 py-1.5 text-sm">
                    <input
                      type="checkbox"
                      checked={chosen.includes(server.id)}
                      onChange={(event) => toggle(server.id, event.target.checked)}
                    />
                    <span className="min-w-0 flex-1 truncate">{server.name}</span>
                    <span className="text-xs text-muted-foreground">
                      {server.transport}
                    </span>
                  </label>
                </li>
              ))}
              {servers.length === 0 && (
                <li className="px-3 py-2 text-sm text-muted-foreground">
                  There are no servers to choose from.
                </li>
              )}
            </ul>
          </fieldset>
          <Problems items={problems} />
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={!changed || problems.length > 0}>
              Review changes
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export function DeleteProfileDialog({
  profile,
  users,
  open,
  onOpenChange,
  onContinue,
}: {
  profile: ProfileData;
  users: string[];
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onContinue: (options: { noClients: boolean }) => void;
}) {
  const [noClients, setNoClients] = useState(false);
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Delete {profile.name}</DialogTitle>
          <DialogDescription>
            The {profile.servers.length} server{profile.servers.length === 1 ? "" : "s"}{" "}
            in it stay in the registry.
            {users.length > 0 && ` Used by ${users.join(", ")}.`} Next you see exactly
            what would change.
          </DialogDescription>
        </DialogHeader>
        <label className="flex items-start gap-2 text-sm">
          <input
            type="checkbox"
            className="mt-0.5"
            checked={noClients}
            onChange={(event) => setNoClients(event.target.checked)}
          />
          <span>
            Leave the client entries scoped to it in place <Code>--no-clients</Code>
          </span>
        </label>
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button onClick={() => onContinue({ noClients })}>Preview deletion</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

const STATE_BADGE: Record<
  InspectRow["state"],
  { label: string; variant: "secondary" | "success" | "warning" | "destructive" }
> = {
  pending: { label: "Waiting", variant: "secondary" },
  ok: { label: "Answered", variant: "success" },
  login: { label: "Needs a login", variant: "warning" },
  failed: { label: "Failed", variant: "destructive" },
};

function InspectRowView({ row }: { row: InspectRow }) {
  const badge = STATE_BADGE[row.state];
  return (
    <li className="flex flex-col gap-1 px-3 py-2 text-sm">
      <div className="flex flex-wrap items-center gap-2">
        {row.state === "pending" && (
          <Loader2 className="size-3.5 animate-spin" aria-hidden="true" />
        )}
        <span className="font-medium">{row.name}</span>
        <Badge variant={badge.variant}>{badge.label}</Badge>
        {row.state === "ok" && (
          <span className="text-xs text-muted-foreground">
            {row.tools.length} tool{row.tools.length === 1 ? "" : "s"}
          </span>
        )}
      </div>
      {row.message && (
        <p className="text-xs break-words text-muted-foreground">{row.message}</p>
      )}
      {row.state === "ok" && row.tools.length > 0 && (
        <details>
          <summary className="cursor-pointer text-xs text-muted-foreground">
            Show the tools of {row.name}
          </summary>
          <ul className="mt-1 flex flex-col gap-0.5">
            {row.tools.map((tool) => (
              <li key={tool.name} className="text-xs">
                <code className="font-mono">{tool.name}</code>
                {tool.description && (
                  <span className="text-muted-foreground"> {tool.description}</span>
                )}
              </li>
            ))}
          </ul>
        </details>
      )}
    </li>
  );
}

/** Asks a whole profile for its tools and keeps what each server says, so a server that needs
 * a login is one row, not the end of the list. */
export function InspectProfileDialog({
  profile,
  onClose,
  onOpenLogins,
}: {
  profile: ProfileData;
  onClose: () => void;
  onOpenLogins: () => void;
}) {
  const [run, setRun] = useState<InspectRun | null>(null);

  useEffect(() => {
    const abort = new AbortController();
    void inspectProfile(profile, { signal: abort.signal, onUpdate: setRun });
    return () => abort.abort();
  }, [profile]);

  const rows = run?.rows ?? [];
  const sum = summarize(rows);
  const running = !run || !run.done;
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>Inspect {profile.name}</DialogTitle>
          <DialogDescription>
            Connects to each server in the profile and lists its tools.
          </DialogDescription>
        </DialogHeader>
        {running && (
          <p role="status" className="flex items-center gap-2 text-sm">
            <Loader2 className="size-4 animate-spin" aria-hidden="true" />
            Asking{" "}
            {sum.pending > 0
              ? `${sum.pending} server${sum.pending === 1 ? "" : "s"}`
              : "the servers"}
            …
          </p>
        )}
        {run?.stoppedAt && (
          <Callout variant="info" role="status">
            The whole-profile command stopped at: {run.stoppedAt}. Each server was asked
            on its own instead.
          </Callout>
        )}
        {run?.done && (
          <p role="status" className="text-sm">
            {rows.length === 0
              ? "This profile has no servers."
              : `${sum.ok} of ${rows.length} servers answered with ${sum.tools} tools` +
                (sum.login > 0 ? `, ${sum.login} need a login` : "") +
                (sum.failed > 0 ? `, ${sum.failed} failed` : "") +
                "."}
          </p>
        )}
        {rows.length > 0 && (
          <ul
            aria-label="Servers in the profile"
            className="flex max-h-80 flex-col divide-y overflow-auto rounded-lg border"
          >
            {rows.map((row) => (
              <InspectRowView key={row.id} row={row} />
            ))}
          </ul>
        )}
        <DialogFooter>
          {sum.login > 0 && (
            <Button variant="outline" onClick={onOpenLogins}>
              Open Logins
            </Button>
          )}
          <Button variant={running ? "outline" : "default"} onClick={onClose}>
            {running ? "Cancel" : "Close"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
