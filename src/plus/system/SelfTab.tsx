import { useState } from "react";
import { Bot, RefreshCw } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import type { McpToolsData } from "../types";
import { AsyncView, CopyButton } from "../ui";
import { Card, Field, Intro, Kv, Mono, Tag } from "./atoms";
import { CheckList, answered } from "./CouncilTab";
import { rowsOf, useRead, useRegistry, useWrite } from "./hooks";
import {
  SELF_ID,
  SELF_STATE,
  checksOf,
  clientSnippet,
  optionFlag,
  planOfMcpInstall,
  planOfMcpUninstall,
  plural,
  profileStates,
  toolEntries,
} from "./model";
import { ToolList } from "./Tools";
import { WriteDialogs } from "./WriteDialogs";

interface DoctorData {
  state: string;
  activeProfile: unknown;
  clientProfiles: unknown;
  checks: unknown;
}

const TIERS: Array<[string, string]> = [
  ["Read-only", "lists and checks, no confirmation"],
  ["Write", "changes something small; a dry run first where the tool has one"],
  ["Confirm", "preview first, then confirm"],
  ["Destructive", "preview, then typed confirmation in the app"],
];

/** Self-management MCP: the server that lets an agent manage Toolport (install, state,
 * what it exposes and how a client connects to it). */
