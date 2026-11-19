import { lazy, Suspense, useCallback, useState } from "react";
import { Group, RowShell } from "./StackRows";
import { useLoad } from "../servers/useLoad";
import { listLayers, syncContext, wiringStatus } from "./commands";

const Wiring = lazy(() => import("./Wiring"));
const SyncPlan = lazy(() => import("./SyncPlan"));
const Layers = lazy(() => import("./Layers"));
const Gauge = lazy(() => import("./Gauge"));
const LaunchProfiles = lazy(() => import("./LaunchProfiles"));

const Loading = ({ what }: { what: string }) => (
  <p className="text-meta">Loading {what}…</p>
);

/** Rules & launch: what Toolport puts on this machine so Claude starts right. One finding leads
 * (the health of the launch wiring), then the one sync that deploys the layers, the layers and
 * the org file, and under Advanced the launch profiles, the checkpoint gauge and the shell
 * functions. The pipeline is `context sync`; the shell function runs it before a launch. */
export default function RulesLaunch({
  home: knownHome,
  folder,
}: {
  home: string | null;
  folder: string | null;
}) {
  const status = useLoad(wiringStatus, "wiring");
  const home = knownHome ?? status.data?.home ?? null;
  const layers = useLoad(listLayers, "layers");
  const [rules, setRules] = useState(true);
  const preview = useLoad(() => syncContext(true, rules), `sync-${rules}`);
  const [confirm, setConfirm] = useState(false);

  const reloadAll = useCallback(() => {
    status.reload();
    layers.reload();
    preview.reload();
  }, [status, layers, preview]);

  const functions = status.data?.shims.functions ?? [];
  return (
    <div className="flex flex-col gap-5">
      <Suspense fallback={<Loading what="the launch wiring" />}>
        <Wiring
          status={status}
          home={home}
          onSync={() => setConfirm(true)}
          reloadAll={reloadAll}
        />
      </Suspense>
      <Suspense fallback={<Loading what="the sync plan" />}>
        <SyncPlan
          preview={preview}
          rules={rules}
          onRules={setRules}
          home={home}
          open={confirm}
          onOpen={() => setConfirm(true)}
          onClose={() => setConfirm(false)}
          onDone={reloadAll}
        />
      </Suspense>
      <Suspense fallback={<Loading what="the layers" />}>
        <Layers
          layers={layers}
          org={status.data?.org ?? null}
          sync={preview.data}
          home={home}
          reload={reloadAll}
        />
      </Suspense>
      <Suspense fallback={<Loading what="the launch profiles" />}>
        <LaunchProfiles home={home} folder={folder} />
      </Suspense>
      <Suspense fallback={<Loading what="the checkpoint gauge" />}>
        <Gauge />
      </Suspense>
      <Group
        title="Shell functions"
        count={functions.length}
        hint={
          status.data?.setUp
            ? "Generated file, read-only"
            : "Set the launch wiring up to get them"
        }
      >
        {functions.map((fn) => (
          <RowShell
            key={fn.name}
            title={<span className="font-mono">{fn.name}</span>}
            subtitle={fn.summary}
          />
        ))}
      </Group>
    </div>
  );
}
