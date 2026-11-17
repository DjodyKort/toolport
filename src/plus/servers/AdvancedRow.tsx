import { useId, useState, type ReactNode } from "react";
import { SlidersHorizontal } from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { toastError } from "@/lib/toast";
import { cn } from "@/lib/utils";
import type { ProbeResult, ServerEntry } from "@/lib/types";
import { errorText } from "../settings/useCtl";
import {
  bridgeWith,
  changedCount,
  editable,
  minutesText,
  parseLimit,
  span,
  summaryLine,
  toolsLine,
  valuesOf,
  type Values,
} from "./advancedModel";
import {
  setCallLimit,
  setInteropSwitch,
  setToolListing,
  type InteropSwitch,
} from "./commands";
import type { ServerInfoData } from "./data";
import { OperationRow } from "./OperationRow";
import { restart, useRestartState } from "./restartStore";
import type { Loaded } from "./useLoad";

const QUESTIONS = "Let the server ask me questions";
const INSTRUCTIONS = "Pass the server's instructions to the model";

const SWITCH_LABEL: Record<InteropSwitch, string> = {
  declareClientCapabilities: QUESTIONS,
  forwardInstructions: INSTRUCTIONS,
};

function Key({ children }: { children: string }) {
  return (
    <code className="font-mono text-xs font-normal text-muted-foreground">
      {children}
    </code>
  );
}

function Help({
  fallback,
  change,
  takes,
}: {
  fallback: string;
  change: ReactNode;
  takes: ReactNode;
}) {
  return (
    <details className="group text-meta">
      <summary className="w-fit cursor-pointer rounded-md text-muted-foreground select-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring focus-visible:outline-none">
        When to change it
      </summary>
      <dl className="mt-1.5 grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1">
        <dt className="text-muted-foreground">Default</dt>
        <dd>{fallback}</dd>
        <dt className="text-muted-foreground">Change it when</dt>
        <dd>{change}</dd>
        <dt className="text-muted-foreground">Takes effect</dt>
        <dd>{takes}</dd>
      </dl>
    </details>
  );
}

function Setting({
  title,
  registryKey,
  titleId,
  control,
  children,
}: {
  title: string;
  registryKey: string;
  titleId: string;
  control?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section
      aria-labelledby={titleId}
      className="flex flex-col gap-1.5 border-t border-border pt-3 first:border-t-0 first:pt-0"
    >
      <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1.5">
        <h4
          id={titleId}
          className="flex flex-wrap items-baseline gap-x-2 text-sm font-semibold"
        >
          {title} <Key>{registryKey}</Key>
        </h4>
        {control}
      </div>
      {children}
    </section>
  );
}

function ToolListing({
  value,
  tools,
  busy,
  onChange,
}: {
  value: Values["mode"];
  tools: number | null;
  busy: boolean;
  onChange: (mode: Values["mode"]) => void;
}) {
  const id = useId();
  const options: Array<[Values["mode"], string]> = [
    ["auto", "Auto"],
    ["search", "Search only"],
  ];
  return (
    <Setting
      title="Tool listing"
      registryKey="mode"
      titleId={id}
      control={
        <div
          role="radiogroup"
          aria-labelledby={id}
          className="inline-flex rounded-md border border-border p-0.5"
        >
          {options.map(([mode, label]) => (
            <button
              key={mode}
              type="button"
              role="radio"
              aria-checked={value === mode}
              disabled={busy}
              onClick={() => value !== mode && onChange(mode)}
              className={cn(
                "h-7 rounded-sm px-2.5 text-sm font-medium focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none disabled:opacity-60",
                value === mode
                  ? "bg-primary text-primary-foreground"
                  : "text-muted-foreground hover:bg-muted",
              )}
            >
              {label}
            </button>
          ))}
        </div>
      }
    >
      <p className="text-meta text-muted-foreground" aria-live="polite">
        {toolsLine(value, tools)}
      </p>
      <Help
        fallback="Auto"
        change="One server has hundreds of tools that crowd the model's context before you ask anything. Search only keeps them out of the way until they are needed."
        takes="With the next tool list a client asks for. No restart. A client that is already running may need to reconnect to see the change."
      />
    </Setting>
  );
}

