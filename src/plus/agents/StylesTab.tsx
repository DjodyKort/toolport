import { useState } from "react";
import { Plus, RefreshCw, Trash2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import type { StylesLsData } from "../bridge/data";
import type { StylesDiffData, StylesLintData, StylesStatusData } from "../types/styles";
import { AsyncView } from "../ui";
import { useRead, useRegistryRows, useWrite, type WriteControl } from "./hooks";
import { byClient, clientName, type ActiveRow, type NativeRow } from "./model";
import { NewDialog } from "./NewDialog";
import { Card, Chips, DiffBody, Discovery, LintBody, PathLine, Section } from "./parts";
import { useRestoreFocus } from "./useRestoreFocus";
import { WriteDialogs } from "./WriteDialogs";

type Style = StylesLsData["styles"][number];

function StyleCard({
  style,
  active,
  write,
}: {
  style: Style;
  active: StylesLsData["active"];
  write: WriteControl;
}) {
  const on = active.filter((a) => a.style === style.name).map((a) => a.client);
  return (
    <li className="flex flex-wrap items-start justify-between gap-3 rounded-lg border bg-card p-4">
      <div className="flex min-w-0 flex-col gap-1.5">
        <p className="flex flex-wrap items-center gap-2">
          <b className="text-sm">{style.name}</b>
          {style.keepCodingInstructions && (
            <Badge variant="outline">Keeps coding instructions</Badge>
          )}
          {on.length > 0 && <Badge variant="info">Active</Badge>}
        </p>
        {style.description && (
          <p className="text-sm text-muted-foreground">{style.description}</p>
        )}
        <PathLine path={style.path} />
        <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-3 gap-y-1 text-sm">
          <dt className="text-muted-foreground">Synced to</dt>
          <dd>
            <Chips
              items={[...style.clientsSynced].sort(byClient)}
              empty="Not synced yet"
            />
          </dd>
          <dt className="text-muted-foreground">Always-on in</dt>
          <dd>
            <Chips items={[...on].sort(byClient)} empty="No client" />
          </dd>
        </dl>
      </div>
      <div className="flex flex-wrap gap-2">
        <Button
          size="sm"
          variant="outline"
          disabled
          aria-label={`Edit body of ${style.name}`}
          title="Editing the body needs `mcp call styles_edit_body` (MIG-GUI-14). Open the file in your editor for now."
        >
          Edit body
        </Button>
        <Button
          size="sm"
          variant="outline"
          aria-label={`Apply ${style.name} to other clients`}
          disabled={write.busy}
          onClick={() =>
            write.begin({
              command: "styles apply",
              title: `Apply ${style.name} to the other clients`,
              argv: ["styles", "apply", style.name],
              confirmLabel: "Apply",
              phrase: style.name,
            })
          }
        >
          Apply to other clients…
        </Button>
      </div>
    </li>
  );
}

function PerClient({ data }: { data: StylesStatusData }) {
  const native = (data.native as NativeRow[])
    .slice()
    .sort((a, b) => byClient(a.client, b.client));
  const applied = (data.applyRemove as ActiveRow[])
    .slice()
    .sort((a, b) => byClient(a.client, b.client));
  if (native.length === 0 && applied.length === 0)
    return <p className="text-sm text-muted-foreground">No client has a style yet.</p>;
  return (
    <table className="w-full text-sm">
      <thead>
        <tr className="text-left text-xs text-muted-foreground">
          <th className="py-1 pr-3 font-normal">Client</th>
          <th className="py-1 pr-3 font-normal">Switchable styles</th>
          <th className="py-1 font-normal">Always-on style</th>
        </tr>
      </thead>
      <tbody>
        {native.map((row) => (
          <tr key={`n-${row.client}`} className="border-t">
            <td className="py-1.5 pr-3">{row.name || clientName(row.client)}</td>
            <td className="py-1.5 pr-3">{row.styles.join(", ") || "none"}</td>
            <td className="py-1.5 text-muted-foreground">not used</td>
          </tr>
        ))}
        {applied.map((row) => (
          <tr key={`a-${row.client}`} className="border-t">
            <td className="py-1.5 pr-3">{row.name || clientName(row.client)}</td>
            <td className="py-1.5 pr-3 text-muted-foreground">none</td>
            <td className="py-1.5">{row.active ?? "none"}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function Actions({ write, onNew }: { write: WriteControl; onNew: () => void }) {
  return (
    <div className="flex flex-wrap gap-2">
      <Button variant="outline" disabled={write.busy} onClick={onNew}>
        <Plus /> New style…
      </Button>
      <Button
        disabled={write.busy}
        onClick={() =>
          write.begin({
            command: "styles sync",
            title: "Write every style to the native clients",
            argv: ["styles", "sync"],
            confirmLabel: "Sync",
            phrase: "sync",
          })
        }
      >
        <RefreshCw /> Sync…
      </Button>
      <Button
        variant="outline"
        disabled={write.busy}
        onClick={() =>
          write.begin({
            command: "styles remove",
            title: "Remove the active style from the other clients",
            argv: ["styles", "remove"],
            confirmLabel: "Remove",
            phrase: "remove style",
          })
        }
      >
        Remove active style…
      </Button>
      <Button
        variant="outline"
        disabled={write.busy}
        onClick={() =>
          write.begin({
            command: "styles clean",
            title: "Remove every synced and applied style file",
            argv: ["styles", "clean"],
            confirmLabel: "Remove",
            phrase: "clean styles",
          })
        }
      >
        <Trash2 /> Clean style files…
      </Button>
    </div>
  );
}

function Styles({ write, onNew }: { write: WriteControl; onNew: () => void }) {
  const list = useRead<StylesLsData>(["styles", "ls"]);
  const status = useRead<StylesStatusData>(["styles", "status"]);
  const lint = useRead<StylesLintData>(["styles", "lint"]);
  const diff = useRead<StylesDiffData>(["styles", "diff"]);
  const empty = list.data !== null && list.data.styles.length === 0;
  return (
    <>
      <Section
        title="Styles"
        count={list.data?.styles.length}
        actions={
          empty || !list.data ? undefined : <Actions write={write} onNew={onNew} />
        }
      >
        <AsyncView
          query={list}
          errorTitle="Couldn't list styles"
          isEmpty={(d) => d.styles.length === 0}
          empty={
            <EmptyState
              title="No output styles yet"
              description="A style is a short instruction on how Claude should write, kept in your skills repository and sent to every client."
              action={
                <Button onClick={onNew}>
                  <Plus /> Create your first style
                </Button>
              }
            />
          }
        >
          {(data) => (
            <>
              <Discovery warnings={data.discoveryWarnings} />
              <ul className="flex flex-col gap-3">
                {data.styles.map((style) => (
                  <StyleCard
                    key={style.name}
                    style={style}
                    active={data.active}
                    write={write}
                  />
                ))}
              </ul>
            </>
          )}
        </AsyncView>
      </Section>
      {!empty && (
        <Section title="Per client">
          <Card title="Styles per client" query={status}>
            {(data) => <PerClient data={data} />}
          </Card>
          <div className="grid gap-3 lg:grid-cols-2">
            <Card title="Lint" query={lint}>
              {(data) => <LintBody data={data} noun="your styles" />}
            </Card>
            <Card title="Changes since last sync" query={diff}>
              {(data) => <DiffBody data={data} noun="style" />}
            </Card>
          </div>
        </Section>
      )}
    </>
  );
}

/** The Styles panel. It starts empty with one action, "Create your first style"; once there
 * are styles it shows where each is synced and active, and the writes that move them. */
export function StylesTab() {
  const rows = useRegistryRows();
  const [epoch, setEpoch] = useState(0);
  const [naming, setNaming] = useState(false);
  const write = useWrite(rows, () => setEpoch((n) => n + 1));
  useRestoreFocus();
  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="max-w-prose text-sm text-muted-foreground">
          Claude Code switches styles natively. Other clients get the active style as an
          always-on rule.
        </p>
      </div>
      <WriteDialogs write={write} />
      {naming && (
        <NewDialog
          kind="style"
          onClose={() => setNaming(false)}
          onSubmit={(name) => {
            setNaming(false);
            write.begin({
              command: "styles add",
              title: `Create style ${name}`,
              argv: ["styles", "add", name],
              confirmLabel: "Create",
              phrase: name,
            });
          }}
        />
      )}
      <Styles key={epoch} write={write} onNew={() => setNaming(true)} />
    </div>
  );
}
