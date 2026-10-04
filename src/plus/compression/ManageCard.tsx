import { useState } from "react";
import { Button } from "@/components/ui/button";
import type { CompressionStatusData } from "../bridge/data";
import { Field, OptionsDialog, SELECT_CLASS } from "./forms";
import { Input } from "@/components/ui/input";
import {
  ctl,
  flagArgs,
  planOfSeal,
  planOfWrite,
  PROVIDERS,
  type FlagParts,
} from "./model";
import type { WriteControl } from "./useWrite";

type Form = "enable" | "disable" | "sync" | null;

const ENGINES = PROVIDERS.filter((p) => p.id !== "none");

function EnableForm({
  presets,
  current,
  write,
  onClose,
}: {
  presets: string[];
  current: string;
  write: WriteControl;
  onClose: () => void;
}) {
  const [provider, setProvider] = useState(current === "none" ? "headroom" : current);
  const [preset, setPreset] = useState("");
  const [mode, setMode] = useState("");
  const [port, setPort] = useState("");
  const [telemetry, setTelemetry] = useState("");
  const portOk = port === "" || /^\d{1,5}$/.test(port);
  const parts: FlagParts[] = [
    { flag: "--preset", value: preset },
    { flag: "--mode", value: mode },
    { flag: "--port", value: port },
    { flag: "--telemetry", value: telemetry },
  ];
  return (
    <OptionsDialog
      title="Enable compression"
      description="Pick the provider and, if you want, a preset, mode, port or telemetry setting. You see what changes before anything is written."
      submitLabel="Preview"
      canSubmit={portOk}
      onClose={onClose}
      onSubmit={() => {
        onClose();
        write.begin({
          command: "compression enable",
          title: `Enable ${provider}`,
          argv: ctl("enable", "--provider", provider, ...flagArgs(parts)),
          confirmLabel: "Enable",
          phrase: provider,
          plan: (data) => planOfWrite(data, `toolportctl compression disable`),
        });
      }}
    >
      <Field label="Provider">
        {(id) => (
          <select
            id={id}
            className={SELECT_CLASS}
            value={provider}
            onChange={(e) => setProvider(e.target.value)}
          >
            {ENGINES.map((p) => (
              <option key={p.id} value={p.id}>
                {p.id}
              </option>
            ))}
          </select>
        )}
      </Field>
      <Field label="Preset" hint="Leave on the active preset to keep it.">
        {(id, hint) => (
          <select
            id={id}
            aria-describedby={hint}
            className={SELECT_CLASS}
            value={preset}
            onChange={(e) => setPreset(e.target.value)}
          >
            <option value="">Keep the active preset</option>
            {presets.map((name) => (
              <option key={name} value={name}>
                {name}
              </option>
            ))}
          </select>
        )}
      </Field>
      <div className="grid gap-3 sm:grid-cols-3">
        <Field label="Mode">
          {(id) => (
            <select
              id={id}
              className={SELECT_CLASS}
              value={mode}
              onChange={(e) => setMode(e.target.value)}
            >
              <option value="">Keep</option>
              <option value="cache">cache</option>
              <option value="token">token</option>
            </select>
          )}
        </Field>
        <Field label="Port" hint={portOk ? undefined : "Digits only"}>
          {(id, hint) => (
            <Input
              id={id}
              aria-describedby={hint}
              aria-invalid={!portOk}
              inputMode="numeric"
              placeholder="Keep"
              value={port}
              onChange={(e) => setPort(e.target.value)}
            />
          )}
        </Field>
        <Field label="Telemetry">
          {(id) => (
            <select
              id={id}
              className={SELECT_CLASS}
              value={telemetry}
              onChange={(e) => setTelemetry(e.target.value)}
            >
              <option value="">Keep</option>
              <option value="on">on</option>
              <option value="off">off</option>
            </select>
          )}
        </Field>
      </div>
    </OptionsDialog>
  );
}

function DisableForm({
  status,
  write,
  onClose,
}: {
  status: CompressionStatusData;
  write: WriteControl;
  onClose: () => void;
}) {
  const [teardown, setTeardown] = useState(false);
  return (
    <OptionsDialog
      title="Disable compression"
      description="Turns the provider off. This is a destructive change, so you type a phrase to confirm after the preview."
      submitLabel="Preview"
      onClose={onClose}
      onSubmit={() => {
        onClose();
        write.begin({
          command: "compression disable",
          title: "Disable compression",
          argv: ctl("disable", ...(teardown ? ["--teardown"] : [])),
          confirmLabel: "Disable",
          phrase: "disable",
          plan: (data) =>
            planOfWrite(
              data,
              status.provider === "none"
                ? ""
                : `toolportctl compression enable --provider ${status.provider}`,
            ),
        });
      }}
    >
      <label className="flex items-start gap-2 text-sm">
        <input
          type="checkbox"
          className="mt-1"
          checked={teardown}
          onChange={(e) => setTeardown(e.target.checked)}
        />
        <span>
          Also tear the engine down
          <small className="block text-xs text-muted-foreground">
            Removes the engine's MCP entry and unwraps the Claude launcher (--teardown).
          </small>
        </span>
      </label>
    </OptionsDialog>
  );
}

