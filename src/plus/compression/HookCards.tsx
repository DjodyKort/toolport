import { useState } from "react";
import {
  Copy,
  Lock,
  PauseCircle,
  PlugZap,
  ShieldCheck,
  TriangleAlert,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { StatusCard } from "@/components/ui/status-card";
import { RowShell } from "../context/StackRows";
import type {
  CompressionStatusData,
  EntryHealth,
  HooksWriteData,
  VerifyRow,
} from "./data";
import {
  HOOK_INFO,
  PARSEC_CONSENT,
  PROXY_COMMANDS,
  latencyText,
  type Headline,
  type HookRow,
  type Tile,
  type TileState,
  untilText,
} from "./model";
import { copyLine } from "./useHookWrites";

const settingsName = (file: string) => file.replace(/^.*\/(\.claude[^/]*\/)/, "~/$1");

export function CopyLine({ text }: { text: string }) {
  return (
    <div className="flex items-center gap-2">
      <code className="min-w-0 flex-1 rounded-md bg-muted px-2 py-1 text-meta break-all">
        {text}
      </code>
      <Button
        size="icon-sm"
        variant="ghost"
        aria-label={`Copy ${text}`}
        onClick={() => copyLine(text)}
      >
        <Copy />
      </Button>
    </div>
  );
}

const WRAP_ACTION: Record<string, string> = {
  "would wrap": "wrapped",
  "would re-apply": "put back",
  "would restore ours": "restored to what Toolport wrote",
  kept: "already Toolport's, untouched",
  "none to wrap": "not found, nothing to wrap",
};

/** The dry run of an adoption: what each entry becomes, and that nothing else is touched. */
export function AdoptPreview({ plan }: { plan: HooksWriteData }) {
  const rows = plan.rows.filter((r) => r.action !== "none to wrap");
  return (
    <div className="flex flex-col gap-2 text-meta" data-testid="adopt-preview">
      <p>
        Only the <code>hooks</code> key of the settings file changes, through the safe
        writer. Your other entries are not touched, and Undo puts your original lines
        back.
      </p>
      <ul className="flex flex-col gap-2">
        {rows.map((row) => (
          <li key={`${row.file}:${row.provider}`} className="flex flex-col gap-0.5">
            <span className="font-medium">
              {row.provider} in {settingsName(row.file)}: {WRAP_ACTION[row.action]}
            </span>
            {row.command && (
              <>
                <code className="break-all text-muted-foreground">− {row.command}</code>
                <code className="break-all">
                  + {HOOK_INFO[row.provider].wrapper} -- /bin/sh -c &apos;{row.command}
                  &apos;
                </code>
              </>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}

export function HeadlineCard({
  headline,
  rows,
  pausedSince,
  pausedUntil,
  busy,
  preview,
  checks,
  onAdopt,
  onPreview,
  onNotNow,
  onRepair,
  onPause,
  onResume,
  onVerify,
}: {
  headline: Headline;
  rows: HookRow[];
  pausedSince: string | null;
  pausedUntil: string | null;
  busy: boolean;
  preview: HooksWriteData | null;
  checks: React.ReactNode;
  onAdopt: () => void;
  onPreview: () => void;
  onNotNow: () => void;
  onRepair: () => void;
  onPause: () => void;
  onResume: () => void;
  onVerify: () => void;
}) {
  const names = (ids: string[]) => ids.join(" and ");
  const verify = (
    <Button size="sm" variant="outline" disabled={busy} onClick={onVerify}>
      Verify all
    </Button>
  );
  switch (headline.kind) {
    case "attention":
      return (
        <StatusCard
          role="alert"
          variant="warning"
          icon={<TriangleAlert />}
          title={`A compression hook needs you: ${names(headline.broken)}`}
          detail="Compression for it has stopped or no longer does what Toolport wrote, and nothing errors. Restore it to what Toolport wrote, or undo the adoption on its card."
          actions={
            <Button size="sm" disabled={busy} onClick={onRepair}>
              Restore ours
            </Button>
          }
        >
          {checks}
        </StatusCard>
      );
    case "paused":
      return (
        <StatusCard
          variant="warning"
          icon={<PauseCircle />}
          title={
            pausedUntil
              ? `Compression is paused until ${untilText(pausedUntil)}`
              : `Compression is paused since ${pausedSince?.slice(0, 10) ?? "an earlier day"}`
          }
          detail={
            pausedUntil
              ? "The wrapped hooks answer nothing for the window and answer again by themselves; the ledger records the epoch, so measurements group days correctly."
              : "The wrapped hooks answer nothing until you resume, so Claude Code runs without rtk and read-once. Nothing resumes by itself: compare matched days, then resume."
          }
          actions={
            <Button size="sm" disabled={busy} onClick={onResume}>
              Resume now
            </Button>
          }
        >
          {checks}
        </StatusCard>
      );
    case "adopt":
      return (
        <StatusCard
          variant="info"
          icon={<PlugZap />}
          title={`Toolport found ${headline.foreign.length} compression ${headline.foreign.length === 1 ? "hook" : "hooks"} it did not write`}
          detail={`${names(headline.foreign)} ${headline.foreign.length === 1 ? "is" : "are"} wired by hand in settings.json. Nothing checks ${headline.foreign.length === 1 ? "it" : "them"} today, so if they break nothing would notice. Adopt them and Toolport counts their calls, can verify them and can undo them.`}
          actions={
            <>
              <Button size="sm" disabled={busy} onClick={onAdopt}>
                Adopt
              </Button>
              <Button size="sm" variant="outline" disabled={busy} onClick={onPreview}>
                Preview adoption…
              </Button>
              <Button size="sm" variant="ghost" onClick={onNotNow}>
                Not now
              </Button>
            </>
          }
        >
          {preview && <AdoptPreview plan={preview} />}
          {checks}
        </StatusCard>
      );
    case "on":
      return (
        <StatusCard
          variant="success"
          icon={<ShieldCheck />}
          title={`Compression is on: ${headline.owned} ${headline.owned === 1 ? "hook" : "hooks"} written by Toolport, all healthy`}
          detail="rtk shortens Bash output and read-once skips repeat reads. They act inside Claude Code only; MCP results are shaped by the gateway itself. Pausing for a day lets you compare matched days; the hooks resume by themselves."
          actions={
            <>
              <Button size="sm" variant="outline" disabled={busy} onClick={onPause}>
                Pause for a day
              </Button>
              {verify}
            </>
          }
        >
          {checks}
        </StatusCard>
      );
    case "none":
      return (
        <StatusCard
          variant="info"
          icon={<PlugZap />}
          title="No compression hooks found"
          detail={`Neither rtk nor read-once is wired in ${rows.length > 0 ? "a Claude settings file" : "settings.json"}. Toolport does not install third-party binaries: install one and wire its hook, and Toolport offers to adopt it here.`}
        />
      );
  }
}

const HEALTH_BADGE: Record<
  Exclude<EntryHealth, "absent">,
  { variant: "success" | "warning" | "off"; label: string }
> = {
  ok: { variant: "success", label: "ok" },
  edited: { variant: "warning", label: "edited by hand" },
  missing: { variant: "warning", label: "missing from settings.json" },
  foreign: { variant: "off", label: "not written by Toolport" },
};

export interface CardCheck {
  at: string;
  row: VerifyRow;
}

function VerifyResult({ result }: { result: CardCheck }) {
  const { row } = result;
  const pass = row.status === "pass";
  return (
    <div
      className="flex flex-col gap-0.5 text-meta"
      data-testid={`verify-${row.provider}`}
    >
      <p className="flex flex-wrap items-center gap-1.5">
        <Badge variant={pass ? "success" : row.status === "fail" ? "destructive" : "off"}>
          {pass ? "verified" : row.status === "fail" ? "failed" : "not checked"}
        </Badge>
        <span>
          Checked at {result.at}
          {row.latencyMs !== null && row.latencyMs !== undefined && pass
            ? ` · ${row.latencyMs} ms`
            : ""}
        </span>
      </p>
      <p className="break-words">
        {pass
          ? `It ${row.returned ?? "answered"}.`
          : (row.reason ?? "No entry to check.")}
      </p>
      {row.paused && (
        <p className="text-muted-foreground">
          The hooks are paused; this check ran the command anyway.
        </p>
      )}
    </div>
  );
}

export function HookCard({
  row,
  paused,
  busy,
  verification,
  onAdopt,
  onUndo,
  onRepair,
  onKeep,
  onVerify,
}: {
  row: HookRow;
  paused: boolean;
  busy: boolean;
  verification: CardCheck | null;
  onAdopt: () => void;
  onUndo: () => void;
  onRepair: () => void;
  onKeep: () => void;
  onVerify: () => void;
}) {
  const info = HOOK_INFO[row.provider];
  if (row.health === "absent") return null;
  const badge = HEALTH_BADGE[row.health];
  const owned = row.health !== "foreign";
  const detail =
    row.health === "foreign"
      ? `Found a ${row.provider} hook that Toolport did not write. It works today.`
      : row.health === "missing"
        ? `Wired ${row.wrappedAt?.slice(0, 10) ?? "earlier"}; the entry is gone from settings.json.`
        : row.health === "edited"
          ? "The command differs from what Toolport wrote; the hook may not do what this card says."
          : latencyText(row.counter);
  return (
    <RowShell
      title={row.provider}
      badge={
        <>
          <Badge variant="outline">hook</Badge>
          <Badge variant={badge.variant}>{badge.label}</Badge>
          {paused && owned && row.health === "ok" && (
            <Badge variant="warning">paused</Badge>
          )}
        </>
      }
      subtitle={info.summary}
      trailing={
        <>
          {row.health === "foreign" && (
            <Button
              size="sm"
              variant="outline"
              disabled={busy}
              aria-label={`Adopt ${row.provider}`}
              onClick={onAdopt}
            >
              Adopt
            </Button>
          )}
          {(row.health === "missing" || row.health === "edited") && (
            <Button
              size="sm"
              disabled={busy}
              aria-label={`${row.health === "missing" ? "Re-apply" : "Restore ours for"} ${row.provider}`}
              onClick={onRepair}
            >
              {row.health === "missing" ? "Re-apply" : "Restore ours"}
            </Button>
          )}
          {row.health === "edited" && (
            <Button
              size="sm"
              variant="outline"
              disabled={busy}
              aria-label={`Keep my edit of ${row.provider}`}
              onClick={onKeep}
            >
              Keep my edit
            </Button>
          )}
          {(row.health === "ok" || row.health === "edited") && (
            <Button
              size="sm"
              variant="outline"
              disabled={busy}
              aria-label={`Verify ${row.provider}`}
              onClick={onVerify}
            >
              Verify
            </Button>
          )}
          {owned && row.health !== "missing" && (
            <Button
              size="sm"
              variant="ghost"
              disabled={busy}
              aria-label={`Undo adoption of ${row.provider}`}
              onClick={onUndo}
            >
              Undo adoption
            </Button>
          )}
        </>
      }
    >
      <p className="text-meta break-words text-muted-foreground">
        PreToolUse · {info.matcher} · <code>{info.wrapper}</code>
        {row.files > 1 && ` · in ${row.files} settings files`}
      </p>
      <p className="text-meta break-words" data-testid={`hook-detail-${row.provider}`}>
        {detail}
      </p>
      {row.health === "edited" && (
        <p className="text-meta text-muted-foreground">
          Restore ours puts back what Toolport wrote. Keep my edit makes the edited
          command Toolport&apos;s own, and Undo adoption then restores your edit.
        </p>
      )}
      {verification && <VerifyResult result={verification} />}
    </RowShell>
  );
}

const TILE_BADGE: Record<
  TileState,
  { variant: "success" | "warning" | "off" | "outline"; label: string }
> = {
  active: { variant: "success", label: "active" },
  paused: { variant: "warning", label: "paused" },
  "not-managed": { variant: "outline", label: "not managed by Toolport" },
  attention: { variant: "warning", label: "needs you" },
  "not-found": { variant: "off", label: "not found" },
  off: { variant: "off", label: "off" },
  locked: { variant: "off", label: "locked" },
};

const KIND_WORD = { hook: "hook", proxy: "proxy", hosted: "hosted" } as const;

export function ProviderTile({ tile }: { tile: Tile }) {
  const [open, setOpen] = useState(false);
  const badge = TILE_BADGE[tile.state];
  return (
    <RowShell
      title={tile.id}
      badge={
        <>
          <Badge variant="outline">{KIND_WORD[tile.kind]}</Badge>
          <Badge variant={badge.variant}>
            {tile.state === "locked" && <Lock data-icon="inline-start" />}
            {badge.label}
          </Badge>
        </>
      }
      subtitle={`${tile.summary} ${tile.trust}`}
      trailing={
        (tile.state === "locked" || (tile.id === "headroom" && tile.state === "off")) && (
          <Button
            size="sm"
            variant="ghost"
            aria-expanded={open}
            aria-label={
              tile.id === "parsec" ? "Show why parsec is locked" : "Why headroom is off"
            }
            onClick={() => setOpen(!open)}
          >
            {tile.id === "parsec" ? "Show why" : "Why it is off"}
          </Button>
        )
      }
    >
      {open && tile.id === "parsec" && (
        <div className="flex flex-col gap-2 text-meta">
          <p>
            <strong>Sends your code off this machine.</strong> parsec is a closed engine.
            It cannot be chosen by a preset. Enabling needs an explicit consent that is
            written to the ledger:
          </p>
          <CopyLine text={PARSEC_CONSENT} />
        </div>
      )}
      {open && tile.id === "headroom" && (
        <p className="text-meta">
          A proxy that rewrites the request body can break the prompt-cache prefix. Most
          context tokens are read from the prompt cache after their first turn, so
          rewriting history can cost more than it saves. Toolport checks this with
          <code> toolportctl compression verify</code> before it routes anything.
        </p>
      )}
    </RowShell>
  );
}

export function AdvancedPanel({ status }: { status: CompressionStatusData | null }) {
  const [open, setOpen] = useState(false);
  return (
    <section
      aria-label="Advanced: compression proxy"
      className="flex flex-col gap-2 rounded-xl border bg-card p-3"
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="text-sm font-semibold">
          Advanced: compression proxy (headroom){" "}
          <span className="font-normal text-muted-foreground">
            {status?.provider === "headroom" ? "on" : "off"}
          </span>
        </h3>
        <Button
          size="sm"
          variant="outline"
          aria-expanded={open}
          onClick={() => setOpen(!open)}
        >
          {open ? "Hide engine controls" : "Show engine controls"}
        </Button>
      </div>
      <p className="text-meta text-muted-foreground">
        The proxy is optional and not recommended while most of your context is read from
        the prompt cache: it changes the API route and can break the cache prefix.
      </p>
      {open && status && (
        <div className="flex flex-col gap-2 text-meta" data-testid="engine-controls">
          <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1">
            <dt className="text-muted-foreground">Provider</dt>
            <dd>{status.provider}</dd>
            <dt className="text-muted-foreground">Preset</dt>
            <dd>
              {status.preset.name} ({status.preset.mode} mode, port {status.preset.port})
            </dd>
            <dt className="text-muted-foreground">Engine pin</dt>
            <dd>
              {status.pin.pin}
              {status.pin.installed
                ? `, installed ${status.pin.installed}`
                : ", not installed"}
            </dd>
          </dl>
          <p>Engine controls are terminal commands:</p>
          {PROXY_COMMANDS.map((line) => (
            <CopyLine key={line} text={line} />
          ))}
        </div>
      )}
    </section>
  );
}
