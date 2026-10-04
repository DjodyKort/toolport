import { useState } from "react";
import { Terminal } from "lucide-react";
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
import { commandLine } from "../allcommands/model";
import type { ClientImportData } from "../types/client";
import { AsyncView, CopyButton, useCtlQuery } from "../ui";
import { Code, Field, Pill, SELECT_CLASS } from "./atoms";
import type { ClientView, DirectEntry, ProfileData, ServerView } from "./model";
import { stateLabel, stateTone } from "./model";

const GATEWAY: Record<
  string,
  { label: string; variant: "success" | "warning" | "secondary" }
> = {
  managed: { label: "managed", variant: "success" },
  customized: { label: "customized", variant: "warning" },
  absent: { label: "not set up", variant: "secondary" },
};

export function GatewayBadge({ state }: { state: string }) {
  const found = GATEWAY[state] ?? { label: state, variant: "secondary" as const };
  return <Badge variant={found.variant}>{found.label}</Badge>;
}

const LAUNCHER_STATE: Record<string, { label: string; variant: "success" | "warning" }> =
  {
    ok: { label: "in sync", variant: "success" },
    stale: { label: "out of date", variant: "warning" },
    customized: { label: "edited by hand", variant: "warning" },
    missing: { label: "missing", variant: "warning" },
    orphan: { label: "server removed", variant: "warning" },
    unrecorded: { label: "not recorded", variant: "warning" },
  };

/** The line a direct entry runs. It starts a process in a terminal of its own, so the app only
 * shows it for copying. */
function DirectRun({ entry }: { entry: DirectEntry }) {
  const line = commandLine(["direct", "run", entry.server]);
  return (
    <div className="mt-1 flex flex-col gap-2 rounded-md border bg-muted/40 p-2">
      <p className="text-xs text-muted-foreground">
        The client starts this entry with the command below. It needs a terminal, so it
        cannot run from here.
      </p>
      <div className="flex flex-wrap items-center gap-2">
        <code aria-label="Command line" className="min-w-0 font-mono text-xs break-all">
          {line}
        </code>
        <CopyButton text={line} label="Copy command" />
        <Button
          size="sm"
          variant="outline"
          disabled
          title="Not available yet: copy the command and run it in a terminal"
        >
          <Terminal /> Open in Terminal
        </Button>
      </div>
    </div>
  );
}

