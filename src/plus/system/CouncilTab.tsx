import { useState } from "react";
import { KeyRound, RefreshCw, Users } from "lucide-react";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import type { CouncilToolsData } from "../types";
import { AsyncView, type CtlQuery } from "../ui";
import { SetSecretDialog } from "../logins/SetSecretDialog";
import { Card, Intro, Mono, Tag, Toggle } from "./atoms";
import { rowsOf, useRead, useRegistry, useWrite } from "./hooks";
import {
  checkLabel,
  checksOf,
  planOfCouncilInstall,
  planOfCouncilUninstall,
  toolEntries,
  type Check,
} from "./model";
import { ToolList } from "./Tools";
import { WriteDialogs } from "./WriteDialogs";

export function CheckList({ checks, label }: { checks: Check[]; label: string }) {
  return (
    <ul aria-label={label} className="flex flex-col gap-1.5 text-sm">
      {checks.map((check) => (
        <li key={check.name} className="flex flex-wrap items-baseline gap-2">
          <Tag tone={check.ok ? "success" : "destructive"}>{check.ok ? "OK" : "Fix"}</Tag>
          <span>{checkLabel(check.name)}</span>
          {check.detail && check.detail !== "-" && (
            <span className="min-w-0 text-xs break-words text-muted-foreground">
              <Mono>{check.detail}</Mono>
            </span>
          )}
        </li>
      ))}
    </ul>
  );
}

/** A doctor exits 1 when a check fails but still prints the checks: that is an answer, not a
 * failure of the read. */
export function answered<T>(query: CtlQuery<T>): CtlQuery<T> {
  return query.data ? { ...query, status: "ready", error: null } : query;
}

/** Council: the multi-model server (install, key, doctor, tools). The key is written to the
 * vault with `secret set` on stdin; `council install --api-key-env` is never used. */
export function CouncilTab() {
  const registry = useRegistry();
  const rows = rowsOf(registry);
  const doctor = useRead<{ checks: unknown }>(["council", "doctor"]);
  const tools = useRead<CouncilToolsData>(["council", "tools"]);
  const [purge, setPurge] = useState(false);
  const [keyDialog, setKeyDialog] = useState(false);
  const refresh = () => doctor.reload();
  const write = useWrite(rows, refresh);
  const checks = checksOf(doctor.data);
  const installed = checks.find((check) => check.name === "registry_entry")?.ok === true;
  const keyDetail = checks.find((check) => check.name === "key_declared_secret")?.detail;
  const keyName = keyDetail && keyDetail !== "-" ? keyDetail : "OPENROUTER_API_KEY";
  const keyStored = checks.find((check) => check.name === "key_in_vault")?.ok ?? null;

  return (
    <div className="flex flex-col gap-4">
      <div className="grid gap-4 lg:grid-cols-2">
        <Card
          title="Council"
          actions={
            installed ? (
              <>
                <Button size="sm" variant="outline" onClick={() => setKeyDialog(true)}>
                  <KeyRound /> {keyStored ? "Replace key" : "Set key"}
                </Button>
                <Button
                  size="sm"
                  variant="destructive"
                  onClick={() =>
                    write.begin({
                      command: "council uninstall",
                      title: "Uninstall the council",
                      argv: ["council", "uninstall", ...(purge ? ["--purge-key"] : [])],
                      confirmLabel: "Uninstall",
                      planned: planOfCouncilUninstall(purge),
                    })
                  }
                >
                  Uninstall…
                </Button>
              </>
            ) : undefined
          }
        >
          <AsyncView query={answered(doctor)} errorTitle="Couldn't check the council">
            {() =>
              installed ? (
                <div className="flex flex-col gap-3">
                  <p className="flex items-center gap-2 text-sm">
                    <Tag tone="success">Installed</Tag> The council server is in your
                    registry.
                  </p>
                  <Toggle
                    label="Also delete the stored API key when uninstalling"
                    checked={purge}
                    onChange={setPurge}
                  />
                </div>
              ) : (
                <EmptyState
                  className="py-8"
                  icon={<Users />}
                  title="Not installed"
                  description="The council runs several models on one question and returns their answers as tools."
                  action={
                    <Button
                      size="sm"
                      onClick={() =>
                        write.begin({
                          command: "council install",
                          title: "Install the council",
                          argv: ["council", "install"],
                          confirmLabel: "Install",
                          planned: planOfCouncilInstall(),
                        })
                      }
                    >
                      Install…
                    </Button>
                  }
                />
              )
            }
          </AsyncView>
        </Card>
        <Card
          title="Doctor"
          actions={
            <Button
              size="xs"
              variant="ghost"
              aria-label="Run the council doctor again"
              onClick={doctor.reload}
            >
              <RefreshCw />
            </Button>
          }
        >
          <AsyncView
            query={answered(doctor)}
            errorTitle="Couldn't run the council doctor"
          >
            {() => <CheckList checks={checks} label="Council checks" />}
          </AsyncView>
        </Card>
      </div>
      <Card title="Tools and resources">
        <Intro>
          What the council exposes to your agents once it is enabled in a profile.
        </Intro>
        <AsyncView
          query={tools}
          errorTitle="Couldn't list the council tools"
          isEmpty={(data) => data.tools.length === 0}
          empty={
            <p className="text-sm text-muted-foreground">The council lists no tools.</p>
          }
        >
          {(data) => (
            <div className="flex flex-col gap-3">
              <ToolList tools={toolEntries(data.tools)} label="Council tools" />
              <ul aria-label="Council resources" className="flex flex-col gap-1 text-sm">
                {data.resources.map((resource) => (
                  <li key={resource.uri} className="flex flex-wrap gap-2">
                    <Mono>{resource.uri}</Mono>
                    <span className="text-muted-foreground">{resource.summary}</span>
                  </li>
                ))}
              </ul>
            </div>
          )}
        </AsyncView>
      </Card>
      <WriteDialogs write={write} />
      {keyDialog && (
        <SetSecretDialog
          server={{ id: "council", name: "Council" }}
          keys={[keyName]}
          existing={() => keyStored}
          onClose={() => setKeyDialog(false)}
          onStored={refresh}
        />
      )}
    </div>
  );
}
