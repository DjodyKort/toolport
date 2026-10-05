import { useMemo, useState, type ReactNode } from "react";
import { AlertTriangle, RefreshCw, Search } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import type { ProfileLsData } from "../bridge/data";
import type { InspectData } from "../types/inspect";
import { ErrorState, ScreenSkeleton, outcomeOf, useCtlJob, useCtlQuery } from "../ui";
import { Chip, Code, Kv, Pill, StateBadge } from "./atoms";
import { launchLine, looksLikeLogin, type ServerInfo, type ServerView } from "./model";
import { ServerTools } from "./ServerTools";
import { useServers } from "./useServers";
import type { WriteControl } from "./useWrite";

const SHOWN_TOOLS = 60;

function LiveTools({ view }: { view: ServerView }) {
  const { go } = useServers();
  const job = useCtlJob();
  const [filter, setFilter] = useState("");
  const outcome = outcomeOf(job.state);
  const running = job.state.phase === "running";
  const tools = useMemo(() => {
    if (outcome?.kind !== "ok") return null;
    const data = outcome.data as InspectData;
    return (
      (data.servers.find((entry) => entry.id === view.id) ?? data.servers[0])?.tools ?? []
    );
  }, [outcome, view.id]);
  const shown = (tools ?? []).filter((tool) =>
    `${tool.name} ${tool.description}`
      .toLowerCase()
      .includes(filter.trim().toLowerCase()),
  );
  const wantsLogin =
    outcome?.kind === "error" &&
    (view.state === "login" || looksLikeLogin(outcome.message));

  return (
    <section aria-label="Tools" className="flex flex-col gap-2">
      <div className="flex flex-wrap items-center gap-2">
        <h3 className="text-2xs font-semibold tracking-[0.09em] text-muted-foreground uppercase">
          Tools
        </h3>
        {view.tools !== null && (
          <span className="rounded-full bg-secondary px-2 py-px text-2xs font-semibold text-muted-foreground tabular-nums">
            {view.tools}
          </span>
        )}
        <span className="ml-auto flex gap-2">
          {running ? (
            <Button size="sm" variant="outline" onClick={() => void job.cancel()}>
              Cancel
            </Button>
          ) : (
            <Button
              size="sm"
              variant="outline"
              onClick={() => void job.start(["inspect", view.id])}
            >
              <RefreshCw /> Inspect live
            </Button>
          )}
        </span>
      </div>
      {running && <ScreenSkeleton rows={2} label="Connecting to the server" />}
      {outcome?.kind === "error" && (
        <Callout variant={wantsLogin ? "warning" : "danger"} role="alert">
          <p className="flex items-start gap-2">
            <AlertTriangle className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
            <span>
              {wantsLogin
                ? "The server asked for a sign-in, so no tools could be listed. "
                : "The server did not list its tools. "}
              <span className="break-words">{outcome.message}</span>
            </span>
          </p>
          {wantsLogin && (
            <div className="mt-2">
              <Button size="sm" variant="outline" onClick={() => go("logins")}>
                Open Logins
              </Button>
            </div>
          )}
        </Callout>
      )}
      {outcome?.kind === "cancelled" && (
        <p className="text-sm text-muted-foreground">
          Cancelled. Nothing more was started.
        </p>
      )}
      {tools && tools.length === 0 && (
        <p className="text-sm text-muted-foreground">The server lists no tools.</p>
      )}
      {tools && tools.length > 0 && (
        <>
          <div className="relative">
            <Search
              className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground"
              aria-hidden="true"
            />
            <Input
              type="search"
              aria-label="Filter tools"
              placeholder="Filter tools"
              value={filter}
              onChange={(event) => setFilter(event.target.value)}
              className="pl-8"
            />
          </div>
          <ul aria-label="Tool list" className="flex flex-col divide-y rounded-lg border">
            {shown.slice(0, SHOWN_TOOLS).map((tool) => (
              <li key={tool.name} className="flex flex-col gap-0.5 px-3 py-1.5">
                <code className="font-mono text-xs">{tool.name}</code>
                {tool.description && (
                  <span className="line-clamp-2 text-xs text-muted-foreground">
                    {tool.description}
                  </span>
                )}
              </li>
            ))}
            {shown.length === 0 && (
              <li className="px-3 py-2 text-sm text-muted-foreground">
                No tool matches {filter}.
              </li>
            )}
          </ul>
          {shown.length > SHOWN_TOOLS && (
            <p className="text-sm text-muted-foreground">
              and {shown.length - SHOWN_TOOLS} more
            </p>
          )}
        </>
      )}
    </section>
  );
}

