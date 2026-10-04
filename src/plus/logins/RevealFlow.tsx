import { useCallback, useEffect, useRef, useState } from "react";
import { toastError } from "@/lib/toast";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ctlData } from "../bridge/ctl";
import type { SecretGetData } from "../types/secret";
import { TypedConfirmDialog, errorText } from "../ui";
import type { ServerRef } from "./plans";
import type { Tier } from "./useRoster";

export const REVEAL_SECONDS = 10;

function ValueDialog({
  label,
  value,
  onHide,
}: {
  label: string;
  value: string;
  onHide: () => void;
}) {
  const [left, setLeft] = useState(REVEAL_SECONDS);
  const hide = useRef(onHide);
  useEffect(() => {
    hide.current = onHide;
  });
  useEffect(() => {
    const deadline = Date.now() + REVEAL_SECONDS * 1000;
    const timer = window.setInterval(() => {
      const rest = Math.ceil((deadline - Date.now()) / 1000);
      if (rest <= 0) hide.current();
      else setLeft(rest);
    }, 250);
    const away = () => document.hidden && hide.current();
    document.addEventListener("visibilitychange", away);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", away);
    };
  }, []);
  return (
    <Dialog open onOpenChange={(open) => !open && onHide()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{label}</DialogTitle>
          <DialogDescription>Hides in {left} s</DialogDescription>
        </DialogHeader>
        <code
          aria-label="Secret value"
          className="rounded bg-muted px-2 py-1.5 font-mono text-sm break-all"
        >
          {value}
        </code>
        <DialogFooter>
          <Button onClick={onHide}>Hide now</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** `secret get --reveal`: a confirmation first (typed when the policy tier asks for it), then
 * the value for ten seconds. The value lives in this component's state for that window only and
 * is dropped when it ends, when the dialog closes or when the window is hidden. */
export function RevealFlow({
  server,
  secretKey,
  tier,
  onClose,
}: {
  server: ServerRef;
  secretKey: string;
  tier: Tier;
  onClose: () => void;
}) {
  const [value, setValue] = useState<string | null>(null);
  const revealed = useRef(false);
  const hide = useCallback(() => {
    setValue(null);
    onClose();
  }, [onClose]);

  if (value !== null) {
    return (
      <ValueDialog label={`${secretKey} of ${server.name}`} value={value} onHide={hide} />
    );
  }

  async function confirm() {
    try {
      const data = await ctlData<SecretGetData>([
        "secret",
        "get",
        server.id,
        secretKey,
        "--reveal",
      ]);
      if (typeof data.value !== "string")
        throw new Error("The command returned no value");
      revealed.current = true;
      setValue(data.value);
    } catch (error) {
      toastError(`Couldn't reveal ${secretKey}: ${errorText(error).message}`);
      throw error;
    }
  }

  const onOpenChange = (open: boolean) => {
    if (!open && !revealed.current) onClose();
  };
  const body = (
    <div className="flex flex-col gap-3">
      <Callout variant="warning">
        The value stays visible for {REVEAL_SECONDS} seconds, then hides again. It is not
        copied or logged.
      </Callout>
      <p className="text-xs text-muted-foreground">
        Runs{" "}
        <code className="font-mono">
          toolportctl secret get {server.id} {secretKey} --reveal
        </code>
      </p>
    </div>
  );
  return tier === "destructive" ? (
    <TypedConfirmDialog
      open
      onOpenChange={onOpenChange}
      title={`Reveal ${secretKey}?`}
      phrase={secretKey}
      confirmLabel={`Reveal for ${REVEAL_SECONDS} seconds`}
      onConfirm={confirm}
    >
      {body}
    </TypedConfirmDialog>
  ) : (
    <ConfirmDialog
      open
      onOpenChange={onOpenChange}
      title={`Reveal ${secretKey}?`}
      contentClassName="sm:max-w-md"
      description={body}
      confirmLabel={`Reveal for ${REVEAL_SECONDS} seconds`}
      onConfirm={confirm}
    />
  );
}
