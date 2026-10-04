import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { JobProgress, outcomeOf, type CtlJobControl } from "../ui";
import type { AuthLoginData } from "../types/auth";
import type { LoginRow } from "./model";

export function loginArgv(server: string, noOpen: boolean): string[] {
  return ["auth", "login", server, ...(noOpen ? ["--no-open"] : [])];
}

/** `auth login`: the browser opens (or, with `--no-open`, only the address is printed), the
 * address is shown with a Copy button, and the dialog says how it ended. The run belongs to
 * the tab, so closing the dialog while it waits is Cancel and Escape never stops it. */
export function SignInDialog({
  row,
  job,
  noOpen,
  onNoBrowser,
  onClose,
  onSetSecret,
}: {
  row: LoginRow;
  job: CtlJobControl;
  noOpen: boolean;
  /** Starts the sign-in again without opening a browser. */
  onNoBrowser: () => void;
  onClose: () => void;
  onSetSecret?: () => void;
}) {
  const { state } = job;
  const running = state.phase === "running";
  const outcome = outcomeOf(state);
  const unsupported = outcome?.kind === "error" && outcome.code === "unsupported";
  return (
    <Dialog open onOpenChange={(open) => !open && !running && onClose()}>
      <DialogContent
        className="sm:max-w-lg"
        showCloseButton={!running}
        onEscapeKeyDown={(event) => running && event.preventDefault()}
        onInteractOutside={(event) => event.preventDefault()}
      >
        <DialogHeader>
          <DialogTitle>Sign in to {row.name}</DialogTitle>
          <DialogDescription>
            {noOpen
              ? "Open this address in a browser to approve access. Toolport waits for you."
              : "Your browser opens so you can approve access. If it does not open, use this address."}
          </DialogDescription>
        </DialogHeader>
        <JobProgress
          state={state}
          onCancel={() => void job.cancel()}
          title="Waiting for the browser"
          doneLabel="Signed in"
          renderResult={(data) => {
            const login = data as Partial<AuthLoginData>;
            return (
              <div className="flex flex-col gap-1.5">
                <p>{login.message ?? `Signed in to ${row.name}.`}</p>
                {typeof login.consentUrl === "string" && (
                  <code className="rounded bg-muted px-1.5 py-0.5 font-mono text-xs break-all">
                    {login.consentUrl}
                  </code>
                )}
              </div>
            );
          }}
        />
        {unsupported && (
          <p className="text-sm text-muted-foreground">
            This server signs in with a token, not in a browser.
          </p>
        )}
        <DialogFooter>
          {running ? (
            !noOpen && (
              <Button variant="outline" onClick={onNoBrowser}>
                Don't open a browser
              </Button>
            )
          ) : (
            <>
              {unsupported && onSetSecret && (
                <Button variant="outline" onClick={onSetSecret}>
                  Set secret…
                </Button>
              )}
              <Button onClick={onClose}>Close</Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
