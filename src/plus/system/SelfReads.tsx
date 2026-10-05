import { Fragment, type ReactNode } from "react";
import { ArrowLeft, ArrowLeftRight, ArrowRight, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { FlowDiagramResult, WhereAmIResult } from "../types";
import { AsyncView, CopyButton } from "../ui";
import { Card, Intro, Kv, Mono, Tag } from "./atoms";
import { useToolRead } from "./hooks";
import { plural } from "./model";
import {
  loginCounts,
  parseFlow,
  secretsBackendLabel,
  type FlowArrow,
  type FlowBlock,
  type FlowNode,
} from "./where";

function Refresh({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <Button size="xs" variant="ghost" aria-label={label} onClick={onClick}>
      <RefreshCw />
    </Button>
  );
}

function Path({ path, copy }: { path: string; copy: string }) {
  return (
    <span className="flex flex-wrap items-center gap-2">
      <Mono>{path}</Mono>
      <CopyButton text={path} label={copy} size="xs" />
    </span>
  );
}

function registryState(registry: WhereAmIResult["registry"]) {
  if (!registry.exists) return { tone: "warning", label: "Not created yet" } as const;
  if (!registry.readable) return { tone: "destructive", label: "Not readable" } as const;
  return { tone: "success", label: "Readable" } as const;
}

function Facts({ data }: { data: WhereAmIResult }) {
  const registry = registryState(data.registry);
  const running = data.gateway.builds.length;
  const logins = loginCounts(data.auth.counts);
  const rows: Array<[string, ReactNode]> = [
    ["Version", <Mono key="v">{data.version}</Mono>],
    [
      "Data folder",
      <Path key="d" path={data.dataDir} copy="Copy the data folder path" />,
    ],
    [
      "Registry",
      <span key="r" className="flex flex-col gap-1">
        <span className="flex flex-wrap items-center gap-2">
          <Tag tone={registry.tone}>{registry.label}</Tag>
          <Mono>{data.registry.path}</Mono>
        </span>
        {data.registry.error && (
          <span className="text-xs text-destructive">{data.registry.error}</span>
        )}
      </span>,
    ],
    [
      "Gateway",
      <span key="g" className="flex flex-col gap-1">
        <span className="flex flex-wrap items-center gap-2">
          <Tag tone={data.gateway.present ? "success" : "warning"}>
            {data.gateway.present ? "Installed" : "Not found"}
          </Tag>
          <Mono>{data.gateway.path}</Mono>
        </span>
        <span className="text-xs text-muted-foreground">
          {running === 0
            ? "No gateway is running"
            : `${plural(running, "gateway")} running`}
        </span>
      </span>,
    ],
    ["Secrets", secretsBackendLabel(data.secretsBackend)],
    ["Active profile", data.activeProfile],
    [
      "Registry holds",
      `${plural(data.serverCount, "server")}, ${plural(data.profileCount, "profile")}`,
    ],
    [
      "Logins",
      logins.length === 0 ? (
        "None tracked"
      ) : (
        <span key="l" className="flex flex-wrap gap-1">
          {logins.map((entry) => (
            <Tag key={entry.key} tone={entry.problem ? "warning" : "success"}>
              {entry.count} {entry.label}
            </Tag>
          ))}
        </span>
      ),
    ],
  ];
  return <Kv rows={rows} />;
}

/** Where Toolport keeps its files and what it found there: the read of the `where_am_i`
 * tool. Nothing here changes anything; Refresh asks again. */
export function WhereAmI() {
  const query = useToolRead<WhereAmIResult>("where_am_i");
  return (
    <Card
      title="Where am I"
      actions={<Refresh label="Read where am I again" onClick={query.reload} />}
    >
      <Intro>
        The folder, the files and the secret store this copy of Toolport uses.
      </Intro>
      <AsyncView
        query={query}
        errorTitle="Couldn't read where Toolport is"
        context="mcp call where_am_i"
      >
        {(data) => <Facts data={data} />}
      </AsyncView>
    </Card>
  );
}

const ARROW: Record<FlowArrow, { icon: typeof ArrowRight; words: string }> = {
  to: { icon: ArrowRight, words: "goes to" },
  both: { icon: ArrowLeftRight, words: "goes both ways with" },
  from: { icon: ArrowLeft, words: "comes from" },
};

function Box({ node }: { node: FlowNode }) {
  return (
    <span className="rounded-md border bg-muted/50 px-2 py-1 text-xs">
      <span className="font-medium">{node.label}</span>
      {node.detail && <span className="text-muted-foreground"> ({node.detail})</span>}
    </span>
  );
}

function Chain({ nodes, arrows }: { nodes: FlowNode[]; arrows: FlowArrow[] }) {
  return (
    <li className="flex flex-wrap items-center gap-1.5">
      {nodes.map((node, at) => {
        const arrow = at > 0 ? ARROW[arrows[at - 1]] : null;
        return (
          <Fragment key={at}>
            {arrow && (
              <span className="text-muted-foreground">
                <arrow.icon className="size-3.5" aria-hidden="true" />
                <span className="sr-only"> {arrow.words} </span>
              </span>
            )}
            <Box node={node} />
          </Fragment>
        );
      })}
    </li>
  );
}

function Flow({ blocks }: { blocks: FlowBlock[] }) {
  const out: ReactNode[] = [];
  let chains: Array<Extract<FlowBlock, { kind: "chain" }>> = [];
  const flush = () => {
    if (chains.length === 0) return;
    const list = chains;
    chains = [];
    out.push(
      <ul key={`c${out.length}`} aria-label="Flows" className="flex flex-col gap-2">
        {list.map((chain, at) => (
          <Chain key={at} nodes={chain.nodes} arrows={chain.arrows} />
        ))}
      </ul>,
    );
  };
  for (const block of blocks) {
    if (block.kind === "chain") {
      chains.push(block);
      continue;
    }
    flush();
    const key = `b${out.length}`;
    if (block.kind === "heading")
      out.push(
        <h4 key={key} className="text-sm font-medium">
          {block.text}
        </h4>,
      );
    else if (block.kind === "code")
      out.push(
        <pre
          key={key}
          aria-label="Diagram source"
          className="overflow-auto rounded-md bg-muted p-2 font-mono text-xs whitespace-pre"
        >
          {block.text}
        </pre>,
      );
    else out.push(<Intro key={key}>{block.text}</Intro>);
  }
  flush();
  return <div className="flex flex-col gap-3">{out}</div>;
}

/** How data moves between the parts of Toolport: the markdown of the `flow_diagram` tool drawn
 * as boxes and arrows. */
export function HowItConnects() {
  const query = useToolRead<FlowDiagramResult>("flow_diagram");
  return (
    <Card
      title="How the pieces connect"
      actions={<Refresh label="Read the data flow again" onClick={query.reload} />}
    >
      <AsyncView
        query={query}
        errorTitle="Couldn't read the data flow"
        context="mcp call flow_diagram"
        isEmpty={(data) => parseFlow(data.markdown).length === 0}
        empty={<Intro>Toolport describes no data flow.</Intro>}
      >
        {(data) => <Flow blocks={parseFlow(data.markdown)} />}
      </AsyncView>
    </Card>
  );
}
