import { useState } from "react";
import {
  ClipboardCopy,
  Download,
  FolderPlus,
  GitCommitHorizontal,
  MoreHorizontal,
  Plus,
  RefreshCw,
  Search,
  Upload,
  X,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { SkeletonRows } from "@/components/ui/skeleton";
import { toast } from "sonner";
import { toastError } from "@/lib/toast";
import { useLoad, type Loaded } from "../servers/useLoad";
import { errorText } from "../settings/useCtl";
import {
  addRoot,
  addTap,
  installSkill,
  listLibrary,
  listRoots,
  listSources,
  listTaps,
  remoteStatus,
  removeRoot,
  removeTap,
  searchTaps,
  updateTaps,
} from "./commands";
import type { SearchHit, SourceRow, SourcesData, TapUpdateData } from "./data";
import { AskDialog, FileLines, Mono, WriteDialog } from "./dialogs";
import {
  canPull,
  gitStatusText,
  plural,
  pullBlockedReason,
  remoteState,
  when,
} from "./model";
import { AddRemoteDialog, CommitDialog, PullDialog, PushDialog } from "./writes";

const SELECT =
  "h-8 rounded-md border bg-background px-2 text-sm text-foreground focus-visible:ring-3 focus-visible:ring-ring/50 focus-visible:outline-none";

function Heading({
  title,
  count,
  note,
  children,
}: {
  title: string;
  count?: number;
  note?: string;
  children?: React.ReactNode;
}) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-2">
      <h2 className="text-meta font-semibold tracking-wide text-muted-foreground uppercase">
        {title}
        {count !== undefined && (
          <span className="ml-1 font-normal normal-case">{count}</span>
        )}
      </h2>
      <div className="flex flex-wrap items-center gap-2 text-meta text-muted-foreground">
        {note}
        {children}
      </div>
    </div>
  );
}

function RemoteCard() {
  const [fetchTick, setFetchTick] = useState(0);
  const status = useLoad(() => remoteStatus(fetchTick > 0), fetchTick);
  const [dialog, setDialog] = useState<"pull" | "push" | "commit" | "remote" | null>(
    null,
  );
  const data = status.data;
  const state = remoteState(data);
  const reason = pullBlockedReason(state);

  async function copyStatus() {
    if (!data) return;
    try {
      await navigator.clipboard.writeText(gitStatusText(data, null));
      toast.message("Git status copied");
    } catch (error) {
      toastError("Could not copy", { description: errorText(error) });
    }
  }

  const label =
    state.kind === "dirty"
      ? "Behind and edited"
      : state.kind === "behind"
        ? "Behind"
        : state.kind === "diverged"
          ? "Diverged"
          : state.kind === "offline"
            ? "Could not reach origin"
            : state.kind === "no-remote"
              ? "Local library"
              : state.kind === "unpublished"
                ? "Not pushed yet"
                : state.kind === "ahead"
                  ? "Ahead"
                  : "Up to date";
  return (
    <section aria-label="Library remote" className="rounded-xl border bg-card p-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <h2 className="text-sm font-semibold">Library remote</h2>
          {data && (
            <Badge variant={state.kind === "current" ? "success" : "warning"}>
              {label}
            </Badge>
          )}
        </div>
        {data && (
          <div className="flex flex-wrap gap-2">
            {state.kind === "no-remote" && (
              <Button variant="outline" size="sm" onClick={() => setDialog("remote")}>
                <Plus /> Add remote…
              </Button>
            )}
            {state.kind !== "no-remote" && (
              <>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => setFetchTick((n) => n + 1)}
                >
                  <RefreshCw /> Fetch
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={!canPull(state)}
                  onClick={() => setDialog("pull")}
                >
                  <Download /> Pull
                  {data.behind > 0 ? ` ${plural(data.behind, "commit")}` : ""}
                </Button>
              </>
            )}
            {data.uncommitted > 0 && (
              <Button variant="outline" size="sm" onClick={() => setDialog("commit")}>
                <GitCommitHorizontal /> Commit…
              </Button>
            )}
            {state.kind !== "no-remote" && (
              <Button variant="outline" size="sm" onClick={() => setDialog("push")}>
                <Upload /> {data.uncommitted > 0 ? "Commit and push…" : "Push…"}
              </Button>
            )}
            <Button variant="outline" size="sm" onClick={() => void copyStatus()}>
              <ClipboardCopy /> Copy git status
            </Button>
          </div>
        )}
      </div>
      {status.error && !data && (
        <p role="alert" className="mt-2 text-meta text-destructive">
          {status.error}
        </p>
      )}
      {!data && !status.error && (
        <p className="mt-2 text-meta text-muted-foreground">Reading the library…</p>
      )}
      {data && (
        <dl className="mt-3 grid grid-cols-[6.5rem_minmax(0,1fr)] gap-x-3 gap-y-1 text-meta">
          <dt className="text-muted-foreground">Remote</dt>
          <dd className="font-mono text-xs break-all">{data.remote ?? "none"}</dd>
          <dt className="text-muted-foreground">Branch</dt>
          <dd className="font-mono text-xs">{data.branch}</dd>
          <dt className="text-muted-foreground">Against origin</dt>
          <dd>
            {data.ahead} ahead, {data.behind} behind{" "}
            <span className="text-muted-foreground">
              (fetched {when(data.lastFetch)})
            </span>
          </dd>
          <dt className="text-muted-foreground">Working copy</dt>
          <dd>
            {data.uncommitted === 0
              ? "Clean"
              : `${plural(data.uncommitted, "uncommitted change")}`}
          </dd>
          <dt className="text-muted-foreground">Credentials</dt>
          <dd>Through git and gh. Toolport never reads or stores them.</dd>
        </dl>
      )}
      {reason && <p className="mt-3 text-meta text-muted-foreground">{reason}</p>}
      {dialog === "pull" && (
        <PullDialog
          onClose={() => setDialog(null)}
          onDone={() => setFetchTick((n) => n + 1)}
        />
      )}
      {dialog === "push" && (
        <PushDialog
          uncommitted={data?.uncommitted ?? 0}
          onClose={() => setDialog(null)}
          onDone={() => setFetchTick((n) => n + 1)}
        />
      )}
      {dialog === "commit" && (
        <CommitDialog
          onClose={() => setDialog(null)}
          onDone={() => setFetchTick((n) => n + 1)}
        />
      )}
      {dialog === "remote" && (
        <AddRemoteDialog
          onClose={() => setDialog(null)}
          onDone={() => setFetchTick((n) => n + 1)}
        />
      )}
    </section>
  );
}

