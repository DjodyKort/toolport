import { Callout } from "@/components/Callout";
import { Button } from "@/components/ui/button";
import { ScreenSkeleton } from "../ui";
import type { ClientReachData } from "../types/client";
import { Code, Pill } from "./atoms";
import type { ServerView } from "./model";
import { useServers } from "./useServers";

type Route = ClientReachData["servers"][number]["routes"][number];
type Finding = ClientReachData["findings"][number];

const LAUNCHER_STATE: Record<string, string> = {
  stale: "the launcher file moved",
  customized: "edited since Toolport wrote it",
  missing: "the entry is gone from the client",
  unrecorded: "Toolport has no record of writing it",
};

const LIVE_WORDS: Record<string, string> = {
  connected: "connects",
  "needs-auth": "needs a sign-in",
  failed: "fails to connect",
};

function heldWords(held: NonNullable<Route["held"]>): string {
  const switchedOn = held.enabled
    ? "switched on"
    : `switched off in ${held.disabledIn.length} project${held.disabledIn.length === 1 ? "" : "s"}`;
  const live = held.live ? LIVE_WORDS[held.live] : null;
  return live ? `${switchedOn}, ${live}` : switchedOn;
}

function RouteRow({ route, profile }: { route: Route; profile: string | null }) {
  const flagged =
    route.state && route.state !== "ok" ? LAUNCHER_STATE[route.state] : null;
  return (
    <li className="flex flex-col gap-0.5 text-sm">
      <span className="flex flex-wrap items-center gap-2">
        <span className="font-medium">{route.clientName}</span>
        {route.how === "gateway" && (
          <>
            <Pill>through Toolport</Pill>
            {profile && (
              <span className="text-xs text-muted-foreground">profile {profile}</span>
            )}
          </>
        )}
        {route.how === "launcher" && <Pill>Toolport launcher entry</Pill>}
        {route.how === "own-entry" && <Pill>its own entry, not through Toolport</Pill>}
      </span>
      {route.how !== "gateway" && route.entry && (
        <span className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
          <span>entry</span>
          <Code>{route.entry}</Code>
          {route.transport && <span>{route.transport}</span>}
          {route.target && <Code>{route.target}</Code>}
        </span>
      )}
      {route.held && (
        <span className="text-xs text-muted-foreground">
          In {route.clientName}: {heldWords(route.held)}.
          {!route.held.enabled && route.held.disabledIn.length <= 3 && (
            <> Off for {route.held.disabledIn.join(", ")}.</>
          )}
        </span>
      )}
      {route.how === "own-entry" && route.basis && route.basis !== "record" && (
        <span className="text-xs text-muted-foreground">
          Matched to this server by its {route.basis}.
        </span>
      )}
      {flagged && <span className="text-xs text-muted-foreground">{flagged}.</span>}
      {route.note && <span className="text-xs text-muted-foreground">{route.note}.</span>}
    </li>
  );
}

function FindingNote({ finding }: { finding: Finding }) {
  return (
    <Callout variant={finding.level === "look" ? "warning" : "info"}>
      {finding.text}
    </Callout>
  );
}

/** Which clients reach one server and by what route: through the gateway, through a Toolport
 * launcher entry, or through an entry the client holds itself (which Toolport does not manage). */
export function ServerReach({ view }: { view: ServerView }) {
  const { reach, liveCheck } = useServers();
  const data = reach.data;
  const entry = data?.servers.find((server) => server.id === view.id);
  const findings = (data?.findings ?? []).filter((finding) => finding.server === view.id);
  const profileOf = (client: string) =>
    data?.clients.find((candidate) => candidate.id === client)?.profile ?? null;

  return (
    <section aria-label="How clients reach it" className="flex flex-col gap-2">
      <h3 className="text-2xs font-semibold tracking-[0.09em] text-muted-foreground uppercase">
        How clients reach it
      </h3>
      {!data && reach.status !== "error" && (
        <ScreenSkeleton rows={1} label="Checking the clients" />
      )}
      {!data && reach.status === "error" && (
        <p className="text-sm text-muted-foreground">
          Couldn&apos;t check how the clients reach this server.{" "}
          <button type="button" className="underline" onClick={reach.reload}>
            Try again
          </button>
        </p>
      )}
      {data && entry && entry.routes.length === 0 && (
        <p className="text-sm text-muted-foreground">No client reaches this server.</p>
      )}
      {entry && entry.routes.length > 0 && (
        <ul aria-label="Routes" className="flex flex-col gap-2">
          {entry.routes.map((route) => (
            <RouteRow
              key={`${route.client}:${route.how}:${route.entry ?? ""}`}
              route={route}
              profile={profileOf(route.client)}
            />
          ))}
        </ul>
      )}
      {entry?.routes.some((route) => route.held) && (
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <Button
            size="sm"
            variant="outline"
            disabled={liveCheck.running}
            onClick={liveCheck.run}
          >
            {liveCheck.running ? "Asking Claude Code…" : "Ask Claude Code again"}
          </Button>
          <span>Starts the servers it holds to see whether they connect.</span>
        </div>
      )}
      {findings.map((finding) => (
        <FindingNote key={`${finding.kind}:${finding.client}`} finding={finding} />
      ))}
    </section>
  );
}
