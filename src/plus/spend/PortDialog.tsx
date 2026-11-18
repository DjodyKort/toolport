import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useLoad } from "../servers/useLoad";
import { errorText } from "../settings/useCtl";
import { setConfig, setupApply, setupPlan, spendStatus } from "./commands";
import type { StatusData } from "./data";
import { MAX_PORT, MIN_PORT, validPort } from "./plan";
import { ProfileDiffs } from "./SetupDialog";
import { DialogError, SpendDialog } from "./SpendDialog";

const CHECKS = 8;
const CHECK_MS = 400;

const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/** Only profiles Toolport already set up move: a port change never adds keys to another one. */
async function readSetUp() {
  return (await spendStatus()).setup.map((s) => s.profile);
}

/** The receiver rebinds in the app; look at its status until it names the new port. */
async function receiverOn(port: number): Promise<StatusData> {
  let status = await spendStatus();
  for (let check = 0; check < CHECKS; check += 1) {
    const here = status.receiver.address.endsWith(`:${port}`);
    if (here && (status.receiver.listening || status.receiver.error)) return status;
    await wait(CHECK_MS);
    status = await spendStatus();
  }
  return status;
}

export interface PortDialogProps {
  /** The port the receiver tried first. */
  current: number;
  suggested: number;
  onClose: () => void;
  onMoved: (port: number) => void;
}

/** Use another port: the plan shows what changes in Claude Code's settings, then the receiver
 * is moved and its status says whether it listens. A busy port changes nothing. */
export function PortDialog({ current, suggested, onClose, onMoved }: PortDialogProps) {
  const [text, setText] = useState(String(suggested));
  const port = validPort(text);
  const setUp = useLoad(readSetUp, "set-up");
  const profiles = setUp.data;
  const plan = useLoad(
    () =>
      port === null || profiles === null
        ? Promise.resolve(null)
        : setupPlan({ port, profiles }),
    `${port}|${profiles?.join("|")}`,
  );
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<{ title: string; detail: string } | null>(null);

  async function move() {
    if (port === null) return;
    setBusy(true);
    setFailure(null);
    try {
      await setConfig({ port });
      const status = await receiverOn(port);
      if (!status.receiver.listening) {
        await setConfig({ port: current });
        setFailure({
          title: `Port ${port} cannot be used`,
          detail: `${status.receiver.error ?? "The receiver did not start on it."} Nothing was changed; try another port.`,
        });
        return;
      }
      if (profiles && profiles.length > 0) {
        try {
          await setupApply({ port, profiles });
        } catch (error) {
          setFailure({
            title: "The receiver moved, but Claude Code's settings were not updated",
            detail: errorText(error),
          });
          return;
        }
      }
      onMoved(port);
    } catch (error) {
      setFailure({ title: "The port was not changed", detail: errorText(error) });
    } finally {
      setBusy(false);
    }
  }

  const data = plan.data;
  return (
    <SpendDialog
      title="Use another port"
      description={`Port ${current} is used by another program. The receiver moves to a free port and Claude Code is pointed at it.`}
      busy={busy}
      onClose={onClose}
      footer={
        <>
          <Button variant="outline" disabled={busy} onClick={onClose}>
            Cancel
          </Button>
          <Button disabled={port === null || !data || busy} onClick={() => void move()}>
            {busy ? "Moving…" : port === null ? "Use port" : `Use port ${port}`}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3 text-sm">
        <label className="flex flex-wrap items-center gap-2 text-meta">
          <span>Receiver port</span>
          <Input
            aria-label="Receiver port"
            inputMode="numeric"
            value={text}
            disabled={busy}
            aria-invalid={port === null}
            aria-describedby="port-hint"
            className="w-28"
            onChange={(event) => setText(event.target.value)}
          />
          <span id="port-hint" className="text-muted-foreground">
            {MIN_PORT} to {MAX_PORT}, 127.0.0.1 only
          </span>
        </label>
        {port === null && (
          <p role="alert" className="text-meta text-destructive">
            Enter a port from {MIN_PORT} to {MAX_PORT}.
          </p>
        )}
        {plan.error && !data && port !== null && (
          <DialogError title="The plan could not be made" detail={plan.error} />
        )}
        {data && (
          <>
            <p className="text-meta text-muted-foreground">
              It changes <code className="font-mono">OTEL_EXPORTER_OTLP_ENDPOINT</code>{" "}
              and nothing else. Data stored so far stays on the page.
            </p>
            <ProfileDiffs
              profiles={data.profiles
                .map((p) => ({
                  ...p,
                  entries: p.entries.filter((e) => e.action !== "already set"),
                }))
                .filter((p) => p.entries.length > 0)}
              replace={[]}
            />
          </>
        )}
        {busy && (
          <p role="status" className="text-meta text-muted-foreground">
            Restarting the receiver on port {port}…
          </p>
        )}
        {failure && <DialogError title={failure.title} detail={failure.detail} />}
      </div>
    </SpendDialog>
  );
}