function Taps({ sources, onUpdated }: { sources: SourceRow[]; onUpdated: () => void }) {
  const taps = useLoad(listTaps, "taps");
  const [results, setResults] = useState<TapUpdateData["results"]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [dialog, setDialog] = useState<
    { kind: "add" } | { kind: "remove"; name: string } | null
  >(null);

  async function update(name?: string) {
    setBusy(name ?? "all");
    try {
      const data = await updateTaps(name);
      setResults((prev) => [
        ...prev.filter((r) => !data.results.some((d) => d.name === r.name)),
        ...data.results,
      ]);
      onUpdated();
    } catch (error) {
      toastError("Could not update the taps", { description: errorText(error) });
    } finally {
      setBusy(null);
    }
  }

  const rows = taps.data?.taps ?? [];
  return (
    <section aria-label="Taps" className="flex flex-col gap-2">
      <Heading title="Taps" count={rows.length}>
        {rows.length > 0 && (
          <Button
            variant="ghost"
            size="sm"
            disabled={busy !== null}
            onClick={() => void update()}
          >
            <RefreshCw className={busy === "all" ? "animate-spin" : undefined} /> Update
            all
          </Button>
        )}
        <Button variant="outline" size="sm" onClick={() => setDialog({ kind: "add" })}>
          <Plus /> Add tap…
        </Button>
      </Heading>
      {taps.error && !taps.data && (
        <p role="alert" className="text-meta text-destructive">
          {taps.error}
        </p>
      )}
      {taps.data && rows.length === 0 && (
        <p className="rounded-xl border bg-card px-4 py-4 text-meta text-muted-foreground">
          No taps. A tap is a git repository of skills you can search and install from.
        </p>
      )}
      {rows.length > 0 && (
        <ul className="divide-y overflow-hidden rounded-xl border bg-card">
          {rows.map((tap) => {
            const run = results.find((r) => r.name === tap.name);
            const known = sources.find((row) => row.id === `tap:${tap.name}`);
            const result =
              run ??
              (known?.status.state === "failed"
                ? { name: tap.name, ok: false, head: null, error: null }
                : undefined);
            return (
              <li key={tap.name} className="flex flex-wrap items-center gap-3 px-4 py-3">
                <div className="min-w-48 flex-1">
                  <p className="text-sm font-medium">{tap.name}</p>
                  <p className="text-meta text-muted-foreground">
                    <span className="font-mono text-xs">{tap.repo}</span>
                    {result?.error && <> · Could not update: {result.error}</>}
                    {!run && result && known && <> · {known.status.detail}</>}
                  </p>
                </div>
                {result && (
                  <Badge variant={result.ok ? "success" : "destructive"}>
                    {result.ok ? `Updated ${result.head ?? ""}`.trim() : "Update failed"}
                  </Badge>
                )}
                <Button
                  variant="outline"
                  size="sm"
                  disabled={busy !== null}
                  onClick={() => void update(tap.name)}
                >
                  {result && !result.ok ? "Retry" : "Update"}
                </Button>
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button
                      variant="ghost"
                      size="icon"
                      aria-label={`More for ${tap.name}`}
                    >
                      <MoreHorizontal />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end">
                    <DropdownMenuItem
                      variant="destructive"
                      onSelect={() => setDialog({ kind: "remove", name: tap.name })}
                    >
                      Remove tap…
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              </li>
            );
          })}
        </ul>
      )}
      {dialog?.kind === "add" && (
        <AskDialog
          title="Add tap"
          description="A tap is a git repository of skills. Toolport clones it and searches it."
          label="Repository"
          placeholder="user/repo or a git URL"
          onClose={() => setDialog(null)}
          next={(repo) => (
            <WriteDialog
              title="Add tap"
              confirmLabel="Add tap"
              preview={() => addTap(repo, true)}
              render={(data) => (
                <p className="text-meta">
                  Clones <Mono>{data.repo ?? repo}</Mono> as the tap{" "}
                  <span className="font-medium">{data.name}</span>.
                </p>
              )}
              confirm={() => addTap(repo, false)}
              onClose={() => setDialog(null)}
              onDone={taps.reload}
            />
          )}
        />
      )}
      {dialog?.kind === "remove" && (
        <WriteDialog
          title={`Remove ${dialog.name}`}
          description="Deletes the clone of the tap. Skills you installed from it stay in your library."
          confirmLabel="Remove tap"
          destructive
          preview={() => removeTap(dialog.name, true)}
          render={() => (
            <p className="text-meta">The clone is removed; this cannot be undone.</p>
          )}
          confirm={() => removeTap(dialog.name, false)}
          onClose={() => setDialog(null)}
          onDone={taps.reload}
        />
      )}
    </section>
  );
}