function SyncForm({ write, onClose }: { write: WriteControl; onClose: () => void }) {
  const [root, setRoot] = useState("");
  return (
    <OptionsDialog
      title="Sync the policy"
      description="Re-applies the policy: shims, the env snippet and the MCP entry."
      submitLabel="Preview"
      onClose={onClose}
      onSubmit={() => {
        onClose();
        write.begin({
          command: "compression sync",
          title: "Sync the compression policy",
          argv: ctl("sync", ...flagArgs([{ flag: "--mcpm-root", value: root }])),
          confirmLabel: "Sync",
          phrase: "sync",
          plan: (data) => planOfWrite(data, ""),
        });
      }}
    >
      <Field
        label="mcpm folder"
        hint="Optional. Only needed while an old mcpm install is still around."
      >
        {(id, hint) => (
          <Input
            id={id}
            aria-describedby={hint}
            placeholder="Leave empty to skip"
            value={root}
            onChange={(e) => setRoot(e.target.value)}
          />
        )}
      </Field>
    </OptionsDialog>
  );
}

const PROXY_HINTS = {
  proxy_up:
    "The engine (headroom) is not installed. Install the pin first, see Engine pin.",
  proxy_down: "No proxy is running, so there is nothing to stop.",
  no_proxy: "Start the proxy first, then seal what it runs.",
};

/** Enable, disable, sync, seal and the proxy lifecycle: the policy writes that are not a
 * switch of provider or preset. */
export function ManageCard({
  status,
  presets,
  write,
}: {
  status: CompressionStatusData;
  presets: string[];
  write: WriteControl;
}) {
  const [form, setForm] = useState<Form>(null);
  const close = () => setForm(null);
  const proxy = (sub: "up" | "down" | "restart", label: string) => (
    <Button
      size="sm"
      variant="outline"
      onClick={() =>
        write.begin({
          command: `compression proxy ${sub}`,
          title: `${label} the compression proxy`,
          argv: ctl("proxy", sub),
          confirmLabel: label,
          phrase: sub,
          hints: PROXY_HINTS,
        })
      }
    >
      {label}
    </Button>
  );
  return (
    <section aria-label="Policy actions" className="flex flex-col gap-2">
      <h3 className="text-sm font-semibold">Policy</h3>
      <div className="flex flex-col gap-3 rounded-lg border p-3">
        <div className="flex flex-wrap items-center gap-2">
          <Button size="sm" variant="outline" onClick={() => setForm("enable")}>
            Enable…
          </Button>
          <Button size="sm" variant="outline" onClick={() => setForm("disable")}>
            Disable…
          </Button>
          <Button size="sm" variant="outline" onClick={() => setForm("sync")}>
            Sync…
          </Button>
          <Button
            size="sm"
            variant="outline"
            onClick={() =>
              write.begin({
                command: "compression seal",
                title: "Seal the live proxy posture",
                argv: ctl("seal", "--apply"),
                previewArgv: ctl("seal", "--dry-run"),
                confirmLabel: "Seal",
                phrase: "seal",
                plan: planOfSeal,
                hints: PROXY_HINTS,
              })
            }
          >
            Seal…
          </Button>
        </div>
        <div
          className="flex flex-wrap items-center gap-2"
          role="group"
          aria-label="Proxy"
        >
          <span className="text-xs text-muted-foreground">Proxy</span>
          {proxy("up", "Start")}
          {proxy("down", "Stop")}
          {proxy("restart", "Restart")}
        </div>
        <p className="text-xs text-muted-foreground">
          Seal declares the knobs the running proxy reports as policy, so a later preset
          refresh keeps them. It needs a running proxy.
        </p>
      </div>
      {form === "enable" && (
        <EnableForm
          presets={presets}
          current={status.provider}
          write={write}
          onClose={close}
        />
      )}
      {form === "disable" && (
        <DisableForm status={status} write={write} onClose={close} />
      )}
      {form === "sync" && <SyncForm write={write} onClose={close} />}
    </section>
  );
}