function SwitchSetting({
  name,
  on,
  busy,
  onChange,
  help,
  children,
}: {
  name: InteropSwitch;
  on: boolean;
  busy: boolean;
  onChange: (on: boolean) => void;
  help: ReactNode;
  children?: ReactNode;
}) {
  const id = useId();
  return (
    <Setting
      title={SWITCH_LABEL[name]}
      registryKey={name}
      titleId={id}
      control={
        <Switch
          aria-labelledby={id}
          checked={on}
          disabled={busy}
          onCheckedChange={onChange}
        />
      }
    >
      {help}
      {children}
    </Setting>
  );
}

function CallLimit({
  server,
  info,
  onSaved,
}: {
  server: ServerEntry;
  info: ServerInfoData;
  onSaved: () => void;
}) {
  const id = useId();
  const stored = info.maxRequestTimeoutMs;
  const [draft, setDraft] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [saving, setSaving] = useState(false);
  const { interop } = info;
  const window = info.requestTimeoutMs ?? interop.defaultRequestTimeoutMs;

  const text = draft ?? (stored === null ? "" : minutesText(stored));
  const parsed = parseLimit(text);
  const next = parsed.kind === "ok" ? parsed.ms : null;
  const valid = parsed.kind !== "error";
  const differs = valid && next !== stored;
  const label = `${server.name}`;

  async function save(limit: number | null) {
    setSaving(true);
    try {
      await setCallLimit(info.id, limit);
      setDraft(null);
      setSaved(true);
      onSaved();
    } catch (error) {
      toastError(`Could not save the call limit of ${label}`, {
        description: errorText(error),
      });
    } finally {
      setSaving(false);
    }
  }

  const now = bridgeWith(info, stored, server.name);
  const after = bridgeWith(info, next, server.name);
  const origin = (bridge: typeof now) =>
    bridge.setBy === null
      ? "the bridge's own minimum"
      : bridge.setBy === server.name
        ? `${server.name} (this server)`
        : bridge.setBy;

  let message: ReactNode = null;
  let tone: "error" | "note" | "ok" = "note";
  if (parsed.kind === "error") {
    message = parsed.message;
    tone = "error";
  } else if (parsed.kind === "ok" && parsed.ms < window) {
    message = `Calls are not cut before the request timeout of ${span(window)}, so ${span(parsed.ms)} has no effect. Raise it, or lower the request timeout in Edit.`;
  } else if (saved && !differs) {
    message = "Saved. Applies to the next call.";
    tone = "ok";
  } else if (parsed.kind === "default") {
    message = `Empty means the default, ${span(interop.defaultLimitMs)}.`;
  }

  return (
    <Setting
      title="Longest a call may run"
      registryKey="maxRequestTimeoutMs"
      titleId={id}
    >
      <p className="text-meta text-muted-foreground">
        Normally a call to a local server is cut after its request timeout ({span(window)}{" "}
        without a sign of life, set in Edit). If the server keeps reporting progress,
        Toolport keeps waiting, but never longer than this.
      </p>
      {editable(stored) ? (
        <>
          <div className="flex flex-wrap items-center gap-2">
            <label htmlFor={`${id}-limit`} className="text-meta text-muted-foreground">
              Limit
            </label>
            <Input
              id={`${id}-limit`}
              inputMode="decimal"
              className="w-24"
              value={text}
              placeholder={minutesText(interop.defaultLimitMs)}
              aria-invalid={parsed.kind === "error"}
              aria-describedby={`${id}-message`}
              onChange={(event) => {
                setDraft(event.target.value);
                setSaved(false);
              }}
            />
            <span className="text-meta text-muted-foreground">
              minutes
              {parsed.kind === "ok" && ` = ${span(parsed.ms)}`}
            </span>
            <Button
              size="sm"
              disabled={!differs || saving}
              onClick={() => void save(next)}
            >
              Save
            </Button>
            <Button
              variant="ghost"
              size="sm"
              disabled={stored === null || saving}
              onClick={() => void save(null)}
            >
              Use default ({minutesText(interop.defaultLimitMs)})
            </Button>
          </div>
          <p
            id={`${id}-message`}
            role={tone === "error" ? "alert" : undefined}
            className={cn(
              "text-meta",
              tone === "error"
                ? "text-destructive"
                : tone === "ok"
                  ? "text-success"
                  : "text-muted-foreground",
            )}
          >
            {message}
          </p>
        </>
      ) : (
        <p className="text-meta">
          <span className="font-medium">{span(stored ?? 0)}</span>
          <span className="text-muted-foreground">
            {" "}
            · set from the command line, below the 1 minute this field takes. Change it
            with{" "}
          </span>
          <code className="font-mono text-xs break-all">
            toolportctl server edit {info.name} --max-request-timeout-ms
            &lt;ms&gt;|default
          </code>
        </p>
      )}
      <p className="text-meta text-muted-foreground" data-testid="bridge-line">
        The bridge follows. Clients that connect through Toolport&apos;s command-line
        bridge (<code className="font-mono text-xs">toolport-stdio</code>) used to give up
        after {span(interop.bridgeBaseMs)}. They now wait for the longest limit among your
        enabled local servers, plus {span(interop.bridgeGraceMs)}.{" "}
        {differs && info.enabled ? (
          after.ceilingMs === now.ceilingMs && after.setBy === now.setBy ? (
            <>
              Stays <strong>{span(after.ceilingMs)}</strong>, set by {origin(after)}.
            </>
          ) : (
            <>
              After saving: <strong>{span(after.ceilingMs)}</strong>, set by{" "}
              {origin(after)}. It was {span(now.ceilingMs)}.
            </>
          )
        ) : (
          <>
            Right now: <strong>{span(now.ceilingMs)}</strong>
            {now.setBy === server.name && stored === null
              ? `, from the default limit of ${span(interop.defaultLimitMs)}.`
              : `, set by ${origin(now)}.`}
          </>
        )}
      </p>
      <details className="text-meta">
        <summary className="w-fit cursor-pointer rounded-md text-muted-foreground select-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring focus-visible:outline-none">
          How long calls work
        </summary>
        <ul className="mt-1.5 flex flex-col gap-1">
          <li>
            <span className="font-medium">No progress reported:</span> cut after{" "}
            {span(window)}.
          </li>
          <li>
            <span className="font-medium">Progress every few seconds:</span> each report
            starts a new {span(window)} wait.
          </li>
          <li>
            <span className="font-medium">Progress, reaches the limit:</span> stopped at
            the limit you set.
          </li>
        </ul>
        <p className="mt-1.5 text-muted-foreground">
          Waiting for you to answer a question the server asked does not count against
          either limit. A call that reports no progress, or whose client sent no progress
          token, keeps the plain request timeout.
        </p>
        <p className="mt-1.5 text-muted-foreground">
          What the client shows when the limit stops a call:
        </p>
        <code className="mt-1 block rounded-md bg-muted px-2 py-1.5 font-mono text-xs break-words">
          timed out waiting for &apos;tools/call&apos; response: the call reached its
          absolute cap (maxRequestTimeoutMs) while the server was still reporting progress
        </code>
      </details>
    </Setting>
  );
}

