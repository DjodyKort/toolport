import { useState } from "react";
import { Terminal } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { Input } from "@/components/ui/input";
import { commandLine } from "../allcommands/model";
import type { CompressionEnvData, CompressionRunData } from "../types/compression";
import { AsyncView, CopyButton, useCtlQuery } from "../ui";
import { ctl } from "./model";

const SENSITIVE = /(key|token|secret|password|credential)/i;

/** A launch env is the proxy's address and knobs; a value that looks like a credential is
 * never shown or copied, whatever a command printed. */
const shownValue = (key: string, value: string) =>
  SENSITIVE.test(key) ? "(hidden)" : value;

const shownLine = (line: string) => {
  const at = line.indexOf("=");
  return at > 0
    ? `${line.slice(0, at)}=${shownValue(line.slice(0, at), line.slice(at + 1))}`
    : line;
};

function TerminalLine({ line }: { line: string }) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      <code aria-label="Command line" className="min-w-0 font-mono text-xs break-all">
        {line}
      </code>
      <CopyButton text={line} label="Copy command" />
      <Button
        size="sm"
        variant="outline"
        disabled
        title="Not available yet: copy the command and run it in a terminal"
      >
        <Terminal /> Open in Terminal
      </Button>
    </div>
  );
}

function Plan({ dir }: { dir: string }) {
  const run = useCtlQuery<CompressionRunData>(
    ctl("run", "--plan", "--cwd", dir, "claude"),
  );
  const env = useCtlQuery<CompressionEnvData>(ctl("env", "--cwd", dir));
  return (
    <div className="flex flex-col gap-3">
      <AsyncView
        query={run}
        errorTitle="Couldn't plan the launch"
        context="compression run --plan"
      >
        {(plan) => {
          const set = Object.entries(plan.env.set);
          return (
            <div aria-label="Launch plan" className="flex flex-col gap-2 text-sm">
              <p className="flex flex-wrap items-center gap-2">
                <Badge variant={plan.routed ? "success" : "secondary"}>
                  {plan.routed ? "routed" : "plain"}
                </Badge>
                <span>
                  {plan.routed
                    ? `Through ${plan.provider}, preset ${plan.preset}, port ${plan.ledger.port}`
                    : `Plain launch (provider ${plan.provider})`}
                </span>
              </p>
              <p>
                Runs{" "}
                <code className="font-mono text-xs">
                  {[plan.program, ...plan.argv.slice(1)].join(" ")}
                </code>
              </p>
              {(set.length > 0 || plan.env.unset.length > 0) && (
                <ul className="list-disc pl-5 text-xs">
                  {set.map(([key, value]) => (
                    <li key={key}>
                      sets{" "}
                      <code className="font-mono">
                        {key}={shownValue(key, value)}
                      </code>
                    </li>
                  ))}
                  {plan.env.unset.map((key) => (
                    <li key={key}>
                      unsets <code className="font-mono">{key}</code>
                    </li>
                  ))}
                </ul>
              )}
              {plan.warnings.map((warning) => (
                <Callout key={String(warning)} variant="warning">
                  {String(warning)}
                </Callout>
              ))}
            </div>
          );
        }}
      </AsyncView>
      <AsyncView query={env} errorTitle="Couldn't read the env" context="compression env">
        {(data) => (
          <div className="flex flex-col gap-1">
            <p className="text-xs text-muted-foreground">
              Env for this folder, to paste into a shell (launch {data.launch}):
            </p>
            <pre
              aria-label="Env lines"
              className="overflow-auto rounded-md bg-muted p-2 font-mono text-xs"
            >
              {data.lines.map(shownLine).join("\n")}
            </pre>
            <div>
              <CopyButton text={data.lines.map(shownLine).join("\n")} label="Copy env" />
            </div>
          </div>
        )}
      </AsyncView>
    </div>
  );
}

/** What starting Claude in a folder would do under its policy, and the terminal command that
 * does it. Claude takes the terminal over, so the app only plans and copies (D-085). */
export function RunCard() {
  const [draft, setDraft] = useState("");
  const [dir, setDir] = useState<string | null>(null);
  const folder = draft.trim();
  const line = commandLine(
    folder
      ? ["compression", "run", "--cwd", folder, "--", "claude"]
      : ["compression", "run", "--", "claude"],
  );
  return (
    <section aria-label="Run Claude under this policy" className="flex flex-col gap-2">
      <h3 className="flex items-center gap-2 text-sm font-semibold">
        Run Claude under this policy <Badge variant="secondary">terminal</Badge>
      </h3>
      <div className="flex flex-col gap-3 rounded-lg border p-3">
        <p className="text-sm text-muted-foreground">
          Starting Claude replaces the process, so it runs in a terminal. The app shows
          the exact command and, for a folder, what it would do.
        </p>
        <form
          className="flex flex-wrap items-center gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            if (folder) setDir(folder);
          }}
        >
          <label htmlFor="compression-run-dir" className="sr-only">
            Folder
          </label>
          <Input
            id="compression-run-dir"
            className="max-w-sm"
            placeholder="/path/to/project"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
          />
          <Button type="submit" size="sm" variant="outline" disabled={!folder}>
            Preview this folder
          </Button>
        </form>
        <TerminalLine line={line} />
        {dir && <Plan key={dir} dir={dir} />}
      </div>
    </section>
  );
}
