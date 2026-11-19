import { useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import PlanDialog from "../context/PlanDialog";
import { changeKnobs, type KnobChange } from "./pluginsCommands";
import type { KnobRow, OptionRow, PluginShowData } from "./pluginsData";
import { fromText, knobChoices, knobValue } from "./pluginsModel";

const SELECT_CLASS =
  "h-8 rounded-md border bg-background px-2 text-sm text-foreground focus-visible:ring-3 focus-visible:ring-ring/50 focus-visible:outline-none";

function OptionLine({ option }: { option: OptionRow }) {
  return (
    <li className="flex flex-wrap items-center gap-3 px-4 py-2.5">
      <div className="min-w-48 flex-1">
        <p className="text-sm font-medium">{option.title}</p>
        {option.description && (
          <p className="text-meta text-muted-foreground">{option.description}</p>
        )}
      </div>
      {option.sensitive ? (
        <Badge variant="off">Hidden</Badge>
      ) : (
        <span className="font-mono text-xs">{option.current ?? "not set"}</span>
      )}
    </li>
  );
}

interface Target {
  knob: KnobRow;
  /** `null` writes the plugin option for every folder. */
  cwd: string | null;
  change: KnobChange;
}

function KnobDialog({
  knob,
  folder,
  onClose,
  onPlan,
}: {
  knob: KnobRow;
  folder: string | null;
  onClose: () => void;
  onPlan: (target: Target) => void;
}) {
  const choices = knobChoices(knob);
  const [value, setValue] = useState(
    knob.current.value ?? knob.default ?? choices?.[0] ?? "",
  );
  const places = [
    ...(folder ? [{ id: "folder", label: `This folder (${folder})`, cwd: folder }] : []),
    ...(knob.option
      ? [{ id: "all", label: "Every folder (plugin option)", cwd: null }]
      : []),
  ];
  const [place, setPlace] = useState(places[0]?.id ?? "");
  const chosen = places.find((p) => p.id === place);
  const submit = (change: KnobChange) =>
    chosen && onPlan({ knob, cwd: chosen.cwd, change });
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{knob.label}</DialogTitle>
          <DialogDescription>
            {knob.folderOnly
              ? "This switch is written to one folder."
              : "Written to one folder, or to the plugin's own options for every folder."}
          </DialogDescription>
        </DialogHeader>
        {places.length === 0 ? (
          <p className="text-meta text-muted-foreground">
            Choose a folder first: this switch only exists per folder.
          </p>
        ) : (
          <form
            className="flex flex-col gap-3"
            onSubmit={(event) => {
              event.preventDefault();
              submit({ set: { [knob.key]: value }, unset: [] });
            }}
          >
            {places.length > 1 && (
              <label className="flex flex-col gap-1 text-meta">
                Where
                <select
                  className={SELECT_CLASS}
                  value={place}
                  onChange={(event) => setPlace(event.target.value)}
                >
                  {places.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.label}
                    </option>
                  ))}
                </select>
              </label>
            )}
            <label className="flex flex-col gap-1 text-meta">
              Value
              {choices ? (
                <select
                  className={SELECT_CLASS}
                  value={value}
                  onChange={(event) => setValue(event.target.value)}
                >
                  {choices.map((choice) => (
                    <option key={choice} value={choice}>
                      {choice}
                    </option>
                  ))}
                </select>
              ) : (
                <Input
                  value={value}
                  onChange={(event) => setValue(event.target.value)}
                  placeholder="comma separated"
                />
              )}
            </label>
            <DialogFooter>
              <Button
                type="button"
                variant="ghost"
                onClick={() => submit({ set: {}, unset: [knob.key] })}
              >
                {knob.option && chosen?.cwd === null
                  ? "Back to default"
                  : "Remove my value"}
              </Button>
              <Button type="button" variant="outline" onClick={onClose}>
                Cancel
              </Button>
              <Button type="submit" disabled={!value.trim()}>
                Review
              </Button>
            </DialogFooter>
          </form>
        )}
      </DialogContent>
    </Dialog>
  );
}

/** A plugin's own options and the knobs of its adapter, with where each value comes from. A
 * change is written after its plan, in the chosen folder or as a plugin option. */
export function PluginSettings({
  plugin,
  folder,
  onChanged,
}: {
  plugin: PluginShowData;
  folder: string | null;
  onChanged: () => void;
}) {
  const [editing, setEditing] = useState<KnobRow | null>(null);
  const [target, setTarget] = useState<Target | null>(null);
  return (
    <section aria-label="Settings" className="flex flex-col gap-2">
      <h2 className="text-meta font-semibold tracking-wide text-muted-foreground uppercase">
        Settings
        <span className="ml-1 font-normal normal-case">
          {plugin.options.length + plugin.knobs.length}
        </span>
      </h2>
      {plugin.options.length === 0 && plugin.knobs.length === 0 && (
        <p className="rounded-xl border bg-card px-4 py-4 text-meta text-muted-foreground">
          This plugin declares no options and Toolport has no adapter for it.
        </p>
      )}
      {plugin.options.length > 0 && (
        <>
          <ul
            aria-label="Plugin options"
            className="divide-y overflow-hidden rounded-xl border bg-card"
          >
            {plugin.options.map((option) => (
              <OptionLine key={option.key} option={option} />
            ))}
          </ul>
          <p className="text-meta text-muted-foreground">
            Values marked Hidden are never shown here. Change an option with{" "}
            <code className="font-mono text-xs">claude plugin configure {plugin.id}</code>
            .
          </p>
        </>
      )}
      {plugin.knobs.length > 0 && (
        <ul
          aria-label="Switches"
          className="divide-y overflow-hidden rounded-xl border bg-card"
        >
          {plugin.knobs.map((knob) => (
            <li key={knob.key} className="flex flex-wrap items-center gap-3 px-4 py-2.5">
              <div className="min-w-48 flex-1">
                <p className="text-sm font-medium">{knob.label}</p>
                <p className="text-meta text-muted-foreground">
                  {fromText(knob.current.from)}
                  {knob.folderOnly ? " · per folder" : ""}
                </p>
              </div>
              <span className="font-mono text-xs">{knobValue(knob)}</span>
              <Button
                variant="outline"
                size="sm"
                aria-label={`Change ${knob.label}`}
                onClick={() => setEditing(knob)}
              >
                Change…
              </Button>
            </li>
          ))}
        </ul>
      )}
      {plugin.knobs.length > 0 && (
        <p className="text-meta text-muted-foreground">
          A hook that is switched off still starts a process and exits at once; only
          turning the plugin off removes it. Claude Code reads these when a session
          starts.
        </p>
      )}
      {plugin.adapterProblems.map((problem) => (
        <p key={problem.file} className="text-meta text-destructive">
          {problem.file}: {problem.message}
        </p>
      ))}
      {editing && !target && (
        <KnobDialog
          knob={editing}
          folder={folder}
          onClose={() => setEditing(null)}
          onPlan={setTarget}
        />
      )}
      {target && (
        <PlanDialog
          title={`Change ${target.knob.label}`}
          confirmLabel="Change"
          preview={() => changeKnobs(plugin.id, target.cwd, target.change, true)}
          confirm={() => changeKnobs(plugin.id, target.cwd, target.change, false)}
          onClose={() => {
            setTarget(null);
            setEditing(null);
          }}
          onDone={onChanged}
        />
      )}
    </section>
  );
}
