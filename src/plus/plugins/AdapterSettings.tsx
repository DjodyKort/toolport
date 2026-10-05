import { useId } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Callout } from "@/components/Callout";
import type { AdapterKnob } from "../types/plugins";
import { KNOB_FROM, KNOB_WRITTEN, pick, type Draft } from "./model";

const SELECT =
  "h-8 rounded-md border bg-background px-2 text-sm outline-none focus-visible:ring-1 focus-visible:ring-ring";

function choicesOf(knob: AdapterKnob): Array<[string, string]> {
  if (knob.kind === "bool")
    return [
      ["true", "on"],
      ["false", "off"],
    ];
  if (knob.kind === "bool-off")
    return [
      ["on", "on"],
      ["off", "off"],
    ];
  return (knob.choices ?? []).map((choice) => [choice, choice]);
}

function Control({
  knob,
  draft,
  onChange,
}: {
  knob: AdapterKnob;
  draft: Draft;
  onChange: (key: string, value: string | null | undefined) => void;
}) {
  const id = useId();
  const picked = knob.key in draft;
  const value = draft[knob.key];
  const set = (next: string) =>
    onChange(knob.key, next === "" ? undefined : pick(knob, next));
  if (knob.kind === "hook-ids") {
    const chosen = new Set((value ?? "").split(",").filter(Boolean));
    const options = knob.choices ?? [];
    return (
      <fieldset className="flex flex-col gap-1" aria-label={knob.label}>
        {options.length === 0 && (
          <span className="text-xs text-muted-foreground">
            The plugin names no hook ids Toolport can switch.
          </span>
        )}
        <div className="grid max-h-40 grid-cols-1 gap-1 overflow-auto sm:grid-cols-2">
          {options.map((hook) => (
            <label key={hook} className="flex items-center gap-2 text-xs">
              <input
                type="checkbox"
                checked={chosen.has(hook)}
                onChange={(event) => {
                  const next = new Set(chosen);
                  if (event.target.checked) next.add(hook);
                  else next.delete(hook);
                  onChange(knob.key, next.size === 0 ? null : [...next].join(","));
                }}
              />
              <code className="font-mono">{hook}</code>
            </label>
          ))}
        </div>
      </fieldset>
    );
  }
  if (knob.kind === "csv" || knob.kind === "globs") {
    return (
      <Input
        id={id}
        aria-label={knob.label}
        value={value ?? ""}
        placeholder={knob.current.value ?? "leave as is"}
        spellCheck={false}
        autoComplete="off"
        onChange={(event) => set(event.target.value)}
      />
    );
  }
  const shown = !picked ? "" : value === null ? "on" : (value ?? "");
  return (
    <select
      id={id}
      aria-label={knob.label}
      className={SELECT}
      value={shown}
      onChange={(event) => set(event.target.value)}
    >
      <option value="">leave as is</option>
      {choicesOf(knob).map(([choice, label]) => (
        <option key={choice} value={choice}>
          {label}
        </option>
      ))}
    </select>
  );
}

/** The switches a plugin's adapter names, as controls. A change is a draft until "Apply to a
 * folder" runs it through plan and confirm; nothing is written from here. */
export function AdapterSettings({
  knobs,
  draft,
  onChange,
  folder,
  onApply,
  onUndo,
  canUndo,
  busy,
}: {
  knobs: AdapterKnob[];
  draft: Draft;
  onChange: (key: string, value: string | null | undefined) => void;
  folder: string;
  onApply: () => void;
  onUndo: () => void;
  canUndo: boolean;
  busy: boolean;
}) {
  const changed = Object.keys(draft).length;
  const reason = folder
    ? null
    : "Choose a folder above: these settings are written in the folder where Claude Code starts.";
  return (
    <div role="group" aria-label="Plugin settings" className="flex flex-col gap-3">
      <ul className="flex flex-col divide-y rounded-lg border">
        {knobs.map((knob) => (
          <li
            key={knob.key}
            className="grid grid-cols-1 gap-2 px-3 py-2 text-sm sm:grid-cols-[12rem_minmax(0,1fr)_14rem]"
          >
            <span className="flex flex-col">
              <b>{knob.label}</b>
              <small className="text-muted-foreground">{KNOB_WRITTEN[knob.kind]}</small>
            </span>
            <span className="flex min-w-0 flex-col text-xs text-muted-foreground">
              <span>
                Now:{" "}
                <code className="font-mono text-foreground">
                  {knob.current.value ?? "not set"}
                </code>{" "}
                ({KNOB_FROM[knob.current.from]})
              </span>
              <span>
                Written as <code className="font-mono">{knob.env}</code>
                {knob.option ? `, or the plugin option ${knob.option}` : " in the folder"}
              </span>
            </span>
            <span className="flex items-center gap-2">
              <Control knob={knob} draft={draft} onChange={onChange} />
              {(knob.key in draft || knob.current.from === "folder-env") && (
                <Button
                  size="xs"
                  variant="ghost"
                  onClick={() =>
                    onChange(
                      knob.key,
                      knob.key in draft && draft[knob.key] === null ? undefined : null,
                    )
                  }
                  aria-pressed={draft[knob.key] === null}
                  title="Take this key out of the folder's settings"
                >
                  Unset here
                </Button>
              )}
            </span>
          </li>
        ))}
      </ul>
      <Callout variant="warning" role="note">
        A hook that is switched off here still starts a process and exits at once. Only
        turning the plugin off removes it. Counts are counted from matchers, not timed.
      </Callout>
      <p className="text-xs text-muted-foreground">
        Settings go into <code>.claude/settings.local.json</code> of the folder as{" "}
        <code>env</code> keys, which that folder&apos;s Claude Code reads. A key you set
        in your own settings.json <code>env</code> is replaced there, not added to; the
        plan says so. Undo restores only the keys Toolport wrote.
      </p>
      <div className="flex flex-wrap gap-2">
        <Button
          size="sm"
          disabled={busy || changed === 0 || reason !== null}
          title={reason ?? (changed === 0 ? "Change a setting first" : undefined)}
          onClick={onApply}
        >
          Apply to a folder…
        </Button>
        <Button
          size="sm"
          variant="outline"
          disabled={busy || !canUndo || reason !== null}
          title={reason ?? (canUndo ? undefined : "Nothing set in this folder to undo")}
          onClick={onUndo}
        >
          Undo in a folder…
        </Button>
      </div>
      {reason && <p className="text-xs text-muted-foreground">{reason}</p>}
    </div>
  );
}
