import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { CompressionStatusData } from "../bridge/data";
import type { CompressionPresetsData } from "../types/compression";
import { AsyncView, useCtlQuery, type CtlQuery } from "../ui";
import { PROVIDERS, ctl, planOfWrite } from "./model";
import { WriteDialogs, useRegistryRows, useWrite, type WriteControl } from "./useWrite";

function Stat({ label, value, note }: { label: string; value: string; note?: string }) {
  return (
    <div className="flex min-w-0 flex-col gap-0.5 rounded-lg border bg-card p-3">
      <span className="text-xs text-muted-foreground">{label}</span>
      <b className="text-base font-semibold break-words">{value}</b>
      {note && (
        <small className="text-xs break-words text-muted-foreground">{note}</small>
      )}
    </div>
  );
}

function engineText(pin: CompressionStatusData["pin"], provider: string) {
  if (provider === "none" || provider === "rtk-only")
    return {
      value: provider === "none" ? "Off" : "Healthy",
      note: "not used by this provider",
    };
  if (pin.installed === null)
    return { value: "Not installed", note: `pin ${pin.pin} is not on PATH` };
  return pin.drift
    ? { value: "Drift", note: `${pin.installed} installed, pin ${pin.pin}` }
    : { value: "Healthy", note: `${pin.installed} (pinned)` };
}

function StatusStrip({ status }: { status: CompressionStatusData }) {
  const { preset, pin } = status;
  const engine = engineText(pin, status.provider);
  return (
    <div
      aria-label="Compression status"
      className="grid gap-3 sm:grid-cols-2 lg:grid-cols-5"
    >
      <Stat label="Provider" value={status.provider} note={`${status.runtime} runtime`} />
      <Stat
        label="Preset"
        value={preset.name}
        note={`${preset.mode} mode · port ${preset.port}`}
      />
      <Stat
        label="Scope"
        value={status.scope.join(", ")}
        note={`${status.contexts} folder policies`}
      />
      <Stat label="Engine" value={engine.value} note={engine.note} />
      <Stat label="Pin" value={pin.pin} note={pin.package} />
    </div>
  );
}

function ProviderSwitcher({
  status,
  write,
}: {
  status: CompressionStatusData;
  write: WriteControl;
}) {
  const undo = `toolportctl compression set-provider ${status.provider}`;
  return (
    <section aria-label="Provider" className="flex flex-col gap-2">
      <h3 className="text-sm font-semibold">Provider</h3>
      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
        {PROVIDERS.map((provider) => {
          const active = provider.id === status.provider;
          return (
            <div
              key={provider.id}
              className={`flex flex-col gap-1 rounded-lg border p-3 ${active ? "border-primary" : ""}`}
            >
              <b className="flex items-center gap-2 text-sm">
                {provider.id}
                {active && <Badge variant="success">active</Badge>}
              </b>
              <small className="text-xs text-muted-foreground">{provider.blurb}</small>
              {!active && (
                <Button
                  size="sm"
                  variant="outline"
                  className="mt-1 self-start"
                  aria-label={`Switch to ${provider.id}`}
                  onClick={() =>
                    write.begin({
                      command: "compression set-provider",
                      title: `Switch the provider to ${provider.id}`,
                      argv: ctl("set-provider", provider.id),
                      confirmLabel: "Switch provider",
                      phrase: provider.id,
                      plan: (data) => planOfWrite(data, undo),
                    })
                  }
                >
                  Switch…
                </Button>
              )}
            </div>
          );
        })}
      </div>
    </section>
  );
}

function Presets({
  status,
  query,
  write,
}: {
  status: CompressionStatusData;
  query: CtlQuery<CompressionPresetsData>;
  write: WriteControl;
}) {
  const undo = `toolportctl compression use ${status.preset.name}`;
  return (
    <section aria-label="Presets" className="flex flex-col gap-2">
      <h3 className="text-sm font-semibold">Presets</h3>
      <AsyncView query={query} errorTitle="Couldn't load the presets">
        {(data) => (
          <ul className="divide-y rounded-lg border">
            {data.presets.map((preset) => (
              <li
                key={preset.name}
                className="flex flex-wrap items-center gap-3 px-3 py-2"
              >
                <div className="min-w-0 flex-1">
                  <b className="text-sm">{preset.name}</b>
                  <small className="block text-xs text-muted-foreground">
                    {preset.mode}
                    {preset.savingsProfile ? ` · ${preset.savingsProfile}` : ""} · port{" "}
                    {preset.port} · {preset.knobCount} knobs
                    {preset.snapshotVersion
                      ? ` · snapshot ${preset.snapshotVersion}`
                      : ""}
                  </small>
                </div>
                {preset.active ? (
                  <Badge variant="success">active</Badge>
                ) : (
                  <Button
                    size="sm"
                    variant="outline"
                    aria-label={`Use ${preset.name}`}
                    onClick={() =>
                      write.begin({
                        command: "compression use",
                        title: `Use the ${preset.name} preset`,
                        argv: ctl("use", preset.name),
                        confirmLabel: "Use preset",
                        phrase: preset.name,
                        plan: (result) => planOfWrite(result, undo),
                      })
                    }
                  >
                    Use
                  </Button>
                )}
              </li>
            ))}
          </ul>
        )}
      </AsyncView>
    </section>
  );
}

/** The Compression tab of the Tokens screen. PHASE 1 so far: status, provider switch and
 * presets; the other cards are listed in the item's progress file. */
export function CompressionTab() {
  const rows = useRegistryRows();
  const status = useCtlQuery<CompressionStatusData>(ctl("status"));
  const presets = useCtlQuery<CompressionPresetsData>(ctl("presets"));
  const { reload: reloadStatus } = status;
  const { reload: reloadPresets } = presets;
  const write = useWrite(rows, () => {
    reloadStatus();
    reloadPresets();
  });
  return (
    <div className="flex flex-col gap-5">
      <AsyncView
        query={status}
        errorTitle="Couldn't read the compression policy"
        context="compression status"
      >
        {(data) => (
          <>
            <StatusStrip status={data} />
            <ProviderSwitcher status={data} write={write} />
            <Presets status={data} query={presets} write={write} />
          </>
        )}
      </AsyncView>
      <WriteDialogs write={write} />
    </div>
  );
}
