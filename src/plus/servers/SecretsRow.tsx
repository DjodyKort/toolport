import { useEffect, useState } from "react";
import { Copy, EyeOff, KeyRound } from "lucide-react";
import { toast } from "sonner";
import { LazySecretsDialog } from "@/components/LazySecretsDialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { Registry, ServerEntry } from "@/lib/types";
import type { AuthRow } from "../bridge/data";
import { plural } from "../settings/model";
import { errorText } from "../settings/useCtl";
import { revealSecret, secretIsSet } from "./commands";
import type { ServerInfoData } from "./data";
import { loginFailed } from "./loginModel";
import { OperationRow } from "./OperationRow";
import { RevealConfirm, REVEAL_SECONDS } from "./SecretDialogs";
import { useLoad, type Loaded } from "./useLoad";

const HTTP_AUTH_KEY = "__http_auth__";

interface Revealed {
  key: string;
  value: string;
  until: number;
}

/** One value on screen at a time. It leaves when the time is up, when the window loses focus or
 * is hidden, and when the row goes away. */
function useRevealed() {
  const [shown, setShown] = useState<Revealed | null>(null);
  const [left, setLeft] = useState(0);

  useEffect(() => {
    if (!shown) return;
    const hide = () => setShown(null);
    const tick = () => {
      const remaining = Math.ceil((shown.until - Date.now()) / 1000);
      if (remaining <= 0) hide();
      else setLeft(remaining);
    };
    tick();
    const timer = setInterval(tick, 250);
    const onVisibility = () => document.hidden && hide();
    window.addEventListener("blur", hide);
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      clearInterval(timer);
      window.removeEventListener("blur", hide);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [shown]);

  return {
    shown,
    left,
    show: (key: string, value: string) =>
      setShown({ key, value, until: Date.now() + REVEAL_SECONDS * 1000 }),
    hide: () => setShown(null),
  };
}

function RevealedValue({
  secretKey,
  value,
  left,
  onHide,
}: {
  secretKey: string;
  value: string;
  left: number;
  onHide: () => void;
}) {
  async function copy() {
    try {
      await navigator.clipboard.writeText(value);
      toast.success(`Copied ${secretKey}`);
    } catch {
      toast.error("Could not copy the value");
    }
  }
  return (
    <div
      role="group"
      aria-label={`${secretKey} value`}
      className="flex flex-wrap items-center gap-2 rounded-md border border-border bg-muted/40 px-2 py-1.5"
    >
      <code className="min-w-0 flex-1 font-mono text-meta break-all">{value}</code>
      <span className="text-meta text-muted-foreground">Hides in {left} s</span>
      <Button variant="outline" size="xs" onClick={() => void copy()}>
        <Copy /> Copy
      </Button>
      <Button variant="ghost" size="xs" onClick={onHide}>
        <EyeOff /> Hide now
      </Button>
    </div>
  );
}

export interface SecretsRowProps {
  server: ServerEntry;
  serverId: string;
  serverName: string;
  info: Loaded<ServerInfoData>;
  /** Plain settings as the app's registry holds them; their values are not secret. */
  plain: Array<{ key: string; value: string | null }>;
  login: AuthRow | undefined;
  /** Upstream's dialog hands back the registry after it saved or removed a value. */
  onSaved: (registry: Registry) => void;
  /** Runs after a value was set or removed, so the page and Finish setup read the new state. */
  onChanged: () => void | Promise<void>;
}

/** Keyed by server id where it is mounted, so a revealed value never outlives the server's page.
 * The secret keys a server declares and whether the keychain holds them, with a reveal for ten
 * seconds behind a confirmation. Setting, replacing and removing a value is upstream's dialog. */
export function SecretsRow({
  server,
  serverId,
  serverName,
  info,
  plain,
  login,
  onSaved,
  onChanged,
}: SecretsRowProps) {
  const keys = (info.data?.env ?? []).filter((e) => e.secret).map((e) => e.key);
  const [version, setVersion] = useState(0);
  const signature = keys.join();
  const loaded = useLoad(
    async () => ({
      signature,
      entries: await Promise.all(
        keys.map(async (key) => [key, await secretIsSet(serverId, key)] as const),
      ),
    }),
    `${serverId}|${signature}|${version}`,
  );
  const status = {
    data: loaded.data?.signature === signature ? loaded.data.entries : null,
    error: loaded.error,
  };
  const [revealing, setRevealing] = useState<string | null>(null);
  const [userOpen, setUserOpen] = useState<boolean | null>(null);
  const revealed = useRevealed();

  if (keys.length === 0) return null;

  const isSet = new Map(status.data ?? []);
  const missing = keys.filter((key) => status.data && !isSet.get(key));
  const unreadable = status.error !== null && status.data === null;
  const rejected = login !== undefined && loginFailed(login) && missing.length === 0;
  const open = userOpen ?? (missing.length > 0 || unreadable || rejected);

  let line: string;
  if (unreadable) line = `The keychain did not answer: ${status.error}`;
  else if (!status.data) line = "Asking the keychain…";
  else if (missing.length > 0)
    line = `${missing.join(", ")} ${missing.length === 1 ? "is" : "are"} not set`;
  else line = keys.length === 1 ? `${keys[0]} is set` : `${keys.length} secrets are set`;
  if (plain.length > 0 && !unreadable && status.data)
    line += ` · ${plural(plain.length, "plain setting")}`;
  if (rejected) line += " · the server rejected it";

  const badge = unreadable ? (
    <Badge variant="warning">Locked</Badge>
  ) : !status.data ? (
    <Badge variant="off">Checking</Badge>
  ) : missing.length > 0 ? (
    <Badge variant="warning">{missing.length} missing</Badge>
  ) : (
    <Badge variant={rejected ? "destructive" : "success"}>
      {rejected ? "Set, rejected" : "Set"}
    </Badge>
  );

  const target = (key: string) => ({ serverId, serverName, secretKey: key });
  const bump = () => {
    setVersion((value) => value + 1);
    void onChanged();
  };

  return (
    <>
      <OperationRow
        icon={<KeyRound />}
        title="Secrets"
        sub={<span aria-live="polite">{line}</span>}
        tone={
          missing.length > 0 || unreadable ? "warning" : rejected ? "danger" : undefined
        }
        badge={badge}
        actions={
          <>
            {unreadable && (
              <Button variant="outline" onClick={() => setVersion((value) => value + 1)}>
                Try again
              </Button>
            )}
            <LazySecretsDialog
              server={server}
              onSaved={onSaved}
              onChanged={bump}
              trigger={<Button variant="outline">Edit secrets…</Button>}
            />
          </>
        }
        open={open}
        onToggle={() => setUserOpen(!open)}
      >
        <ul className="flex flex-col gap-2" aria-label="Declared secrets">
          {keys.map((key) => {
            const known = isSet.get(key);
            return (
              <li
                key={key}
                data-secret={key}
                className="flex flex-col gap-2 rounded-md border border-border px-3 py-2"
              >
                <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
                  <span className="min-w-0 flex-1 basis-[180px]">
                    <span className="block font-mono text-sm break-all">{key}</span>
                    <span className="block text-meta text-muted-foreground">
                      {known ? "Keychain · set" : status.data ? "Not set" : "Keychain"}
                    </span>
                  </span>
                  <div className="flex flex-wrap items-center gap-2">
                    {known && key !== HTTP_AUTH_KEY && (
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => setRevealing(key)}
                      >
                        Reveal…
                      </Button>
                    )}
                  </div>
                </div>
                {revealed.shown?.key === key && (
                  <RevealedValue
                    secretKey={key}
                    value={revealed.shown.value}
                    left={revealed.left}
                    onHide={revealed.hide}
                  />
                )}
              </li>
            );
          })}
        </ul>
        {plain.length > 0 && (
          <div className="flex flex-col gap-1">
            <span className="text-meta font-semibold">Plain settings</span>
            <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1 text-meta">
              {plain.map((entry) => (
                <div key={entry.key} className="contents">
                  <dt className="font-mono text-muted-foreground">{entry.key}</dt>
                  <dd className="font-mono break-all">{entry.value ?? ""}</dd>
                </div>
              ))}
            </dl>
          </div>
        )}
        <p className="text-meta text-muted-foreground">
          Secret values stay off this screen unless you ask for one. Plain settings are
          shown because they are not secret.
        </p>
      </OperationRow>
      {revealing !== null && (
        <RevealConfirm
          {...target(revealing)}
          open
          onOpenChange={(next) => !next && setRevealing(null)}
          onConfirm={async () => {
            try {
              revealed.show(revealing, await revealSecret(serverId, revealing));
            } catch (error) {
              toast.error(`Could not read ${revealing}`, {
                description: errorText(error),
              });
              throw error;
            }
          }}
        />
      )}
    </>
  );
}
