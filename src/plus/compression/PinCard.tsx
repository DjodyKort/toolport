import { useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { CompressionPinData } from "../types/compression";
import { AsyncView, type CtlQuery } from "../ui";
import { Field, OptionsDialog } from "./forms";
import { ctl, planOfPin, planOfUpdate } from "./model";
import type { WriteControl } from "./useWrite";

const DOWNLOAD = "This downloads the engine package from the network.";
const VERSION = /^\d+(\.\d+){1,3}([-+.][0-9A-Za-z.]+)?$/;

function SetPinForm({
  current,
  write,
  onClose,
}: {
  current: string;
  write: WriteControl;
  onClose: () => void;
}) {
  const [version, setVersion] = useState("");
  const ok = VERSION.test(version.trim());
  return (
    <OptionsDialog
      title="Set the engine pin"
      description={`The pin is the exact engine build Toolport expects. It is ${current} now.`}
      submitLabel="Preview"
      canSubmit={ok}
      onClose={onClose}
      onSubmit={() => {
        onClose();
        write.begin({
          command: "compression pin",
          title: `Pin the engine to ${version.trim()}`,
          argv: ctl("pin", version.trim()),
          confirmLabel: "Set pin",
          phrase: version.trim(),
          plan: (data) => planOfPin(data, `toolportctl compression pin ${current}`),
        });
      }}
    >
      <Field
        label="Version"
        hint={version && !ok ? "Use a version such as 0.30.0" : "For example 0.30.0"}
      >
        {(id, hint) => (
          <Input
            id={id}
            aria-describedby={hint}
            aria-invalid={version !== "" && !ok}
            value={version}
            onChange={(e) => setVersion(e.target.value)}
          />
        )}
      </Field>
    </OptionsDialog>
  );
}

function UpdateForm({
  current,
  write,
  onClose,
}: {
  current: string;
  write: WriteControl;
  onClose: () => void;
}) {
  const [latest, setLatest] = useState(true);
  const [version, setVersion] = useState("");
  const ok = latest || VERSION.test(version.trim());
  const target = latest ? ["--latest"] : ["--to", version.trim()];
  return (
    <OptionsDialog
      title="Update the engine"
      description="Moves the pin, installs that build and re-snapshots the presets. You see the move first, then accept it."
      submitLabel="Preview"
      canSubmit={ok}
      onClose={onClose}
      onSubmit={() => {
        onClose();
        write.begin({
          command: "compression update",
          title: latest
            ? "Update the engine to the latest build"
            : `Update the engine to ${version.trim()}`,
          argv: ctl("update", ...target, "--accept"),
          previewArgv: ctl("update", ...target),
          confirmLabel: "Accept update",
          phrase: latest ? "latest" : version.trim(),
          notice: DOWNLOAD,
          plan: planOfUpdate,
        });
      }}
    >
      <fieldset className="flex flex-col gap-2">
        <legend className="text-sm font-medium">Target</legend>
        <label className="flex items-center gap-2 text-sm">
          <input
            type="radio"
            name="update-target"
            checked={latest}
            onChange={() => setLatest(true)}
          />
          The latest build (pinned now: {current})
        </label>
        <label className="flex items-center gap-2 text-sm">
          <input
            type="radio"
            name="update-target"
            checked={!latest}
            onChange={() => setLatest(false)}
          />
          A specific version
        </label>
      </fieldset>
      {!latest && (
        <Field
          label="Version"
          hint={version && !ok ? "Use a version such as 0.30.0" : undefined}
        >
          {(id, hint) => (
            <Input
              id={id}
              aria-describedby={hint}
              aria-invalid={version !== "" && !ok}
              value={version}
              onChange={(e) => setVersion(e.target.value)}
            />
          )}
        </Field>
      )}
    </OptionsDialog>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-wrap items-baseline justify-between gap-2 px-3 py-2 text-sm">
      <span className="text-muted-foreground">{label}</span>
      <span className="min-w-0 text-right break-words">{children}</span>
    </div>
  );
}

/** The engine pin: what is pinned, what is installed, and the writes that move it. */
export function PinCard({
  query,
  write,
}: {
  query: CtlQuery<CompressionPinData>;
  write: WriteControl;
}) {
  const [form, setForm] = useState<"set" | "update" | null>(null);
  const close = () => setForm(null);
  return (
    <section aria-label="Engine pin" className="flex flex-col gap-2">
      <h3 className="text-sm font-semibold">Engine pin</h3>
      <AsyncView
        query={query}
        errorTitle="Couldn't read the engine pin"
        context="compression pin"
      >
        {(pin) => {
          const undo = `toolportctl compression pin ${pin.pin}`;
          return (
            <>
              <div className="divide-y rounded-lg border">
                <Row label="Pinned">{pin.pin}</Row>
                <Row label="Requirement">
                  <code className="font-mono text-xs">{pin.requirement}</code>
                </Row>
                <Row label="Installed">
                  {typeof pin.installed === "string" ? (
                    <>
                      {pin.installed}{" "}
                      {pin.drift ? (
                        <Badge variant="warning">differs from the pin</Badge>
                      ) : (
                        <Badge variant="success">matches the pin</Badge>
                      )}
                    </>
                  ) : (
                    <span className="text-muted-foreground">Not installed</span>
                  )}
                </Row>
              </div>
              <div className="flex flex-wrap gap-2">
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() =>
                    write.begin({
                      command: "compression pin",
                      title: `Install the pinned engine ${pin.pin}`,
                      argv: ctl("pin", "--install"),
                      confirmLabel: "Install",
                      phrase: pin.pin,
                      notice: DOWNLOAD,
                      plan: (data) => planOfPin(data, undo),
                    })
                  }
                >
                  Install
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() =>
                    write.begin({
                      command: "compression pin",
                      title: "Refresh the preset knobs from the installed engine",
                      argv: ctl("pin", "--refresh"),
                      confirmLabel: "Refresh",
                      phrase: pin.pin,
                      notice: DOWNLOAD,
                      plan: (data) => planOfPin(data, undo),
                    })
                  }
                >
                  Refresh knobs
                </Button>
                <Button size="sm" variant="outline" onClick={() => setForm("set")}>
                  Set pin…
                </Button>
                <Button size="sm" variant="outline" onClick={() => setForm("update")}>
                  Update…
                </Button>
              </div>
              {form === "set" && (
                <SetPinForm current={pin.pin} write={write} onClose={close} />
              )}
              {form === "update" && (
                <UpdateForm current={pin.pin} write={write} onClose={close} />
              )}
            </>
          );
        }}
      </AsyncView>
    </section>
  );
}
