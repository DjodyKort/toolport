import { useId, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { toastError } from "@/lib/toast";
import { cn } from "@/lib/utils";
import { errorText } from "../settings/useCtl";
import {
  budgetChanges,
  budgetRows,
  checkBudgets,
  span,
  type BudgetRow,
} from "./advancedModel";
import { Setting } from "./AdvancedParts";
import { setToolBudgets } from "./commands";
import type { ServerInfoData } from "./data";

export function ToolBudgets({
  serverName,
  info,
  onSaved,
}: {
  serverName: string;
  info: ServerInfoData;
  onSaved: () => void;
}) {
  const id = useId();
  const stored = info.toolBudgets;
  const [draft, setDraft] = useState<BudgetRow[] | null>(null);
  const [nextKey, setNextKey] = useState(1_000);
  const [saved, setSaved] = useState(false);
  const [saving, setSaving] = useState(false);
  const limitMs = info.maxRequestTimeoutMs ?? info.interop.defaultLimitMs;

  const rows = draft ?? budgetRows(stored);
  const { budgets, errors } = checkBudgets(rows);
  const changes = budgetChanges(stored, budgets);
  const invalid = Object.keys(errors).length > 0;
  const edit = (next: BudgetRow[]) => {
    setDraft(next);
    setSaved(false);
  };
  const update = (key: number, patch: Partial<BudgetRow>) =>
    edit(rows.map((row) => (row.key === key ? { ...row, ...patch } : row)));

  async function save() {
    setSaving(true);
    try {
      await setToolBudgets(info.id, changes);
      setDraft(budgetRows(budgets));
      setSaved(true);
      onSaved();
    } catch (error) {
      toastError(`Could not save the tool budgets of ${serverName}`, {
        description: errorText(error),
      });
    } finally {
      setSaving(false);
    }
  }

  let status = "Nothing to save.";
  let tone: "muted" | "ok" = "muted";
  if (saved) {
    status = "Saved. Applies to the next call.";
    tone = "ok";
  } else if (changes.length > 0 && !invalid) {
    status = `${changes.length} ${changes.length === 1 ? "change" : "changes"} to save.`;
  } else if (invalid) {
    status = "Fix the marked rows to save.";
  }

  return (
    <Setting title="Time budget per tool" registryKey="toolBudgets" titleId={id}>
      <p className="text-meta text-muted-foreground">
        Give one tool its own wait. Without one, a tool uses the budget its server lists
        for it, else the longest a call may run above. A budget replaces the request
        timeout for that tool: with no progress the call is cut after it, and each
        progress report starts a new wait of that length. It never goes beyond the longest
        a call may run ({span(limitMs)}).
      </p>
      {rows.length === 0 && (
        <p className="text-meta text-muted-foreground">No tool has its own budget.</p>
      )}
      {rows.map((row) => {
        const error = errors[row.key];
        const seconds =
          row.seconds.trim() === "" ? null : (budgets[row.tool.trim()] ?? null);
        const longer = seconds !== null && seconds * 1000 > limitMs;
        return (
          <div
            key={row.key}
            className="grid grid-cols-[minmax(0,1fr)_5.25rem_auto] items-center gap-x-2 gap-y-1"
          >
            <Input
              aria-label="Tool name"
              value={row.tool}
              aria-invalid={error !== undefined}
              onChange={(event) => update(row.key, { tool: event.target.value })}
            />
            <Input
              aria-label={`Seconds for ${row.tool.trim() || "the new tool"}`}
              inputMode="decimal"
              value={row.seconds}
              aria-invalid={error !== undefined}
              onChange={(event) => update(row.key, { seconds: event.target.value })}
            />
            <span className="flex items-center gap-1">
              <span className="text-meta text-muted-foreground">s</span>
              <Button
                variant="ghost"
                size="sm"
                aria-label={`Remove ${row.tool.trim() || "the new tool"}`}
                onClick={() => edit(rows.filter((other) => other.key !== row.key))}
              >
                Remove
              </Button>
            </span>
            {error !== undefined && (
              <p role="alert" className="col-span-full text-meta text-destructive">
                {error}
              </p>
            )}
            {error === undefined && longer && (
              <p className="col-span-full text-meta text-warning">
                {span((seconds ?? 0) * 1000)} is longer than the call limit (
                {span(limitMs)}): the call is still cut after {span(limitMs)}. Raise the
                call limit above, or lower this.
              </p>
            )}
          </div>
        );
      })}
      <div className="flex flex-wrap items-center gap-2">
        <Button
          variant="outline"
          size="sm"
          onClick={() => {
            edit([...rows, { key: nextKey, tool: "", seconds: "" }]);
            setNextKey(nextKey + 1);
          }}
        >
          Add a tool
        </Button>
        <Button
          size="sm"
          disabled={changes.length === 0 || invalid || saving || saved}
          onClick={() => void save()}
        >
          Save
        </Button>
        {draft !== null && !saved && (
          <Button
            variant="ghost"
            size="sm"
            disabled={saving}
            onClick={() => setDraft(null)}
          >
            Discard changes
          </Button>
        )}
        <span
          role="status"
          className={cn(
            "text-meta",
            tone === "ok" ? "text-success" : "text-muted-foreground",
          )}
        >
          {status}
        </span>
      </div>
    </Setting>
  );
}