export function ClientDetailsDialog({
  view,
  profiles,
  servers,
  onClose,
  onSetProfile,
  onSync,
  onImport,
  onDirectAdd,
  onDirectRm,
  busy,
}: {
  view: ClientView;
  profiles: ProfileData[];
  servers: ServerView[];
  onClose: () => void;
  onSetProfile: (profileId: string) => void;
  onSync: () => void;
  onImport: () => void;
  onDirectAdd: (serverId: string) => void;
  onDirectRm: (entry: DirectEntry) => void;
  busy: boolean;
}) {
  const [profile, setProfile] = useState(view.profile?.id ?? profiles[0]?.id ?? "");
  const [server, setServer] = useState("");
  const withLauncher = new Set(view.launchers.map((entry) => entry.server));
  const addable = servers.filter((candidate) => !withLauncher.has(candidate.id));
  const gateway = view.gateway === "managed";
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-h-[90vh] overflow-auto sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            {view.name} <GatewayBadge state={view.gateway} />
          </DialogTitle>
          <DialogDescription>
            <Code>{view.path}</Code>
          </DialogDescription>
        </DialogHeader>

        <section aria-label="What this client sees" className="flex flex-col gap-2">
          <h3 className="text-2xs font-semibold tracking-[0.09em] text-muted-foreground uppercase">
            What this client sees
          </h3>
          {!gateway ? (
            <p className="text-sm text-muted-foreground">
              {view.gateway === "customized"
                ? "The toolport entry was edited by hand, so Toolport cannot say what this client gets."
                : "Toolport is not set up in this client yet. Sync adds the toolport entry."}
            </p>
          ) : view.seen.length === 0 ? (
            <p className="text-sm text-muted-foreground">
              {view.profile
                ? `The profile ${view.profile.name} has no servers.`
                : "The profile it uses has no servers."}
            </p>
          ) : (
            <>
              <p className="text-sm">
                {view.tools.toLocaleString("en")} tools from {view.connectedServers} of{" "}
                {view.seen.length} servers, through the profile{" "}
                <b>{view.profile?.name ?? "it uses"}</b>
                {view.followsActive ? " (the active one)" : ""}.
              </p>
              <ul className="flex flex-col divide-y rounded-lg border">
                {view.seen.map((row) => (
                  <li
                    key={row.id}
                    className="flex items-center gap-2 px-3 py-1.5 text-sm"
                  >
                    <span className="min-w-0 flex-1 truncate">{row.name}</span>
                    {row.state === "connected" ? (
                      <span className="text-xs text-muted-foreground tabular-nums">
                        {row.tools ?? 0} tools
                      </span>
                    ) : (
                      <Badge
                        variant={
                          stateTone({ state: row.state, expiring: false }) ===
                          "destructive"
                            ? "destructive"
                            : "secondary"
                        }
                      >
                        {stateLabel({ state: row.state, expiring: false })}
                      </Badge>
                    )}
                  </li>
                ))}
              </ul>
            </>
          )}
        </section>

        <section aria-label="Profile" className="flex flex-col gap-2">
          <Field label="Profile this client uses">
            {(id) => (
              <div className="flex gap-2">
                <select
                  id={id}
                  className={SELECT_CLASS}
                  value={profile}
                  onChange={(event) => setProfile(event.target.value)}
                >
                  {profiles.map((entry) => (
                    <option key={entry.id} value={entry.id}>
                      {entry.name}
                      {entry.active ? " (active)" : ""}
                    </option>
                  ))}
                </select>
                <Button
                  variant="outline"
                  disabled={
                    busy ||
                    !profile ||
                    (profile === view.profile?.id && !view.followsActive)
                  }
                  onClick={() => onSetProfile(profile)}
                >
                  Set profile
                </Button>
              </div>
            )}
          </Field>
          <p className="text-xs text-muted-foreground">
            {view.followsActive
              ? "It has no profile of its own, so it follows the active profile."
              : "It is pointed at its own profile."}
          </p>
        </section>

        <section aria-label="Direct entries" className="flex flex-col gap-2">
          <h3 className="text-2xs font-semibold tracking-[0.09em] text-muted-foreground uppercase">
            Direct entries
          </h3>
          <p className="text-xs text-muted-foreground">
            A direct entry starts a server by itself and bypasses the gateway.
          </p>
          {view.launchers.length === 0 &&
            view.orphans.length === 0 &&
            view.redundant.length === 0 && (
              <p className="text-sm text-muted-foreground">
                This client has no direct entries.
              </p>
            )}
          {view.launchers.length > 0 && (
            <ul
              aria-label="Direct launcher entries"
              className="flex flex-col divide-y rounded-lg border"
            >
              {view.launchers.map((entry) => {
                const state = LAUNCHER_STATE[entry.state] ?? {
                  label: entry.state,
                  variant: "warning" as const,
                };
                return (
                  <li key={entry.entry} className="flex flex-col gap-1 px-3 py-2">
                    <div className="flex flex-wrap items-center gap-2 text-sm">
                      <span className="font-medium">{entry.entry}</span>
                      <Badge variant={state.variant}>{state.label}</Badge>
                      <span className="flex-1" />
                      <Button
                        size="sm"
                        variant="outline"
                        disabled={busy}
                        aria-label={`Remove the direct entry ${entry.entry}`}
                        onClick={() => onDirectRm(entry)}
                      >
                        Remove
                      </Button>
                    </div>
                    <details>
                      <summary className="cursor-pointer text-xs text-muted-foreground">
                        How the client starts it
                      </summary>
                      <DirectRun entry={entry} />
                    </details>
                  </li>
                );
              })}
            </ul>
          )}
          {(view.orphans.length > 0 || view.redundant.length > 0) && (
            <ul
              aria-label="Entries Toolport did not write"
              className="flex flex-col divide-y rounded-lg border"
            >
              {view.orphans.map((entry) => (
                <li
                  key={`o:${entry}`}
                  className="flex items-center gap-2 px-3 py-1.5 text-sm"
                >
                  <span className="flex-1">{entry}</span>
                  <Badge variant="warning">orphan</Badge>
                </li>
              ))}
              {view.redundant.map((entry) => (
                <li
                  key={`r:${entry}`}
                  className="flex items-center gap-2 px-3 py-1.5 text-sm"
                >
                  <span className="flex-1">{entry}</span>
                  <Badge variant="secondary">duplicate of a server</Badge>
                </li>
              ))}
            </ul>
          )}
          <div className="flex flex-wrap items-end gap-2">
            <Field label="Add a direct entry for" className="min-w-48 flex-1">
              {(id) => (
                <select
                  id={id}
                  className={SELECT_CLASS}
                  value={server}
                  onChange={(event) => setServer(event.target.value)}
                >
                  <option value="">Choose a server</option>
                  {addable.map((candidate) => (
                    <option key={candidate.id} value={candidate.id}>
                      {candidate.name}
                    </option>
                  ))}
                </select>
              )}
            </Field>
            <Button
              variant="outline"
              disabled={busy || !server}
              onClick={() => onDirectAdd(server)}
            >
              Add direct entry
            </Button>
          </div>
        </section>

        <DialogFooter className="sm:justify-between">
          <div className="flex flex-wrap gap-2">
            <Button variant="outline" disabled={busy} onClick={onImport}>
              Import entries…
            </Button>
            <Button variant="outline" disabled={busy} onClick={onSync}>
              Sync this client…
            </Button>
          </div>
          <Button onClick={onClose}>Close</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

const IMPORT_WHY: Record<string, string> = {
  importable: "",
  already: "Already in the registry",
  "name-taken": "A different server has this name",
  credential: "Holds an inline credential, so it is not imported",
};

export function ImportDialog({
  view,
  profiles,
  onClose,
  onSubmit,
}: {
  view: ClientView;
  profiles: ProfileData[];
  onClose: () => void;
  onSubmit: (selected: string[], profile: string) => void;
}) {
  const query = useCtlQuery<ClientImportData>(["client", "import", view.id, "--dry-run"]);
  const [chosen, setChosen] = useState<string[]>([]);
  const [profile, setProfile] = useState("");
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>Import from {view.name}</DialogTitle>
          <DialogDescription>
            Entries in the client's own config that Toolport did not write can become
            servers in the registry. Entries with an inline credential are never imported.
          </DialogDescription>
        </DialogHeader>
        <AsyncView
          query={query}
          errorTitle="Couldn't read the client's entries"
          context={`client import ${view.id}`}
          isEmpty={(data) => data.direct.length === 0}
          empty={
            <p className="text-sm text-muted-foreground">
              {view.name} has no direct entries to import.
            </p>
          }
        >
          {(data) => (
            <div className="flex flex-col gap-3">
              <ul
                aria-label="Direct entries"
                className="flex flex-col divide-y rounded-lg border"
              >
                {data.direct.map((entry) => {
                  const ok = entry.status === "importable";
                  return (
                    <li key={entry.name}>
                      <label className="flex items-start gap-2 px-3 py-2 text-sm">
                        <input
                          type="checkbox"
                          className="mt-0.5"
                          disabled={!ok}
                          checked={chosen.includes(entry.name)}
                          onChange={(event) =>
                            setChosen((prev) =>
                              event.target.checked
                                ? [...prev, entry.name]
                                : prev.filter((name) => name !== entry.name),
                            )
                          }
                        />
                        <span className="min-w-0 flex-1">
                          <span className="flex items-center gap-2 font-medium">
                            {entry.name} <Pill>{entry.transport}</Pill>
                          </span>
                          <code className="block truncate font-mono text-xs text-muted-foreground">
                            {entry.target}
                          </code>
                          {IMPORT_WHY[entry.status] && (
                            <span className="text-xs text-muted-foreground">
                              {IMPORT_WHY[entry.status]}
                            </span>
                          )}
                        </span>
                      </label>
                    </li>
                  );
                })}
              </ul>
              <Field
                label="Put them in a profile"
                hint="Optional. An existing profile name, or a new one."
              >
                {(id, hint) => (
                  <Input
                    id={id}
                    aria-describedby={hint}
                    list="import-profile-list"
                    value={profile}
                    autoComplete="off"
                    onChange={(event) => setProfile(event.target.value)}
                  />
                )}
              </Field>
              <datalist id="import-profile-list">
                {profiles.map((entry) => (
                  <option key={entry.id} value={entry.name} />
                ))}
              </datalist>
              {chosen.length === 0 && (
                <Callout variant="info">Choose the entries to import.</Callout>
              )}
              <DialogFooter>
                <Button variant="ghost" onClick={onClose}>
                  Cancel
                </Button>
                <Button
                  disabled={chosen.length === 0}
                  onClick={() => onSubmit(chosen, profile.trim())}
                >
                  Review import
                </Button>
              </DialogFooter>
            </div>
          )}
        </AsyncView>
      </DialogContent>
    </Dialog>
  );
}

