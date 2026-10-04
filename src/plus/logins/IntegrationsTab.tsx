import { type ReactNode } from "react";
import { Bell, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { AsyncView, CopyButton, errorText, useCtlQuery, type CtlQuery } from "../ui";
import type { AuthStatuslineData } from "../bridge/data";
import { Offline } from "./atoms";
import type { TabProps } from "./LoginsTab";
import { HOOK_SNIPPET, STATUSLINE_SNIPPET, type Summary } from "./model";
import { Strip } from "./Strip";

type StatuslineAuth = AuthStatuslineData["auth"];

interface HookData extends AuthStatuslineData {
  hookSpecificOutput?: { hookEventName: string; additionalContext: string };
}

function summaryOf(auth: StatuslineAuth): Summary {
  const needSignIn = auth.needs_reauth + auth.revoked + auth.expiring;
  const other = auth.misconfigured + auth.unreachable;
  return {
    total: auth.ok + needSignIn + other,
    signedIn: auth.ok,
    needSignIn,
    needNames: auth.worst,
    other,
    lastProbe: null,
  };
}

function Counts({ auth }: { auth: StatuslineAuth }) {
  const entries: Array<[string, number]> = [
    ["ok", auth.ok],
    ["needs_reauth", auth.needs_reauth],
    ["revoked", auth.revoked],
    ["expiring", auth.expiring],
    ["misconfigured", auth.misconfigured],
    ["unreachable", auth.unreachable],
  ];
  return (
    <ul aria-label="Counts" className="flex flex-wrap gap-1.5">
      {entries
        .filter(([name, count]) => count > 0 || name === "ok")
        .map(([name, count]) => (
          <li
            key={name}
            className="rounded-md border bg-background px-2 py-0.5 font-mono text-xs"
          >
            {name}: {count}
          </li>
        ))}
    </ul>
  );
}

function Live<T>({
  query,
  title,
  command,
  onOpenCommands,
  children,
}: {
  query: CtlQuery<T>;
  title: string;
  command: string;
  onOpenCommands?: (group?: string) => void;
  children: (data: T) => ReactNode;
}) {
  if (
    query.data === null &&
    query.status === "error" &&
    errorText(query.error).code === "bridge"
  ) {
    return (
      <Offline
        error={query.error}
        onRetry={query.reload}
        onOpenCommands={onOpenCommands}
      />
    );
  }
  return (
    <AsyncView query={query} errorTitle={title} context={command}>
      {children}
    </AsyncView>
  );
}

function Snippet({ label, text }: { label: string; text: string }) {
  return (
    <div className="flex flex-col gap-1.5">
      <p className="text-xs font-medium text-muted-foreground">{label}</p>
      <pre
        role="region"
        aria-label={label}
        className="overflow-auto rounded-md bg-muted p-2 font-mono text-xs whitespace-pre-wrap"
      >
        {text}
      </pre>
      <div>
        <CopyButton text={text} label="Copy snippet" />
      </div>
    </div>
  );
}

function Output({ label, value }: { label: string; value: unknown }) {
  const text = JSON.stringify(value, null, 2);
  return (
    <div className="flex flex-col gap-1.5">
      <p className="text-xs font-medium text-muted-foreground">{label}</p>
      <pre
        role="region"
        aria-label={label}
        className="max-h-56 overflow-auto rounded-md bg-muted p-2 font-mono text-xs whitespace-pre-wrap"
      >
        {text}
      </pre>
      <div>
        <CopyButton text={text} label="Copy output" />
      </div>
    </div>
  );
}

function Card({
  title,
  intro,
  children,
}: {
  title: string;
  intro: string;
  children: ReactNode;
}) {
  return (
    <section
      aria-label={title}
      className="flex flex-col gap-3 rounded-xl border bg-card p-4"
    >
      <h2 className="text-sm font-semibold">{title}</h2>
      <p className="text-sm text-muted-foreground">{intro}</p>
      {children}
    </section>
  );
}

/** Integrations: the output of `auth statusline` and `auth hook` as the shells see it right
 * now, and the snippet that installs each one in Claude Code. Both read the login cache; they
 * change after a probe or a sign-in. */
export function IntegrationsTab({ onOpenCommands }: TabProps) {
  const line = useCtlQuery<AuthStatuslineData>(["auth", "statusline"]);
  const hook = useCtlQuery<HookData>(["auth", "hook"]);
  const reload = () => {
    line.reload();
    hook.reload();
  };
  return (
    <div className="flex flex-col gap-4">
      {line.data && <Strip summary={summaryOf(line.data.auth)} lastProbe={false} />}
      <div className="flex justify-end">
        <Button variant="ghost" size="sm" onClick={reload}>
          <RefreshCw /> Refresh
        </Button>
      </div>
      <div className="grid gap-4 lg:grid-cols-2">
        <Card
          title="Statusline"
          intro="Shows login health in the Claude Code status line. Output right now:"
        >
          <Live
            query={line}
            title="Couldn't read the statusline output"
            command="auth statusline"
            onOpenCommands={onOpenCommands}
          >
            {(data) => (
              <div className="flex flex-col gap-3">
                <p className="text-sm font-medium">{data.auth.text}</p>
                <Counts auth={data.auth} />
                <Output label="Statusline output" value={data} />
              </div>
            )}
          </Live>
          <Snippet label="Statusline snippet" text={STATUSLINE_SNIPPET} />
        </Card>
        <Card
          title="Session start hook"
          intro="Lets Claude start a session knowing which logins are broken. Output right now:"
        >
          <Live
            query={hook}
            title="Couldn't read the hook output"
            command="auth hook"
            onOpenCommands={onOpenCommands}
          >
            {(data) => (
              <div className="flex flex-col gap-3">
                <Counts auth={data.auth} />
                {data.hookSpecificOutput ? (
                  <div className="flex flex-col gap-1">
                    <p className="text-xs font-medium text-muted-foreground">
                      What Claude is told at the start of a session
                    </p>
                    <p className="rounded-md border bg-background p-2 text-sm whitespace-pre-wrap">
                      {data.hookSpecificOutput.additionalContext}
                    </p>
                  </div>
                ) : (
                  <p className="text-sm text-muted-foreground">
                    Every login works, so the hook adds nothing to a session.
                  </p>
                )}
                <Output label="Hook output" value={data} />
              </div>
            )}
          </Live>
          <Snippet label="Session start hook snippet" text={HOOK_SNIPPET} />
        </Card>
      </div>
      <section
        aria-label="Notifications"
        className="flex items-start gap-3 rounded-xl border bg-card p-4"
      >
        <Bell
          className="mt-0.5 size-4 shrink-0 text-muted-foreground"
          aria-hidden="true"
        />
        <div className="flex flex-col gap-1 text-sm">
          <h2 className="font-semibold">Notifications</h2>
          <p className="text-muted-foreground">
            While this window is visible, Toolport checks once a minute and raises a
            notification when a login needs a new sign-in or is about to expire. Each
            login is announced once every six hours; Review opens the Logins tab.
          </p>
        </div>
      </section>
    </div>
  );
}