function Find({
  installed,
  onInstalled,
}: {
  installed: Set<string>;
  onInstalled: () => void;
}) {
  const [query, setQuery] = useState("");
  const [asked, setAsked] = useState<string | null>(null);
  const hits = useLoad(() => (asked ? searchTaps(asked) : Promise.resolve(null)), asked);
  const [pick, setPick] = useState<SearchHit | null>(null);
  return (
    <section aria-label="Find and install" className="flex flex-col gap-2">
      <Heading title="Find and install" note="Searches your taps" />
      <form
        className="relative"
        onSubmit={(event) => {
          event.preventDefault();
          setAsked(query.trim() || null);
        }}
      >
        <Search
          aria-hidden
          className="pointer-events-none absolute top-2 left-2.5 size-4 text-muted-foreground"
        />
        <Input
          aria-label="Search skills in taps"
          placeholder="Search skills in taps"
          className="pl-8"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
      </form>
      {hits.error && (
        <p role="alert" className="text-meta text-destructive">
          {hits.error}
        </p>
      )}
      {hits.data && hits.data.results.length === 0 && (
        <p className="text-meta text-muted-foreground">
          No skill in {plural(hits.data.tapCount, "tap")} matches “{hits.data.query}”.
        </p>
      )}
      {hits.data && hits.data.results.length > 0 && (
        <ul
          className="divide-y overflow-hidden rounded-xl border bg-card"
          aria-label="Results"
        >
          {hits.data.results.map((hit) => (
            <li
              key={`${hit.repo}/${hit.name}`}
              className="flex flex-wrap items-center gap-3 px-4 py-3"
            >
              <div className="min-w-48 flex-1">
                <p className="text-sm font-medium">{hit.name}</p>
                <p className="text-meta text-muted-foreground">
                  {hit.description} {hit.tap}
                </p>
              </div>
              {installed.has(hit.name) ? (
                <Badge variant="outline">Installed</Badge>
              ) : (
                <Button variant="outline" size="sm" onClick={() => setPick(hit)}>
                  Install…
                </Button>
              )}
            </li>
          ))}
        </ul>
      )}
      <p className="text-meta text-muted-foreground">
        Install copies the skill into your library folder. It then appears in the Sync
        plan like any other change; nothing reaches Claude Code until you apply.
      </p>
      {pick && (
        <WriteDialog
          title={`Install ${pick.name} from ${pick.tap}`}
          confirmLabel="Add to library"
          preview={() => installSkill(`@${pick.repo}/${pick.name}`, true)}
          blocked={(data) => (data.blocked ? "The audit blocked this install." : null)}
          tick={(data) =>
            data.audit.findings.length > 0
              ? `I read the ${plural(data.audit.findings.length, "audit finding")} and want this skill in my library anyway.`
              : null
          }
          render={(data) => (
            <div className="flex flex-col gap-2 text-meta">
              {data.skills.map((s) => (
                <p key={s.name}>
                  Adds {typeof s.files === "number" ? plural(s.files, "file") : "files"}{" "}
                  to <Mono>{s.path}</Mono>
                </p>
              ))}
              <p>
                <span className="font-medium">Audit: </span>
                {data.audit.findings.length === 0
                  ? "no risky instructions"
                  : plural(data.audit.findings.length, "finding")}
              </p>
              {data.audit.findings.map((f, index) => (
                <p key={index} className="rounded-md border px-2 py-1">
                  <span className="font-medium capitalize">{f.severity}</span>
                  {f.line ? ` · line ${f.line}` : ""}: {f.message}
                </p>
              ))}
              <p className="text-muted-foreground">
                Then it appears in the Sync plan. Undo: delete it from the library.
              </p>
            </div>
          )}
          confirm={() => installSkill(`@${pick.repo}/${pick.name}`, false)}
          onClose={() => setPick(null)}
          onDone={onInstalled}
        />
      )}
    </section>
  );
}