export function SelfTab() {
  const registry = useRegistry();
  const rows = rowsOf(registry);
  const doctor = useRead<DoctorData>(["mcp", "doctor"]);
  const tools = useRead<McpToolsData>(["mcp", "tools"]);
  const [profile, setProfile] = useState("");
  const write = useWrite(rows, doctor.reload);
  const data = doctor.data;
  const state = data?.state ?? "";
  const installed = state !== "" && state !== "missing" && state !== "opted-out";
  const info = SELF_STATE[state] ?? {
    label: state || "Unknown",
    tone: "secondary" as const,
  };
  const checks = checksOf(data);
  const command = checks.find((check) => check.name === "binary_present")?.detail ?? "";
  const profiles = [
    ...profileStates(data?.activeProfile),
    ...profileStates(data?.clientProfiles),
  ].filter((entry, at, all) => all.findIndex((other) => other.id === entry.id) === at);

  return (
    <div className="flex flex-col gap-4">
      <div className="grid gap-4 lg:grid-cols-2">
        <Card
          title="Self-management MCP"
          actions={
            <Button
              size="xs"
              variant="ghost"
              aria-label="Run the self-management doctor again"
              onClick={doctor.reload}
            >
              <RefreshCw />
            </Button>
          }
        >
          {doctor.status === "error" && !doctor.data ? (
            <p className="text-sm text-muted-foreground">
              The state is unknown until the doctor answers.
            </p>
          ) : (
            <AsyncView
              query={answered(doctor)}
              errorTitle="Couldn't check the self-management server"
            >
              {() => (
                <div className="flex flex-col gap-3">
                  {state === "missing" && (
                    <EmptyState
                      className="py-6"
                      icon={<Bot />}
                      title="Not installed"
                      description="This server lets an agent list, check and change Toolport's own setup, with a preview and a confirmation for every change."
                    />
                  )}
                  <Kv
                    rows={[
                      [
                        "State",
                        <Tag key="s" tone={info.tone}>
                          {info.label}
                        </Tag>,
                      ],
                      [
                        "Catalogue",
                        tools.data
                          ? `${plural(tools.data.tools.length, "tool")}, ${plural(tools.data.resources.length, "resource")}`
                          : "Loading",
                      ],
                      [
                        "Enabled in",
                        profiles.length === 0 ? (
                          "no profile"
                        ) : (
                          <span key="p" className="flex flex-wrap gap-1">
                            {profiles.map((entry) => (
                              <Badge
                                key={entry.id}
                                variant={entry.enabled ? "success" : "secondary"}
                              >
                                {entry.id}
                                {entry.optedOut ? " (turned off)" : ""}
                              </Badge>
                            ))}
                          </span>
                        ),
                      ],
                    ]}
                  />
                  <div className="flex flex-wrap items-end gap-2">
                    <div className="min-w-40 flex-1">
                      <Field
                        label="Profile (optional)"
                        value={profile}
                        onChange={setProfile}
                        placeholder="the active profile"
                      />
                    </div>
                    <Button
                      size="sm"
                      onClick={() =>
                        write.begin({
                          command: "mcp install",
                          title: installed
                            ? "Enable the self-management server"
                            : "Install the self-management server",
                          argv: ["mcp", "install", ...optionFlag("--profile", profile)],
                          confirmLabel: "Install",
                          done: "Self-management server installed",
                          planned: planOfMcpInstall(profile.trim()),
                        })
                      }
                    >
                      {installed ? "Enable in a profile…" : "Install…"}
                    </Button>
                    {installed && (
                      <Button
                        size="sm"
                        variant="destructive"
                        onClick={() =>
                          write.begin({
                            command: "mcp uninstall",
                            title: "Turn off the self-management server",
                            argv: ["mcp", "uninstall"],
                            confirmLabel: "Turn off",
                            done: "Self-management server turned off",
                            planned: planOfMcpUninstall(),
                          })
                        }
                      >
                        Turn off…
                      </Button>
                    )}
                  </div>
                </div>
              )}
            </AsyncView>
          )}
        </Card>
        <Card title="Doctor">
          <AsyncView query={answered(doctor)} errorTitle="Couldn't run the doctor">
            {() => <CheckList checks={checks} label="Self-management checks" />}
          </AsyncView>
        </Card>
      </div>

      <div className="grid gap-4 lg:grid-cols-2">
        <Card title="Connect a client">
          <Intro>
            A client that follows a Toolport profile gets this server once it is enabled
            in that profile. For any other client, add it by hand.
          </Intro>
          {command ? (
            <div className="flex flex-col gap-2">
              <p className="text-xs text-muted-foreground">
                Add this to the client's MCP settings:
              </p>
              <pre
                aria-label="Client configuration"
                className="overflow-auto rounded-md bg-muted p-2 font-mono text-xs whitespace-pre-wrap"
              >
                {clientSnippet(command)}
              </pre>
              <div className="flex flex-wrap items-center gap-2">
                <CopyButton text={clientSnippet(command)} label="Copy configuration" />
                <Mono>{`claude mcp add ${SELF_ID} -- ${command}`}</Mono>
                <CopyButton
                  text={`claude mcp add ${SELF_ID} -- ${command}`}
                  label="Copy command"
                />
              </div>
            </div>
          ) : (
            <p className="text-sm text-muted-foreground">
              Install the server first; its command is shown here once the doctor finds
              it.
            </p>
          )}
        </Card>
        <Card title="Tiers">
          <Kv rows={TIERS} />
          <Intro>
            Confirmation is separate from the tier: a tool says whether it needs one every
            time or only when it is not a dry run.
          </Intro>
        </Card>
      </div>

      <Card title="Tool catalogue">
        <AsyncView
          query={tools}
          errorTitle="Couldn't list the tools"
          isEmpty={(value) => value.tools.length === 0}
          empty={
            <p className="text-sm text-muted-foreground">The server lists no tools.</p>
          }
        >
          {(value) => (
            <div className="flex flex-col gap-4">
              <ToolList tools={toolEntries(value.tools)} label="Tools" filterable />
              <div className="flex flex-col gap-1">
                <h4 className="text-sm font-medium">
                  Resources ({value.resources.length})
                </h4>
                <ul aria-label="Resources" className="flex flex-col gap-1 text-sm">
                  {value.resources.map((resource) => (
                    <li key={resource.uri} className="flex flex-wrap gap-2">
                      <Mono>{resource.uri}</Mono>
                      <span className="text-muted-foreground">
                        {resource.description}
                      </span>
                    </li>
                  ))}
                </ul>
              </div>
            </div>
          )}
        </AsyncView>
      </Card>
      <WriteDialogs write={write} />
    </div>
  );
}
