import { useCallback, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { TerminalCommand } from "../system/atoms";
import { disableLine, homeShort } from "./model";

export interface TerminalStep {
  title: string;
  lines: string[];
  line: string;
}

/** Claude Code owns the switch that turns one plugin off, and no Toolport command writes it
 * yet, so the app shows the exact command for the terminal (D-062) instead of running it. */
export function turnOffStep(id: string, name: string, cwd: string): TerminalStep {
  return {
    title: `Turn ${name} off in a folder`,
    line: disableLine(id, "local", cwd),
    lines: [
      cwd
        ? `Run it in ${homeShort(cwd)}: Claude Code writes enabledPlugins: ${id} = false into that folder's .claude/settings.local.json.`
        : "Run it in the folder where Claude Code starts: Claude Code writes enabledPlugins into that folder's .claude/settings.local.json.",
      "Your other keys stay as they are. Undo with the same command and enable instead of disable.",
    ],
  };
}

export function disableEverywhereStep(id: string, name: string): TerminalStep {
  return {
    title: `Disable ${name} everywhere`,
    line: disableLine(id, "user"),
    lines: [
      `This turns ${name} off in every folder that does not turn it on again, and its hooks and MCP servers stop with it.`,
      "Claude Code owns this switch, so it runs in a terminal.",
    ],
  };
}

/** The open step and the focus that goes back to the button that opened it. */
export function useTerminalStep() {
  const [step, setStep] = useState<TerminalStep | null>(null);
  const origin = useRef<HTMLElement | null>(null);
  const open = useCallback((next: TerminalStep) => {
    origin.current =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    setStep(next);
  }, []);
  const close = useCallback(() => {
    setStep(null);
    window.setTimeout(() => {
      if (origin.current?.isConnected) origin.current.focus();
    }, 0);
  }, []);
  return { step, open, close };
}

export function TerminalDialog({
  step,
  onClose,
}: {
  step: TerminalStep | null;
  onClose: () => void;
}) {
  if (!step) return null;
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent aria-describedby={undefined} className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>{step.title}</DialogTitle>
        </DialogHeader>
        {step.lines.map((line) => (
          <p key={line} className="text-sm">
            {line}
          </p>
        ))}
        <TerminalCommand
          line={step.line}
          note="Toolport has no command for this switch yet, so it shows Claude Code's own."
        />
        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            Close
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
