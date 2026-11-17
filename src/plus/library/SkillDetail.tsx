import { useState, type ReactNode } from "react";
import {
  AlertTriangle,
  ArrowLeft,
  CheckCircle2,
  Copy,
  MoreHorizontal,
} from "lucide-react";
import { skillsInventory } from "@/lib/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { SkeletonRows } from "@/components/ui/skeleton";
import { toastError } from "@/lib/toast";
import { useLoad } from "../servers/useLoad";
import { errorText } from "../settings/useCtl";
import { lintSkills, listLibrary, skillFiles } from "./commands";
import { Mono } from "./dialogs";
import {
  bytes,
  CLAUDE_LABEL,
  claudeState,
  count,
  fileBadge,
  FILE_COUNT_LABEL,
  FILE_ORDER,
  meter,
  plural,
} from "./model";
import { UninstallDialog } from "./writes";

function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="grid grid-cols-[7rem_minmax(0,1fr)] gap-2 py-1 text-meta @max-[480px]:grid-cols-1 @max-[480px]:gap-0">
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="min-w-0 break-words">{children}</dd>
    </div>
  );
}

export default function SkillDetail({
  name,
  onBack,
  onChanged,
}: {
  name: string;
  onBack: () => void;
  onChanged: () => void;
}) {
  const files = useLoad(() => skillFiles(name), name);
  const library = useLoad(listLibrary, "library");
  const lint = useLoad(async () => {
    try {
      return await lintSkills();
    } catch {
      return null;
    }
  }, "lint");
  const inventory = useLoad(async () => {
    try {
      return await skillsInventory(null);
    } catch {
      return null;
    }
  }, "inventory");
  const [removing, setRemoving] = useState(false);

  const entry = library.data?.skills.find((s) => s.name === name);
  const data = files.data;
  const claude = claudeState(inventory.data, name);
  const own = (lint.data?.messages ?? []).filter(
    (m) => m.name === name && m.consequence !== "not-copied",
  );
  const bar = data
    ? meter(data.description.length, data.description.limit, data.description.specLimit)
    : null;
  const sorted = data
    ? [...data.files].sort(
        (a, b) => FILE_ORDER.indexOf(a.status) - FILE_ORDER.indexOf(b.status),
      )
    : [];
  const willCopy = data ? data.files.filter((f) => f.client !== null).length : 0;
  const never = data ? data.files.filter((f) => f.status === "not-copied").length : 0;

  async function copyPath() {
    try {
      await navigator.clipboard.writeText(entry?.path ?? data?.source ?? "");
    } catch (error) {
      toastError("Could not copy", { description: errorText(error) });
    }
  }

  return (
    <div className="@container flex flex-col gap-4">
      <div className="flex flex-wrap items-center gap-3">
        <Button variant="ghost" size="sm" onClick={onBack}>
          <ArrowLeft /> Library
        </Button>
        <div className="min-w-48 flex-1">
          <h2 className="text-lg font-semibold">{name}</h2>
          <p className="text-meta text-muted-foreground">
            {data?.type === "rule" ? "Rule" : "Skill"} in your library
            {data ? ` · ${plural(data.files.length, "file")}` : ""}
          </p>
        </div>
        <Button variant="outline" size="sm" onClick={() => void copyPath()}>
          <Copy /> Copy path
        </Button>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="outline" size="icon" aria-label="More">
              <MoreHorizontal />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem variant="destructive" onSelect={() => setRemoving(true)}>
              Delete from library…
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      {files.error && !data && (
        <p
          role="alert"
          className="rounded-xl border bg-card px-4 py-3 text-meta text-destructive"
        >
          {files.error}
        </p>
      )}
      {!data && !files.error && <SkeletonRows label="Reading the skill" />}

      {data && bar && (
        <>
          <section className="rounded-xl border bg-card p-4" aria-label="Summary">
            <p className="text-sm">{entry?.description}</p>
            <div className="mt-3">
              <div
                role="img"
                aria-label={`${count(data.description.length)} characters`}
                className="relative h-1.5 w-full rounded-full bg-muted"
              >
                <div
                  className={
                    bar.over
                      ? "h-full rounded-full bg-destructive"
                      : "h-full rounded-full bg-success"
                  }
                  style={{ width: `${bar.fill}%` }}
                />
                <span
                  className="absolute -top-0.5 h-2.5 w-px bg-foreground/50"
                  style={{ left: `${bar.specTick}%` }}
                />
                <span
                  className="absolute -top-0.5 h-2.5 w-px bg-foreground/50"
                  style={{ left: `${bar.limitTick}%` }}
                />
              </div>
              <p className="mt-1 flex flex-wrap justify-between gap-2 text-meta text-muted-foreground">
                <span>{count(data.description.length)} characters</span>
                <span>
                  ticks: {count(data.description.specLimit)} Agent Skills limit,{" "}
                  {count(data.description.limit)} Claude Code limit
                </span>
              </p>
            </div>
            <dl className="mt-3 divide-y border-t pt-2">
              <Fact label="Library">
                <Mono>{entry?.path ?? data.source}</Mono>
              </Fact>
              <Fact label="Delivered to">
                {data.installed.map((i) => (
                  <span
                    key={i.dir}
                    className="mr-2 inline-flex flex-wrap items-center gap-1.5"
                  >
                    <Mono>{i.dir}</Mono>
                    <Badge variant={i.present ? "outline" : "info"}>
                      {i.present ? "Claude Code" : "Not delivered yet"}
                    </Badge>
                  </span>
                ))}
              </Fact>
              <Fact label="In Claude Code">
                {claude ? (
                  <>
                    <Badge variant={claude === "on" ? "success" : "warning"}>
                      {CLAUDE_LABEL[claude].replace("Claude: ", "")}
                    </Badge>{" "}
                    <span className="text-muted-foreground">read from Skills</span>
                  </>
                ) : (
                  <span className="text-muted-foreground">
                    Not listed by Skills yet. It appears there once a sync has delivered
                    it.
                  </span>
                )}
              </Fact>
              <Fact label="Checks">
                {own.length === 0 ? (
                  <Badge variant="success">None</Badge>
                ) : (
                  <ul className="flex flex-col gap-0.5">
                    {own.map((row, index) => (
                      <li key={`${row.code}-${index}`}>
                        <Badge
                          variant={row.level === "error" ? "destructive" : "warning"}
                        >
                          {row.level === "error" ? "Error" : "Warning"}
                        </Badge>{" "}
                        {row.message}
                      </li>
                    ))}
                  </ul>
                )}
              </Fact>
            </dl>
          </section>

          <section aria-label="Files" className="flex flex-col gap-2">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <h3 className="text-meta font-semibold tracking-wide text-muted-foreground uppercase">
                Files{" "}
                <span className="ml-1 font-normal normal-case">{data.files.length}</span>
              </h3>
              <span className="text-meta text-muted-foreground">
                {willCopy} will be copied, {never} never copied
              </span>
            </div>
            <div className="flex flex-wrap gap-1.5">
              {Object.entries(data.counts)
                .filter(([, n]) => n > 0)
                .map(([status, n]) => (
                  <Badge key={status} variant="outline">
                    {n} {FILE_COUNT_LABEL[status] ?? status}
                  </Badge>
                ))}
            </div>
            <ul className="divide-y overflow-hidden rounded-xl border bg-card">
              {sorted.map((file) => {
                const badge = fileBadge(file);
                return (
                  <li
                    key={file.path}
                    className="flex flex-wrap items-center gap-x-3 gap-y-1 px-4 py-2"
                  >
                    <code className="min-w-40 flex-1 font-mono text-xs break-all">
                      {file.path}
                    </code>
                    {file.reason && (
                      <span className="text-meta text-muted-foreground">
                        {file.reason}
                      </span>
                    )}
                    <span className="text-meta text-muted-foreground">
                      {bytes(file.size)}
                    </span>
                    <Badge variant={badge.variant}>{badge.label}</Badge>
                  </li>
                );
              })}
            </ul>
            <p className="text-meta text-muted-foreground">
              The whole folder is copied, dotfiles included. Only the list below is left
              out.
            </p>
            <div className="flex flex-wrap gap-1.5" aria-label="Never copied">
              <span className="text-meta text-muted-foreground">Never copied:</span>
              {data.denyList.map((rule) => (
                <code
                  key={rule}
                  className="rounded bg-muted px-1.5 py-0.5 font-mono text-xs"
                >
                  {rule}
                </code>
              ))}
            </div>
          </section>

          {data.links.length > 0 && (
            <section aria-label="Links" className="flex flex-col gap-2">
              <h3 className="text-meta font-semibold tracking-wide text-muted-foreground uppercase">
                Links in SKILL.md{" "}
                <span className="ml-1 font-normal normal-case">{data.links.length}</span>
              </h3>
              <ul className="divide-y overflow-hidden rounded-xl border bg-card">
                {data.links.map((link) => (
                  <li
                    key={`${link.target}-${link.line}`}
                    className="flex items-start gap-3 px-4 py-2.5"
                  >
                    {link.state === "installed" ? (
                      <CheckCircle2 aria-hidden className="mt-0.5 size-4 text-success" />
                    ) : (
                      <AlertTriangle aria-hidden className="mt-0.5 size-4 text-warning" />
                    )}
                    <div className="min-w-0">
                      <p className="font-mono text-sm break-all">{link.target}</p>
                      <p className="text-meta text-muted-foreground">
                        Line {link.line}.{" "}
                        {link.state === "installed"
                          ? "Will be installed."
                          : link.state === "outside"
                            ? "Outside this skill folder, so it is not installed. Claude would follow a dead link."
                            : "The file does not exist in your library."}
                      </p>
                    </div>
                  </li>
                ))}
              </ul>
            </section>
          )}
        </>
      )}
      {removing && (
        <UninstallDialog
          name={name}
          onClose={() => setRemoving(false)}
          onDone={() => {
            onChanged();
            onBack();
          }}
        />
      )}
    </div>
  );
}
