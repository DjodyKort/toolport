import { useId, useState } from "react";
import { Play } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { SectionHeader } from "@/components/ui/section-header";
import { Textarea } from "@/components/ui/textarea";
import type { CommandsData } from "../bridge/data";
import { TierBadge } from "./Badges";
import {
  commandLine,
  hasMcpCall,
  parseToolArgs,
  planTool,
  toolCallArgv,
  toolOnlyRows,
} from "./model";
import { RunFlowView } from "./RunFlow";
import { useRunFlow, type FlowSpec } from "./useRunFlow";

export const NO_MCP_CALL =
  "This build of toolportctl has no `mcp call` command yet, so tools cannot be run from here.";

/** Calls one self-management tool that no command covers, through `toolportctl mcp call`
 * (contract section 15). Same safety rules as a command: a preview for tools that have a dry
 * run, a confirmation (typed for the destructive tier), then the apply. */
export function RunTool({ data }: { data: CommandsData }) {
  const available = hasMcpCall(data);
  const tools = toolOnlyRows(data);
  const [name, setName] = useState(tools[0]?.name ?? "");
  const [text, setText] = useState("");
  const [problem, setProblem] = useState<string | null>(null);
  const flow = useRunFlow();
  const textId = useId();
  const tool = tools.find((candidate) => candidate.name === name);

  function change(next: () => void) {
    next();
    setProblem(null);
    if (flow.spec) flow.reset();
  }

  function run() {
    if (!tool) return;
    const args = parseToolArgs(text);
    if (typeof args === "string") return setProblem(args);
    setProblem(null);
    const plan = planTool(tool, args);
    const argv = toolCallArgv(tool.name);
    const shown = JSON.stringify(args, null, 2);
    const base: Omit<FlowSpec, "mode" | "tier"> = {
      title: tool.name,
      line: commandLine(argv),
      detail:
        shown === "{}" ? undefined : (
          <pre
            aria-label="Arguments"
            className="max-h-40 overflow-auto rounded-md bg-muted p-2 font-mono text-xs whitespace-pre-wrap"
          >
            {shown}
          </pre>
        ),
      phrase: tool.name,
      confirmFirst: false,
      argv,
      previewArgv: argv,
    };
    if (plan.kind === "run") {
      flow.begin({ ...base, mode: "run", tier: "read" }, () => plan.stdin);
    } else if (plan.kind === "preview") {
      flow.begin({ ...base, mode: "preview", tier: plan.tier }, (phase) =>
        phase === "preview" ? plan.previewStdin : plan.applyStdin,
      );
    } else {
      flow.begin({ ...base, mode: "direct", tier: plan.tier }, () => plan.stdin);
    }
  }

  const plan = tool ? planTool(tool, {}) : null;
  const label =
    plan?.kind === "preview" ? "Preview changes" : plan?.kind === "run" ? "Run" : "Run…";

  return (
    <section
      aria-label="Run a tool"
      title={available ? undefined : NO_MCP_CALL}
      className="flex flex-col gap-3 rounded-xl border p-4"
    >
      <SectionHeader className="mb-0">Run a tool</SectionHeader>
      <p className="text-sm text-muted-foreground">
        Self-management tools that no command covers. They run through{" "}
        <code className="font-mono text-xs">toolportctl mcp call</code>.
      </p>
      {!available && <Callout variant="info">{NO_MCP_CALL}</Callout>}
      {tools.length === 0 ? (
        <p className="text-sm text-muted-foreground">
          Every tool has a command of its own.
        </p>
      ) : (
        <fieldset disabled={!available || flow.busy} className="flex flex-col gap-3">
          <legend className="sr-only">Run a tool</legend>
          <div className="flex flex-wrap items-center gap-2">
            <Label className="sr-only" htmlFor={`${textId}-tool`}>
              Tool
            </Label>
            <Select value={name} onValueChange={(next) => change(() => setName(next))}>
              <SelectTrigger
                id={`${textId}-tool`}
                aria-label="Tool"
                className="w-72 font-mono text-xs"
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {tools.map((candidate) => (
                  <SelectItem key={candidate.name} value={candidate.name}>
                    {candidate.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            {tool && <TierBadge tier={tool.tier} />}
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor={textId} className="text-xs">
              Arguments (a JSON object)
            </Label>
            <Textarea
              id={textId}
              rows={4}
              value={text}
              onChange={(event) => change(() => setText(event.target.value))}
              placeholder="{}"
              className="font-mono text-xs"
              spellCheck={false}
              aria-invalid={problem ? true : undefined}
            />
          </div>
          {problem && (
            <Callout variant="danger" role="alert">
              {problem}
            </Callout>
          )}
          <div>
            <Button type="button" onClick={run} disabled={!tool}>
              <Play /> {label}
            </Button>
          </div>
        </fieldset>
      )}
      <RunFlowView flow={flow} />
    </section>
  );
}