const STATE_BADGE: Record<string, { label: string; variant: "warning" | "destructive" }> =
  {
    behind: { label: "behind its remote", variant: "warning" },
    duplicate: { label: "duplicate clone", variant: "warning" },
    failed: { label: "update failed", variant: "destructive" },
  };

function SourceList({ sources }: { sources: Loaded<SourcesData> }) {
  const roots = useLoad(listRoots, "roots");
  const [filter, setFilter] = useState({ query: "", kind: "all", state: "any" });
  const [dialog, setDialog] = useState<
    { kind: "add" } | { kind: "rm"; path: string } | null
  >(null);
  const rows = sources.data?.sources ?? [];
  const kinds = [...new Set(rows.map((r) => r.origin.kind))];
  const shown = rows.filter(
    (row) =>
      (filter.kind === "all" || row.origin.kind === filter.kind) &&
      (filter.state === "any" ||
        (filter.state === "attention"
          ? row.status.state !== "ok"
          : row.status.state === filter.state)) &&
      (!filter.query ||
        `${row.origin.name} ${row.status.detail}`
          .toLowerCase()
          .includes(filter.query.toLowerCase())),
  );
  const reload = () => {
    sources.reload();
    roots.reload();
  };
  return (
    <section aria-label="Sources" className="flex flex-col gap-2">
      <Heading
        title="Sources"
        count={rows.length}
        note={sources.data ? `Last scan ${when(sources.data.generatedAt)}` : undefined}
      >
        <Button variant="ghost" size="sm" onClick={reload}>
          <RefreshCw /> Rescan
        </Button>
        <Button variant="outline" size="sm" onClick={() => setDialog({ kind: "add" })}>
          <FolderPlus /> Add folder to scan…
        </Button>
      </Heading>
      <div className="flex flex-wrap gap-2">
        <div className="relative min-w-40 flex-1">
          <Search
            aria-hidden
            className="pointer-events-none absolute top-2 left-2.5 size-4 text-muted-foreground"
          />
          <Input
            aria-label="Search sources"
            placeholder="Search sources"
            className="pl-8"
            value={filter.query}
            onChange={(event) => setFilter({ ...filter, query: event.target.value })}
          />
        </div>
        <select
          aria-label="Source kind"
          className={SELECT}
          value={filter.kind}
          onChange={(event) => setFilter({ ...filter, kind: event.target.value })}
        >
          <option value="all">All kinds ({kinds.length})</option>
          {kinds.map((kind) => (
            <option key={kind} value={kind}>
              {kind}
            </option>
          ))}
        </select>
        <select
          aria-label="Source state"
          className={SELECT}
          value={filter.state}
          onChange={(event) => setFilter({ ...filter, state: event.target.value })}
        >
          <option value="any">Any state</option>
          <option value="attention">Needs attention</option>
        </select>
      </div>
      {sources.error && !sources.data && (
        <p role="alert" className="text-meta text-destructive">
          {sources.error}
        </p>
      )}
      {!sources.data && !sources.error && <SkeletonRows label="Reading the sources" />}
      {sources.data && (
        <ul className="divide-y overflow-hidden rounded-xl border bg-card">
          {shown.map((row: SourceRow) => {
            const badge = STATE_BADGE[row.status.state];
            return (
              <li key={row.id} className="flex flex-wrap items-center gap-3 px-4 py-3">
                <div className="min-w-48 flex-1">
                  <p className="text-sm font-medium">{row.origin.name}</p>
                  <p className="text-meta text-muted-foreground">{row.status.detail}</p>
                </div>
                {badge && <Badge variant={badge.variant}>{badge.label}</Badge>}
                <Badge variant="outline">{row.origin.kind}</Badge>
                <Badge variant={row.writable ? "success" : "off"}>
                  {row.writable ? "yours" : "read-only"}
                </Badge>
              </li>
            );
          })}
        </ul>
      )}
      <p className="text-meta text-muted-foreground">
        Only your own library is ever written. The rest is read to answer “where does this
        skill come from”. A plugin’s skills are not part of your library and are never
        synced.
      </p>
      {roots.data && roots.data.roots.some((r) => r.origin === "config") && (
        <ul
          className="flex flex-col gap-1 text-meta"
          aria-label="Folders you added to the scan"
        >
          {roots.data.roots
            .filter((r) => r.origin === "config")
            .map((root) => (
              <li key={root.path} className="flex items-center gap-2">
                <Mono>{root.path}</Mono>
                <Button
                  variant="ghost"
                  size="icon-xs"
                  aria-label={`Stop scanning ${root.path}`}
                  onClick={() => setDialog({ kind: "rm", path: root.path })}
                >
                  <X />
                </Button>
              </li>
            ))}
        </ul>
      )}
      {dialog?.kind === "add" && (
        <AskDialog
          title="Add folder to scan"
          description="Toolport looks for checkouts and skills folders under it. Nothing is written."
          label="Folder"
          placeholder="/Users/you/work"
          onClose={() => setDialog(null)}
          next={(path) => (
            <WriteDialog
              title="Add folder to scan"
              confirmLabel="Add folder"
              preview={() => addRoot(path, true)}
              render={() => (
                <p className="text-meta">
                  Adds <Mono>{path}</Mono> to the folders scanned for sources.
                </p>
              )}
              confirm={() => addRoot(path, false)}
              onClose={() => setDialog(null)}
              onDone={reload}
            />
          )}
        />
      )}
      {dialog?.kind === "rm" && (
        <WriteDialog
          title="Stop scanning this folder"
          confirmLabel="Stop scanning"
          preview={() => removeRoot(dialog.path, true)}
          render={() => <FileLines files={[dialog.path]} />}
          confirm={() => removeRoot(dialog.path, false)}
          onClose={() => setDialog(null)}
          onDone={reload}
        />
      )}
    </section>
  );
}

export default function SourcesTab() {
  const sources = useLoad(listSources, "sources");
  const library = useLoad(async () => {
    try {
      return await listLibrary();
    } catch {
      return null;
    }
  }, "library");
  const installed = new Set((library.data?.skills ?? []).map((s) => s.name));
  return (
    <div className="flex flex-col gap-6">
      <RemoteCard />
      <Taps sources={sources.data?.sources ?? []} onUpdated={sources.reload} />
      <Find installed={installed} onInstalled={library.reload} />
      <SourceList sources={sources} />
    </div>
  );
}