export function ServerDetail({
  view,
  profiles,
  onEdit,
  onRemove,
  onToggleProfile,
  busy,
  write,
  onChanged,
}: {
  view: ServerView;
  profiles: ProfileLsData;
  onEdit: (info: ServerInfo) => void;
  onRemove: (info: ServerInfo | null) => void;
  onToggleProfile: (profileId: string, on: boolean) => void;
  busy: boolean;
  write: WriteControl;
  onChanged: () => void;
}) {
  const { go } = useServers();
  const info = useCtlQuery<ServerInfo>(["server", "info", view.id]);
  const data = info.data;
  const member = new Set(view.profiles.map((profile) => profile.id));

  return (
    <section
      aria-label={`${view.name} details`}
      className="flex min-w-0 flex-col gap-4 rounded-xl border bg-card p-4"
    >
      <div className="flex flex-wrap items-center gap-2">
        <h2 className="text-base font-semibold">{view.name}</h2>
        <StateBadge view={view} />
        <Pill>{view.transport}</Pill>
      </div>
      {view.state === "login" && (
        <Callout variant="warning" className="flex flex-col gap-2">
          <p>
            <b>Sign-in needed.</b> {view.reason}. The gateway keeps running without it;
            this server shows no tools until you sign in.
          </p>
          <div>
            <Button size="sm" onClick={() => go("logins")}>
              Open Logins
            </Button>
          </div>
        </Callout>
      )}
      {view.state === "failed" && (
        <Callout variant="danger" className="flex flex-col gap-2" role="status">
          <p>
            <b>Could not start.</b> {view.reason}.
          </p>
          <div>
            <Button size="sm" variant="outline" onClick={() => go("health")}>
              Open Health
            </Button>
          </div>
        </Callout>
      )}
      {view.state === "disabled" && (
        <Callout variant="info">
          Not in the active profile, so no client that follows it sees this server.
        </Callout>
      )}
      {info.status === "error" && !data && (
        <ErrorState
          error={info.error}
          title="Couldn't load this server"
          context={`server info ${view.id}`}
          onRetry={info.reload}
        />
      )}
      {!data && info.status !== "error" && (
        <ScreenSkeleton rows={3} label="Loading server" />
      )}
      {data && (
        <Kv
          rows={
            [
              ["Launch", <Code key="launch">{launchLine(data)}</Code>],
              [
                "Environment",
                data.env.length === 0 ? (
                  <span className="text-muted-foreground">No environment keys</span>
                ) : (
                  <ul className="flex flex-col gap-1">
                    {data.env.map((entry) => (
                      <li key={entry.key} className="flex flex-wrap items-center gap-2">
                        <Code>{entry.key}</Code>
                        <span className="text-xs text-muted-foreground">
                          {entry.secret ? "secret, stored in the vault" : "plain value"}
                        </span>
                      </li>
                    ))}
                    <li className="text-xs text-muted-foreground">
                      Key names only, values stay in the vault.
                    </li>
                  </ul>
                ),
              ],
              ...(data.cwd
                ? [["Working folder", <Code key="cwd">{data.cwd}</Code>]]
                : []),
              ...(data.source ? [["Source", String(data.source)]] : []),
              ...(data.disabledTools.length > 0
                ? [
                    [
                      "Tools turned off",
                      <span key="off" className="flex flex-wrap gap-1">
                        {data.disabledTools.map((tool) => (
                          <Chip key={tool}>{tool}</Chip>
                        ))}
                      </span>,
                    ],
                  ]
                : []),
            ] as Array<[string, ReactNode]>
          }
        />
      )}
      <div className="flex flex-col gap-2">
        <h3 className="text-2xs font-semibold tracking-[0.09em] text-muted-foreground uppercase">
          In profiles
        </h3>
        <ul aria-label="Profiles" className="flex flex-col gap-1.5">
          {profiles.profiles.map((profile) => {
            const on = member.has(profile.id);
            return (
              <li key={profile.id} className="flex items-center gap-2 text-sm">
                <Switch
                  size="sm"
                  checked={on}
                  disabled={busy}
                  aria-label={`${view.name} in profile ${profile.name}`}
                  onCheckedChange={(next) => onToggleProfile(profile.id, next)}
                />
                <span>{profile.name}</span>
                {profile.active && <Pill>active</Pill>}
              </li>
            );
          })}
        </ul>
      </div>
      <ServerTools
        key={`tools:${view.id}`}
        view={view}
        profiles={profiles}
        write={write}
        onChanged={onChanged}
      />
      <LiveTools key={view.id} view={view} />
      <div className="flex flex-wrap gap-2 border-t pt-3">
        <Button
          variant="outline"
          disabled={!data || busy}
          onClick={() => data && onEdit(data)}
        >
          Edit
        </Button>
        <Button variant="destructive" disabled={busy} onClick={() => onRemove(data)}>
          Remove…
        </Button>
      </div>
    </section>
  );
}
