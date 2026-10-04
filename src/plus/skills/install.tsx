import { useState } from "react";
import { Download, Search } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { Input } from "@/components/ui/input";
import type { SkillsSearchData } from "../types/skills";
import { AsyncView, CopyButton } from "../ui";
import { commandLine } from "../allcommands/model";
import { useRead, type WriteControl } from "./hooks";
import { installSpecOf } from "./installWrite";
import { installSpec, plural, specProblem } from "./model";
import { PathField } from "./fields";
import { Notes, Section } from "./parts";

function Results({
  query,
  target,
  write,
}: {
  query: string;
  target: string;
  write: WriteControl;
}) {
  const search = useRead<SkillsSearchData>(["skills", "search", query]);
  return (
    <AsyncView query={search} errorTitle="Couldn't search the taps">
      {(data) => (
        <div className="flex flex-col gap-2">
          <Notes items={data.discoveryWarnings.map(String)} />
          {data.results.length === 0 ? (
            <EmptyState
              icon={<Search />}
              title={`Nothing matches “${data.query}”`}
              description={
                data.tapCount === 0
                  ? "You have no taps yet. Add one on the Taps tab, then search again."
                  : `Toolport searched ${plural(data.tapCount, "tap")} by name, description and tags.`
              }
            />
          ) : (
            <ul aria-label="Search results" className="flex flex-col gap-2">
              {data.results.map((hit) => {
                const spec = installSpec(hit);
                return (
                  <li
                    key={`${hit.tap}/${hit.name}`}
                    className="flex flex-wrap items-center justify-between gap-3 rounded-lg border bg-card p-3 text-sm"
                  >
                    <span className="flex min-w-0 flex-col gap-1">
                      <b className="flex items-center gap-2">
                        {hit.name}
                        <Badge variant="outline">{hit.type}</Badge>
                        <Badge variant="secondary">{hit.tap}</Badge>
                      </b>
                      <span className="text-muted-foreground">{hit.description}</span>
                    </span>
                    <Button
                      size="sm"
                      aria-label={`Install ${hit.name}`}
                      disabled={write.busy || !spec}
                      title={
                        spec
                          ? undefined
                          : "This tap is not a GitHub repository. Type its @user/repo/skill spec below."
                      }
                      onClick={() =>
                        spec && write.begin(installSpecOf(spec, target, false))
                      }
                    >
                      <Download /> Install…
                    </Button>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      )}
    </AsyncView>
  );
}

/** Search the taps, then install a skill; or install from a typed spec. */
export function InstallPanel({ write }: { write: WriteControl }) {
  const [text, setText] = useState("");
  const [query, setQuery] = useState<string | null>(null);
  const [spec, setSpec] = useState("");
  const [target, setTarget] = useState("");
  const [touched, setTouched] = useState(false);
  const problem = specProblem(spec.trim());
  const line = commandLine(
    installSpecOf(spec.trim() || "@user/repo", target, false).argv,
  );
  return (
    <div className="flex flex-col gap-6">
      <Section title="Search the taps">
        <form
          className="flex gap-2"
          role="search"
          onSubmit={(event) => {
            event.preventDefault();
            if (text.trim()) setQuery(text.trim());
          }}
        >
          <Input
            type="search"
            aria-label="Search the taps"
            value={text}
            onChange={(event) => setText(event.target.value)}
            placeholder="Name, description or tag"
            autoComplete="off"
            spellCheck={false}
          />
          <Button type="submit" disabled={!text.trim()}>
            <Search /> Search
          </Button>
        </form>
        {query !== null && (
          <Results key={query} query={query} target={target} write={write} />
        )}
      </Section>
      <Section title="Install from a spec">
        <form
          className="flex flex-col gap-3"
          onSubmit={(event) => {
            event.preventDefault();
            setTouched(true);
            if (!problem) write.begin(installSpecOf(spec.trim(), target, false));
          }}
        >
          <label className="flex flex-col gap-1.5 text-sm">
            Spec
            <Input
              value={spec}
              onChange={(event) => setSpec(event.target.value)}
              placeholder="@user/repo/skill"
              autoComplete="off"
              spellCheck={false}
              aria-invalid={touched && problem ? true : undefined}
            />
          </label>
          <p role="status" className="min-h-4 text-xs text-destructive">
            {touched && problem ? problem : ""}
          </p>
          <PathField
            label="Install into"
            kind="folder"
            value={target}
            onChange={setTarget}
            placeholder="Empty: your skills repository"
          />
          <div className="flex flex-wrap items-center gap-2">
            <Button type="submit" disabled={write.busy}>
              <Download /> Preview install
            </Button>
            <CopyButton text={line} label="Copy command" />
            <code className="font-mono text-xs break-all text-muted-foreground">
              {line}
            </code>
          </div>
        </form>
        <p className="text-xs text-muted-foreground">
          Every install runs the audit first. A high-severity finding blocks it.
          Installing from a tap that is not added yet adds the tap and needs the network.
        </p>
      </Section>
    </div>
  );
}
