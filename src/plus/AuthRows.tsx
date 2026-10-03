import type { AuthRow } from "./api";

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
}: {
  rows: AuthRow[];
  onFix?: (row: AuthRow) => void;
}) {
  if (rows.length === 0) return null;
  return (
    <ul aria-label="Server auth health" className="divide-y">
      {rows.map((row) => (
        <li key={row.server} className="flex items-center gap-3 py-2">
          <span className="font-mono text-sm">{row.server}</span>
          <span data-state={row.state} className="text-xs text-muted-foreground">
            {label[row.state] ?? row.state}
            {row.ttlSecs !== null && row.ttlSecs > 0
              ? ` (${Math.ceil(row.ttlSecs / 60)} min)`
              : ""}
          </span>
          {row.fix && (
            <button
              type="button"
              className="ml-auto text-xs underline"
              onClick={() => onFix?.(row)}
            >
              {row.fix.label}
            </button>
          )}
        </li>
      ))}
    </ul>
  );
}
