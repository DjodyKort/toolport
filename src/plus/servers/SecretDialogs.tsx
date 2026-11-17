import { useState } from "react";
import { toast } from "sonner";
import { ConfirmDialog } from "@/components/ConfirmDialog";
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
import { Label } from "@/components/ui/label";
import { toastError } from "@/lib/toast";
import { errorText } from "../settings/useCtl";
import { checkLogin, removeSecret, removalNeedsTyping, setSecret } from "./commands";
import { clock } from "./format";
import { LOGIN_VIEWS, loginReason } from "./loginModel";
import { useLoad } from "./useLoad";

export const REVEAL_SECONDS = 10;

interface SecretTarget {
  serverId: string;
  serverName: string;
  secretKey: string;
}

export interface ReplaceSecretProps extends SecretTarget {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The server has a login check, so the new value can be tried right after saving. */
  canCheck: boolean;
  wasSet: boolean;
  onSaved: () => void;
}

/** A write-only field: the value goes to the CLI's stdin and is dropped from the page at once. */
export function ReplaceSecret({
  serverId,
  serverName,
  secretKey,
  open,
  onOpenChange,
  canCheck,
  wasSet,
  onSaved,
}: ReplaceSecretProps) {
  const [value, setValue] = useState("");
  const [check, setCheck] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState<string[] | null>(null);

  function close(next: boolean) {
    if (!next) {
      setValue("");
      setError(null);
      setSaved(null);
    }
    onOpenChange(next);
  }

  async function submit() {
    setBusy(true);
    setError(null);
    try {
      await setSecret(serverId, secretKey, value);
    } catch (failure) {
      setError(errorText(failure));
      setBusy(false);
      return;
    }
    setValue("");
    const lines = [`Saved ${secretKey} for ${serverName}.`];
    if (canCheck && check) {
      try {
        const data = await checkLogin(serverId);
        const row = data.servers.find((r) => r.server === serverId);
        const at = clock(Date.now());
        if (row) {
          lines.push(
            row.state === "ok"
              ? `The check at ${at} got a normal answer: ${serverName} is signed in.`
              : `The check at ${at} says ${LOGIN_VIEWS[row.state].label.toLowerCase()}${
                  loginReason(row) ? `: ${loginReason(row)}` : ""
                }.`,
          );
        }
      } catch (failure) {
        lines.push(`The check could not run: ${errorText(failure)}`);
      }
    }
    setSaved(lines);
    setBusy(false);
    onSaved();
  }

  return (
    <Dialog open={open} onOpenChange={close}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>
            {saved ? "Saved" : `${wasSet ? "Replace" : "Set"} ${secretKey}`}
          </DialogTitle>
          <DialogDescription>
            {saved
              ? `For ${serverName}.`
              : wasSet
                ? `For ${serverName}. The old value is overwritten and cannot be shown again.`
                : `For ${serverName}.`}
          </DialogDescription>
        </DialogHeader>
        {saved ? (
          <ul className="flex flex-col gap-1 text-sm" aria-label="Result">
            {saved.map((line) => (
              <li key={line}>{line}</li>
            ))}
          </ul>
        ) : (
          <form
            className="flex flex-col gap-3"
            onSubmit={(event) => {
              event.preventDefault();
              if (value !== "" && !busy) void submit();
            }}
          >
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="plus-secret-value">New value</Label>
              <Input
                id="plus-secret-value"
                type="password"
                autoComplete="off"
                spellCheck={false}
                value={value}
                onChange={(event) => setValue(event.target.value)}
              />
              <p className="text-meta text-muted-foreground">
                Write-only: once you leave this window the value is gone from the screen.
                It goes straight to the keychain.
              </p>
            </div>
            {canCheck && (
              <label className="flex items-start gap-2 text-meta">
                <input
                  type="checkbox"
                  className="mt-1"
                  checked={check}
                  onChange={(event) => setCheck(event.target.checked)}
                />
                <span>Check the login right after saving</span>
              </label>
            )}
            {error && (
              <p role="alert" className="text-meta text-destructive">
                {error}
              </p>
            )}
            <DialogFooter>
              <Button type="button" variant="ghost" onClick={() => close(false)}>
                Cancel
              </Button>
              <Button type="submit" disabled={value === "" || busy}>
                {wasSet ? "Replace" : "Save"}
              </Button>
            </DialogFooter>
          </form>
        )}
        {saved && (
          <DialogFooter>
            <Button onClick={() => close(false)}>Close</Button>
          </DialogFooter>
        )}
      </DialogContent>
    </Dialog>
  );
}

export interface RevealConfirmProps extends SecretTarget {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onConfirm: () => Promise<void>;
}

export function RevealConfirm({
  serverName,
  secretKey,
  open,
  onOpenChange,
  onConfirm,
}: RevealConfirmProps) {
  return (
    <ConfirmDialog
      open={open}
      onOpenChange={onOpenChange}
      title={`Show ${secretKey}?`}
      description={
        <div className="flex flex-col gap-2 text-sm text-muted-foreground">
          <p>
            The value of {secretKey} for {serverName} appears for {REVEAL_SECONDS}{" "}
            seconds, then hides again.
          </p>
          <p>Anyone who can see this screen can read it.</p>
        </div>
      }
      confirmLabel={`Show for ${REVEAL_SECONDS} seconds`}
      onConfirm={onConfirm}
    />
  );
}

export interface RemoveSecretProps extends SecretTarget {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The server has a login check that will say so when the key is gone. */
  hasLoginCheck: boolean;
  onRemoved: () => void | Promise<void>;
}

/** What goes is listed first. The policy tier of `secret rm` decides whether the key must be typed. */
export function RemoveSecret({
  serverId,
  serverName,
  secretKey,
  open,
  onOpenChange,
  hasLoginCheck,
  onRemoved,
}: RemoveSecretProps) {
  const [typed, setTyped] = useState("");
  const needsTyping = useLoad(removalNeedsTyping, open);
  const typing = needsTyping.data ?? true;

  async function confirm() {
    try {
      await removeSecret(serverId, secretKey);
      toast.success(`Removed ${secretKey} from ${serverName}`);
      await onRemoved();
    } catch (error) {
      toastError(`Could not remove ${secretKey}`, { description: errorText(error) });
      throw error;
    }
  }

  return (
    <ConfirmDialog
      open={open}
      onOpenChange={(next) => {
        if (!next) setTyped("");
        onOpenChange(next);
      }}
      destructive
      title={`Remove ${secretKey}?`}
      contentClassName="sm:max-w-md"
      description={
        <div className="flex flex-col gap-3 text-meta text-foreground">
          <ul className="flex list-disc flex-col gap-1 pl-5">
            <li>The value in the keychain; it cannot be shown again</li>
            <li>
              {secretKey} as a setting of {serverName}
            </li>
            <li>
              Tools of {serverName} that need it fail until you set it again
              {hasLoginCheck ? ", and its login check reports it at the next check" : ""}
            </li>
          </ul>
          {typing && (
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="plus-secret-typed">
                Type <span className="font-mono">{secretKey}</span> to confirm
              </Label>
              <Input
                id="plus-secret-typed"
                autoComplete="off"
                spellCheck={false}
                value={typed}
                onChange={(event) => setTyped(event.target.value)}
              />
            </div>
          )}
        </div>
      }
      confirmLabel="Remove secret"
      confirmDisabled={needsTyping.data === null || (typing && typed !== secretKey)}
      onConfirm={confirm}
    />
  );
}