export function SyncDialog({
  clients,
  only,
  onClose,
  onSubmit,
}: {
  clients: ClientView[];
  /** Limit the sync to one client. */
  only: string | null;
  onClose: () => void;
  onSubmit: (options: { client: string | null; keepOrphans: boolean }) => void;
}) {
  const [client, setClient] = useState(only ?? "");
  const [keepOrphans, setKeepOrphans] = useState(false);
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Sync clients</DialogTitle>
          <DialogDescription>
            Brings each client's config in line with the registry: adds the toolport entry
            where it is missing and removes entries a server already covers. Next you see
            exactly what would change.
          </DialogDescription>
        </DialogHeader>
        <Field label="Clients">
          {(id) => (
            <select
              id={id}
              className={SELECT_CLASS}
              value={client}
              onChange={(event) => setClient(event.target.value)}
            >
              <option value="">Every managed client</option>
              {clients.map((entry) => (
                <option key={entry.id} value={entry.id}>
                  {entry.name}
                </option>
              ))}
            </select>
          )}
        </Field>
        <label className="flex items-start gap-2 text-sm">
          <input
            type="checkbox"
            className="mt-0.5"
            checked={keepOrphans}
            onChange={(event) => setKeepOrphans(event.target.checked)}
          />
          <span>
            Keep entries that no server owns <Code>--keep-orphans</Code>
          </span>
        </label>
        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={() => onSubmit({ client: client || null, keepOrphans })}>
            Preview sync
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
