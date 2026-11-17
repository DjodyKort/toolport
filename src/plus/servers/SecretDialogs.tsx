import { ConfirmDialog } from "@/components/ConfirmDialog";

export const REVEAL_SECONDS = 10;

interface SecretTarget {
  serverId: string;
  serverName: string;
  secretKey: string;
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
