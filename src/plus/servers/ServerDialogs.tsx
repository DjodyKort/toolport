import { useState, type FormEvent } from "react";
import { Search } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { ctlData } from "../bridge/ctl";
import type { ServerSearchData } from "../types/server";
import { ScreenSkeleton, Tabs, errorText } from "../ui";
import { Chip, Code, Field, Pill, Problems, SELECT_CLASS } from "./atoms";
import {
  editPlan,
  emptyForm,
  fieldsOf,
  formOf,
  newProblems,
  type EditPlan,
  type ServerForm,
} from "./forms";
import type { ServerInfo, ServerView } from "./model";
import type { CatalogRow, ServerFields } from "./plans";

type CatalogState =
  | { status: "idle" }
  | { status: "running" }
  | { status: "ready"; data: ServerSearchData }
  | { status: "error"; message: string; code: string | null };

function CatalogSearch({
  onInstall,
}: {
  onInstall: (entry: CatalogRow, offline: boolean) => void;
}) {
  const [query, setQuery] = useState("");
  const [offline, setOffline] = useState(false);
  const [state, setState] = useState<CatalogState>({ status: "idle" });

  async function search(event: FormEvent | null, forceOffline = offline) {
    event?.preventDefault();
    const text = query.trim();
    if (!text) return;
    setOffline(forceOffline);
    setState({ status: "running" });
    try {
      const data = await ctlData<ServerSearchData>([
        "server",
        "search",
        text,
        "--limit",
        "20",
        ...(forceOffline ? ["--offline"] : []),
      ]);
      setState({ status: "ready", data });
    } catch (error) {
      const { code, message } = errorText(error);
      setState({ status: "error", code, message });
    }
  }

  return (
    <div className="flex flex-col gap-3">
      <form
        role="search"
        aria-label="Search the catalog"
        onSubmit={(event) => void search(event)}
        className="flex gap-2"
      >
        <div className="relative flex-1">
          <Search
            className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground"
            aria-hidden="true"
          />
          <Input
            aria-label="Catalog search"
            placeholder="Name or keyword"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            className="pl-8"
          />
        </div>
        <Button type="submit" disabled={!query.trim() || state.status === "running"}>
          Search
        </Button>
      </form>
      {state.status === "idle" && (
        <p className="text-sm text-muted-foreground">
          Search the catalog for a server to add. It needs the network; Search offline
          uses the copy on this computer.
        </p>
      )}
      {state.status === "running" && <ScreenSkeleton rows={3} label="Searching" />}
      {state.status === "error" && (
        <Callout variant="danger" role="alert" className="flex flex-col gap-2">
          <p>
            {state.code && <code className="mr-1.5 font-mono text-xs">{state.code}</code>}
            {state.message}
          </p>
          <div>
            <Button size="sm" variant="outline" onClick={() => void search(null, true)}>
              Search offline
            </Button>
          </div>
        </Callout>
      )}
      {state.status === "ready" && state.data.results.length === 0 && (
        <p className="text-sm text-muted-foreground">
          No catalog entry matches {state.data.query}.
        </p>
      )}
      {state.status === "ready" && state.data.results.length > 0 && (
        <ul
          aria-label="Catalog results"
          className="flex flex-col divide-y rounded-lg border"
        >
          {state.data.results.map((entry) => (
            <li
              key={`${entry.source}:${entry.name}`}
              className="flex items-start gap-3 p-3"
            >
              <div className="flex min-w-0 flex-1 flex-col gap-1">
                <p className="flex flex-wrap items-center gap-2 text-sm font-medium">
                  {entry.name}
                  <Pill>{entry.transport}</Pill>
                  <Pill>{entry.source}</Pill>
                </p>
                <p className="text-xs text-muted-foreground">{entry.description}</p>
                {entry.envKeys.length > 0 && (
                  <p className="flex flex-wrap items-center gap-1 text-xs text-muted-foreground">
                    Needs
                    {entry.envKeys.map((key) => (
                      <Chip key={key}>{key}</Chip>
                    ))}
                  </p>
                )}
              </div>
              <Button
                size="sm"
                aria-label={`Install ${entry.name}`}
                onClick={() => onInstall(entry, offline)}
              >
                Install
              </Button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function ServerFormFields({
  form,
  onChange,
  locked,
}: {
  form: ServerForm;
  onChange: (next: ServerForm) => void;
  /** An existing server keeps its kind. */
  locked?: boolean;
}) {
  const set = <K extends keyof ServerForm>(key: K, value: ServerForm[K]) =>
    onChange({ ...form, [key]: value });
  return (
    <div className="flex flex-col gap-3">
      <Field label="Name">
        {(id) => (
          <Input
            id={id}
            value={form.name}
            autoComplete="off"
            onChange={(event) => set("name", event.target.value)}
          />
        )}
      </Field>
      {!locked && (
        <Field label="Starts with">
          {(id) => (
            <select
              id={id}
              className={SELECT_CLASS}
              value={form.kind}
              onChange={(event) => set("kind", event.target.value as ServerForm["kind"])}
            >
              <option value="command">A command on this computer</option>
              <option value="url">A web address</option>
            </select>
          )}
        </Field>
      )}
      {form.kind === "command" ? (
        <>
          <Field label="Command">
            {(id) => (
              <Input
                id={id}
                value={form.command}
                autoComplete="off"
                spellCheck={false}
                placeholder="npx"
                onChange={(event) => set("command", event.target.value)}
              />
            )}
          </Field>
          <Field label="Arguments" hint="One per line.">
            {(id, hint) => (
              <Textarea
                id={id}
                aria-describedby={hint}
                rows={3}
                spellCheck={false}
                value={form.args}
                onChange={(event) => set("args", event.target.value)}
              />
            )}
          </Field>
        </>
      ) : (
        <>
          <Field label="Address">
            {(id) => (
              <Input
                id={id}
                value={form.url}
                autoComplete="off"
                spellCheck={false}
                placeholder="https://example.com/mcp"
                onChange={(event) => set("url", event.target.value)}
              />
            )}
          </Field>
          <Field label="Transport">
            {(id) => (
              <select
                id={id}
                className={SELECT_CLASS}
                value={form.transport}
                onChange={(event) => set("transport", event.target.value)}
              >
                <option value="">Default</option>
                <option value="http">http</option>
                <option value="sse">sse</option>
              </select>
            )}
          </Field>
        </>
      )}
      <Field label="Working folder" hint="Optional. Where the command starts.">
        {(id, hint) => (
          <Input
            id={id}
            aria-describedby={hint}
            value={form.cwd}
            autoComplete="off"
            spellCheck={false}
            onChange={(event) => set("cwd", event.target.value)}
          />
        )}
      </Field>
    </div>
  );
}

export function AddServerDialog({
  open,
  onOpenChange,
  existing,
  onInstall,
  onCreate,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  existing: string[];
  onInstall: (entry: CatalogRow, offline: boolean) => void;
  onCreate: (fields: ServerFields) => void;
}) {
  const [tab, setTab] = useState("catalog");
  const [form, setForm] = useState<ServerForm>(emptyForm);
  const [tried, setTried] = useState(false);
  const problems = newProblems(form, existing);

  function submit(event: FormEvent) {
    event.preventDefault();
    setTried(true);
    if (problems.length === 0) onCreate(fieldsOf(form));
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>Add server</DialogTitle>
          <DialogDescription>
            Take one from the catalog or describe your own. You see what changes before
            anything is written.
          </DialogDescription>
        </DialogHeader>
        <Tabs
          label="Add server"
          value={tab}
          onValueChange={setTab}
          items={[
            { id: "catalog", label: "From the catalog" },
            { id: "custom", label: "Custom server" },
          ]}
        >
          {tab === "catalog" ? (
            <CatalogSearch onInstall={onInstall} />
          ) : (
            <form onSubmit={submit} className="flex flex-col gap-3">
              <ServerFormFields form={form} onChange={setForm} />
              {tried && <Problems items={problems} />}
              <DialogFooter>
                <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
                  Cancel
                </Button>
                <Button type="submit">Review</Button>
              </DialogFooter>
            </form>
          )}
        </Tabs>
      </DialogContent>
    </Dialog>
  );
}

export function EditServerDialog({
  info,
  open,
  onOpenChange,
  onSubmit,
}: {
  info: ServerInfo;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (plan: EditPlan) => void;
}) {
  const [form, setForm] = useState<ServerForm>(() => formOf(info));
  const plan = editPlan(info, form);
  const unchanged = plan.changes.length === 0;
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>Edit {info.name}</DialogTitle>
          <DialogDescription>
            Environment values are not shown here. Secrets stay in the vault.
          </DialogDescription>
        </DialogHeader>
        <form
          onSubmit={(event) => {
            event.preventDefault();
            if (!unchanged && plan.problems.length === 0) onSubmit(plan);
          }}
          className="flex flex-col gap-3"
        >
          <ServerFormFields form={form} onChange={setForm} locked />
          <fieldset className="flex flex-col gap-1.5 text-sm">
            <legend className="mb-1 font-medium">Options</legend>
            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={form.declareClientCapabilities}
                onChange={(event) =>
                  setForm({ ...form, declareClientCapabilities: event.target.checked })
                }
              />
              Declare the client's capabilities to the server
            </label>
            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={form.forwardInstructions}
                onChange={(event) =>
                  setForm({ ...form, forwardInstructions: event.target.checked })
                }
              />
              Forward the server's instructions to clients
            </label>
          </fieldset>
          <Problems items={plan.problems} />
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={unchanged || plan.problems.length > 0}>
              Review changes
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export interface RemoveOptions {
  keepClients: boolean;
  keepSecrets: boolean;
}

export function RemoveServerDialog({
  view,
  open,
  onOpenChange,
  onContinue,
}: {
  view: ServerView;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onContinue: (options: RemoveOptions) => void;
}) {
  const [keepClients, setKeepClients] = useState(false);
  const [keepSecrets, setKeepSecrets] = useState(false);
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Remove {view.name}</DialogTitle>
          <DialogDescription>
            Next you see exactly what would change. Nothing is written until you confirm
            it.
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-2 text-sm">
          <label className="flex items-start gap-2">
            <input
              type="checkbox"
              className="mt-0.5"
              checked={keepClients}
              onChange={(event) => setKeepClients(event.target.checked)}
            />
            <span>
              Keep its entries in client configs <Code>--keep-clients</Code>
            </span>
          </label>
          <label className="flex items-start gap-2">
            <input
              type="checkbox"
              className="mt-0.5"
              checked={keepSecrets}
              onChange={(event) => setKeepSecrets(event.target.checked)}
            />
            <span>
              Keep its secrets in the vault <Code>--keep-secrets</Code>
            </span>
          </label>
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button onClick={() => onContinue({ keepClients, keepSecrets })}>
            Preview removal
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
