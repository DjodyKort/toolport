import { useEffect, useState } from "react";
import { Copy, Loader2 } from "lucide-react";
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
import { Textarea } from "@/components/ui/textarea";
import { toast } from "sonner";
import { runCtl, type CtlResult } from "../bridge/ctl";
import { errorText } from "./useCtl";

export interface RunTarget {
  /** Shown in the title, e.g. `toolportctl doctor`. */
  label: string;
  argv: string[];
  /** A self-management tool: its arguments go over stdin as one JSON object. */
  tool?: { name: string; gated: boolean; previews: boolean; hasParams: boolean };
}

type Outcome =
  | { kind: "running" }
  | { kind: "done"; result: CtlResult }
  | { kind: "failed"; message: string };

function format(result: CtlResult): string {
  const body = result.envelope
    ? result.envelope.ok
      ? result.envelope.data
      : (result.envelope.data ?? result.envelope.error)
    : null;
  if (body === null || body === undefined)
    return result.parseError ?? result.stderr.join("\n");
  return JSON.stringify(body, null, 2);
}

/** Runs one command through the bridge and shows what came back. A read-only command runs at once; a
 * tool that changes things asks for its name first and passes `confirm`. */
export function RunDialog({
  target,
  onClose,
}: {
  target: RunTarget | null;
  onClose: () => void;
}) {
  return (
    <Dialog open={target !== null} onOpenChange={(open) => !open && onClose()}>
      {target && <RunDialogBody key={target.label} target={target} />}
    </Dialog>
  );
}

async function execute(
  target: RunTarget,
  args: string,
  extra: Record<string, unknown>,
): Promise<Outcome> {
  try {
    let stdin: string | undefined;
    if (target.tool) {
      const parsed = args.trim() ? (JSON.parse(args) as Record<string, unknown>) : {};
      stdin = JSON.stringify({ ...parsed, ...extra });
    }
    const result = await runCtl(target.argv, { stdinSecret: stdin }).result;
    return { kind: "done", result };
  } catch (error) {
    return { kind: "failed", message: errorText(error) };
  }
}

function RunDialogBody({ target }: { target: RunTarget }) {
  const { tool } = target;
  const [args, setArgs] = useState("{}");
  const [typed, setTyped] = useState("");
  // A plain read starts when the dialog opens; a tool waits for its arguments.
  const [outcome, setOutcome] = useState<Outcome | null>(
    tool ? null : { kind: "running" },
  );

  useEffect(() => {
    if (tool) return;
    let alive = true;
    void execute(target, "{}", {}).then((next) => alive && setOutcome(next));
    return () => {
      alive = false;
    };
  }, [target, tool]);

  async function run(extra: Record<string, unknown> = {}) {
    setOutcome({ kind: "running" });
    setOutcome(await execute(target, args, extra));
  }

  const output =
    outcome?.kind === "done"
      ? format(outcome.result)
      : outcome?.kind === "failed"
        ? outcome.message
        : "";
  const failed =
    outcome?.kind === "failed" ||
    (outcome?.kind === "done" && !outcome.result.envelope?.ok);

  return (
    <DialogContent className="sm:max-w-2xl">
      <DialogHeader>
        <DialogTitle className="font-mono text-base">{target.label}</DialogTitle>
        <DialogDescription>
          {tool
            ? tool.gated
              ? "This tool changes things. Check the arguments, then confirm by typing its name."
              : "This tool only reads. Add arguments if it takes any."
            : "Read-only command. Nothing is changed."}
        </DialogDescription>
      </DialogHeader>
      {tool && outcome === null && (
        <div className="flex flex-col gap-3">
          {tool.hasParams && (
            <Textarea
              aria-label="Arguments as JSON"
              value={args}
              onChange={(e) => setArgs(e.target.value)}
              className="min-h-24 font-mono text-xs"
              spellCheck={false}
            />
          )}
          {tool.gated && (
            <Input
              aria-label={`Type ${tool.name} to confirm`}
              placeholder={tool.name}
              value={typed}
              onChange={(e) => setTyped(e.target.value)}
            />
          )}
          <div className="flex flex-wrap justify-end gap-2">
            {tool.previews && (
              <Button variant="outline" onClick={() => void run({ dry_run: true })}>
                Preview
              </Button>
            )}
            <Button
              disabled={tool.gated && typed !== tool.name}
              onClick={() =>
                void run(tool.gated ? { confirm: true, dry_run: false } : {})
              }
            >
              {tool.gated ? "Run it" : "Run"}
            </Button>
          </div>
        </div>
      )}
      {outcome?.kind === "running" && (
        <p
          role="status"
          className="flex items-center gap-2 text-meta text-muted-foreground"
        >
          <Loader2 className="size-4 animate-spin" aria-hidden="true" /> Running…
        </p>
      )}
      {(outcome?.kind === "done" || outcome?.kind === "failed") && (
        <div className="flex flex-col gap-2">
          <p className="flex items-center gap-2 text-meta">
            <Badge variant={failed ? "destructive" : "success"}>
              {failed ? "Failed" : "Done"}
            </Badge>
            {outcome.kind === "done" && outcome.result.envelope?.error && (
              <span className="text-muted-foreground">
                {outcome.result.envelope.error.message}
              </span>
            )}
          </p>
          <pre
            // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex -- long output scrolls, so the keyboard has to reach it
            tabIndex={0}
            aria-label="Output"
            className="max-h-80 overflow-auto rounded-md border bg-muted/40 p-3 font-mono text-xs"
          >
            {output}
          </pre>
        </div>
      )}
      <DialogFooter>
        <Button
          variant="outline"
          disabled={output === ""}
          onClick={() => {
            void navigator.clipboard
              .writeText(output)
              .then(() => toast.success("Output copied"));
          }}
        >
          <Copy /> Copy output
        </Button>
      </DialogFooter>
    </DialogContent>
  );
}
