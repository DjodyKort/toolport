import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { toastError } from "@/lib/toast";
import { fixIsActionable, plusAuthFix, plusAuthRows } from "./api";
import type { AuthRow, AuthRows as AuthRowsData } from "./api";

const label: Record<string, string> = {
  ok: "Signed in",
  expiring: "Expiring",
  needs_reauth: "Needs sign-in",
  revoked: "Revoked",
  misconfigured: "Misconfigured",
  unreachable: "Unreachable",
  unknown: "Unknown",
};

export function AuthRows({
  rows,
  onFix,
  busy = null,
}: {
  rows: AuthRow[];
  onFix?: (row: AuthRow) => void;
  busy?: string | null;
}) {
  if (rows.length === 0) return null;
  return (
    <ul aria-label="Server auth health" className="divide-y rounded-md border">
      {rows.map((row) => (
        <li key={row.server} className="flex items-center gap-3 px-3 py-2">
          <span className="font-mono text-sm">{row.server}</span>
          <span
            data-state={row.state}
            title={row.reason}
            className="text-xs text-muted-foreground"
          >
            {label[row.state] ?? row.state}
            {row.ttlSecs !== null && row.ttlSecs > 0
              ? ` (${Math.ceil(row.ttlSecs / 60)} min)`
              : ""}
          </span>
          {row.fix &&
            (fixIsActionable(row.fix) ? (
              <button
                type="button"
                className="ml-auto text-xs underline disabled:no-underline disabled:opacity-60"
                disabled={busy !== null}
                aria-busy={busy === row.server}
                onClick={() => onFix?.(row)}
              >
                {busy === row.server ? "Working..." : row.fix.label}
              </button>
            ) : (
              <span className="ml-auto text-xs text-muted-foreground">
                {row.fix.label}
              </span>
            ))}
        </li>
      ))}
    </ul>
  );
}

export function AuthPanel({ onOpenLogins }: { onOpenLogins?: () => void } = {}) {
  const [data, setData] = useState<AuthRowsData | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const refresh = useCallback(() => plusAuthRows().then(setData), []);

  useEffect(() => {
    let alive = true;
    plusAuthRows()
      .then((d) => alive && setData(d))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, []);

  const fix = useCallback(
    async (row: AuthRow) => {
      if (!row.fix) return;
      setBusy(row.server);
      try {
        toast.success(await plusAuthFix(row.fix));
      } catch (e) {
        toastError(`Couldn't fix ${row.server}: ${e}`);
      } finally {
        setBusy(null);
        await refresh().catch(() => {});
      }
    },
    [refresh],
  );

  if (!data || data.rows.length === 0) return null;
  const attention = data.rows.filter((row) => row.fix !== null).length;
  return (
    <section aria-label="Sign-in health" className="flex flex-col gap-2">
      <header className="flex items-baseline gap-2">
        <h2 className="text-sm font-medium">Sign-in health</h2>
        <span className="text-xs text-muted-foreground">
          {attention === 0
            ? "All signed in"
            : `${attention} ${attention === 1 ? "needs" : "need"} attention`}
        </span>
        {onOpenLogins && (
          <button
            type="button"
            className="ml-auto text-xs underline"
            onClick={onOpenLogins}
          >
            Open Logins &amp; secrets
          </button>
        )}
      </header>
      <AuthRows rows={data.rows} onFix={fix} busy={busy} />
    </section>
  );
}
