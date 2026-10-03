import { useEffect, useState } from "react";
import { plusFolderProfiles, plusSetFolderProfiles } from "./api";
import type { FolderProfiles as FolderProfilesData } from "./api";

export function FolderProfiles({
  data,
  onToggle,
}: {
  data: FolderProfilesData;
  onToggle: (enabled: boolean) => void;
}) {
  return (
    <section aria-label="Folder profiles" className="flex flex-col gap-2">
      <header className="flex items-baseline gap-2">
        <h3 className="text-sm font-medium">Folder profiles</h3>
        <span className="text-xs text-muted-foreground">
          {data.mappings.length} mapping(s)
        </span>
        <label className="ml-auto flex items-center gap-2 text-xs">
          <input
            type="checkbox"
            checked={data.enabled}
            onChange={(e) => onToggle(e.target.checked)}
          />
          Enabled
        </label>
      </header>
      {!data.enabled && (
        <p className="text-xs text-muted-foreground">
          Off: the gateway ignores folder mappings and every client keeps its own profile.
        </p>
      )}
      <ul aria-label="Active profile per folder" className="divide-y rounded-md border">
        {data.folders.map((f) => (
          <li
            key={f.root}
            data-applies={f.applies}
            className="flex flex-col gap-0.5 px-3 py-1.5"
          >
            <div className="flex items-center gap-3">
              <span className="font-mono text-sm">{f.root}</span>
              <span className="text-sm">
                {f.profile ?? (f.wouldApply ? `${f.wouldApply} (inactive)` : "default")}
              </span>
              <span className="ml-auto text-xs tabular-nums">~{f.tokens} tokens</span>
            </div>
            <span className="text-xs text-muted-foreground">{f.reason}</span>
          </li>
        ))}
      </ul>
    </section>
  );
}

export function FolderProfilesSection() {
  const [data, setData] = useState<FolderProfilesData | null>(null);
  useEffect(() => {
    let alive = true;
    plusFolderProfiles()
      .then((d) => alive && setData(d))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, []);
  if (!data) return null;
  const toggle = (enabled: boolean) => {
    plusSetFolderProfiles(enabled)
      .then(() => plusFolderProfiles())
      .then(setData)
      .catch(() => {});
  };
  return <FolderProfiles data={data} onToggle={toggle} />;
}