export interface AdvancedRowProps {
  server: ServerEntry;
  enabled: boolean;
  health?: ProbeResult;
  info: Loaded<ServerInfoData>;
}

/** The fork's per-server settings that no connection dialog carries: how the tools are listed,
 * the two interop switches and the longest a call may run. The mode and the switches apply at
 * once with Undo; the call limit has Save. A change that needs a restart says so and offers the
 * Runtime row's restart. */
export function AdvancedRow({ server, enabled, health, info }: AdvancedRowProps) {
  const [open, setOpen] = useState<boolean | null>(null);
  const [override, setOverride] = useState<{
    base: ServerInfoData | null;
    patch: Partial<Values>;
  }>({ base: null, patch: {} });
  const [busy, setBusy] = useState<string | null>(null);
  const [told, setTold] = useState<{ baseline: boolean; at: number } | null>(null);
  const runtime = useRestartState(server.id);
  const data = info.data;

  if (!data) {
    return (
      <OperationRow
        icon={<SlidersHorizontal />}
        title="Advanced"
        sub={
          info.error
            ? `Could not read its settings: ${info.error}`
            : "Reading its settings…"
        }
        tone={info.error ? "danger" : undefined}
      />
    );
  }

  const values = {
    ...valuesOf(data),
    ...(override.base === data ? override.patch : {}),
  };
  const serverId = data.id;
  const local = data.transport === "stdio";
  const isOpen = open ?? false;
  const changed = changedCount(values);
  const tools = health?.ok ? health.toolCount : null;
  const running = enabled && health?.ok === true;

  const restartedSince = (at: number) =>
    runtime.last?.status === "restarted" && runtime.last.at > at;
  const stillTold =
    told !== null && !restartedSince(told.at)
      ? values.declareClientCapabilities !== told.baseline
      : false;

  async function applyChange(
    key: string,
    patch: Partial<Values>,
    run: () => Promise<unknown>,
    undo: () => void,
    message: string,
  ) {
    setBusy(key);
    setOverride((current) => ({
      base: data,
      patch: { ...(current.base === data ? current.patch : {}), ...patch },
    }));
    try {
      await run();
      info.reload();
      toast.success(message, { action: { label: "Undo", onClick: undo } });
    } catch (error) {
      setOverride({ base: null, patch: {} });
      toastError(`Could not change ${key}`, { description: errorText(error) });
    } finally {
      setBusy(null);
    }
  }

  function chooseMode(mode: Values["mode"]) {
    const before = values.mode;
    const label = mode === "search" ? "Search only" : "Auto";
    const leaves =
      mode === "search" && tools !== null
        ? ` ${tools} ${tools === 1 ? "tool leaves" : "tools leave"} the list.`
        : "";
    void applyChange(
      "the tool listing",
      { mode },
      () => setToolListing(serverId, mode),
      () => chooseMode(before),
      `Tool listing for ${server.name}: ${label}.${leaves}`,
    );
  }

  function chooseSwitch(name: InteropSwitch, on: boolean) {
    if (name === "declareClientCapabilities" && running) {
      const current = told && !restartedSince(told.at) ? told : null;
      setTold(current ?? { baseline: !on, at: Date.now() });
    }
    void applyChange(
      SWITCH_LABEL[name],
      { [name]: on },
      () => setInteropSwitch(serverId, name, on),
      () => chooseSwitch(name, !on),
      `${SWITCH_LABEL[name]}: ${on ? "on" : "off"} for ${server.name}.`,
    );
  }

  let note: string | null = null;
  if (!enabled) {
    note = `${server.name} is off. Your settings are saved and are read the next time it starts.`;
  } else if (!running) {
    note = `${server.name} is not connected yet. Your settings are saved and are read when it starts, so there is nothing to restart.`;
  }

  const callRunning = runtime.phase.kind === "busy" || runtime.phase.kind === "waiting";
  const pending = stillTold && running && (
    <div
      role="status"
      className="flex flex-col gap-2 rounded-md border border-border bg-muted/50 p-2.5 text-meta"
    >
      <p>
        {values.declareClientCapabilities ? (
          <>
            <span className="font-medium">Not told yet.</span> {server.name} is running
            and was started without this. It is told the next time a client connects
            (Toolport restarts it once then, and not while a call is running), or you can
            restart it now.
          </>
        ) : (
          <>
            <span className="font-medium">Still told.</span> {server.name} is running and
            was started with this. It stops being told at its next start, or you can
            restart it now.
          </>
        )}
      </p>
      {callRunning ? (
        <p className="text-muted-foreground">
          A call is running. The Runtime row asks whether to wait or restart anyway.
        </p>
      ) : (
        <div>
          <Button
            variant="outline"
            size="sm"
            disabled={runtime.phase.kind === "asking"}
            onClick={() => void restart(server.id, server.name)}
          >
            Restart now
          </Button>
        </div>
      )}
    </div>
  );

  return (
    <OperationRow
      icon={<SlidersHorizontal />}
      title="Advanced"
      sub={summaryLine(values, data.interop, local)}
      badge={
        changed === 0 ? (
          <Badge variant="off">Defaults</Badge>
        ) : (
          <Badge variant="info">{changed} changed</Badge>
        )
      }
      open={isOpen}
      onToggle={() => setOpen(!isOpen)}
    >
      {note && <p className="text-meta text-muted-foreground">{note}</p>}
      <ToolListing
        value={values.mode}
        tools={tools}
        busy={busy !== null}
        onChange={chooseMode}
      />
      <SwitchSetting
        name="declareClientCapabilities"
        on={values.declareClientCapabilities}
        busy={busy !== null}
        onChange={(on) => chooseSwitch("declareClientCapabilities", on)}
        help={
          <>
            <p className="text-meta text-muted-foreground">
              Some servers ask you to confirm a risky step, for example before deleting a
              database. They only ask when the client says it can show questions, and
              Toolport sits in between, so by default it tells the server nothing is
              possible. On: Toolport tells the server what your clients can do: ask you
              questions, list project folders and let the server ask the model.
            </p>
            <Help
              fallback="Off"
              change={`A server never asks for confirmation although its documentation says it does, or it refuses a risky action because "the client does not support questions". Leave it off for servers that never ask.`}
              takes="The server has to be told when it starts. A running server is restarted once when a client that can do more connects, or restart it now from the Runtime row. Needs no restart if the server is not running yet."
            />
          </>
        }
      >
        {pending}
      </SwitchSetting>
      <SwitchSetting
        name="forwardInstructions"
        on={values.forwardInstructions}
        busy={busy !== null}
        onChange={(on) => chooseSwitch("forwardInstructions", on)}
        help={
          <>
            <p className="text-meta text-muted-foreground">
              Many servers carry a short how-to for the model (&quot;use tool A before
              tool B&quot;). By default the model only reads Toolport&apos;s own text. On:
              the server&apos;s text is added after it, under the heading{" "}
              <em>Instructions from server &quot;{server.name}&quot;</em>. At most 4,096
              characters, cleaned of control characters, and left out when it looks like
              it tries to override you.
            </p>
            <Help
              fallback="Off"
              change="A server with many tools is used badly without its own guidance. Leave it off for servers you do not fully trust: this text goes into the model's context."
              takes="When a client starts a new session. No restart. Sessions already running keep what they have."
            />
          </>
        }
      />
      {local ? (
        <CallLimit key={data.id} server={server} info={data} onSaved={info.reload} />
      ) : (
        <Setting
          title="Longest a call may run"
          registryKey="maxRequestTimeoutMs"
          titleId={`${server.id}-limit-remote`}
          control={<Badge variant="off">Local servers only</Badge>}
        >
          <p className="text-meta text-muted-foreground">
            Not used for remote servers. A call to a remote server waits for its request
            timeout (set in Edit) and no longer, even if the server reports progress.
          </p>
        </Setting>
      )}
    </OperationRow>
  );
}
