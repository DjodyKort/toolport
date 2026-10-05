import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { errorText } from "../ui";
import type { BodyKind } from "./bodyEdit";
import { useToolRead } from "./selfTool";

interface Flat {
  transpilers: string[];
}

interface Tiers {
  tier1: string[];
  tier2: string[];
}

function Chips({ items, label }: { items: string[]; label: string }) {
  return (
    <ul aria-label={label} className="flex flex-wrap gap-1.5">
      {items.map((item) => (
        <li key={item}>
          <Badge variant="secondary">{item}</Badge>
        </li>
      ))}
    </ul>
  );
}

/** What each client gets: the converters Toolport has for agents, skills or styles, from the
 * `<kind>_list_transpilers` tool (the same list the sync writes with). */
export function ConvertedFor({ kind }: { kind: BodyKind }) {
  const read = useToolRead<Flat | Tiers>(`${kind}_list_transpilers`, {});
  const noun = { agents: "agents", skills: "skills", styles: "styles" }[kind];
  const data = read.data;
  return (
    <section
      aria-label="Converted for"
      className="flex flex-col gap-2 rounded-lg border bg-card p-3 text-sm"
    >
      <h3 className="font-medium">Converted for</h3>
      {read.status === "loading" && (
        <p role="status" className="text-muted-foreground">
          Reading the converters…
        </p>
      )}
      {read.status === "error" && (
        <p role="alert" className="text-muted-foreground">
          The converters could not be listed: {errorText(read.error).message}{" "}
          <Button size="sm" variant="ghost" onClick={read.reload}>
            Retry
          </Button>
        </p>
      )}
      {data && "transpilers" in data && (
        <>
          <p className="text-muted-foreground">
            Each client below gets its own format of the {noun}; fields it cannot hold are
            dropped and listed in the sync preview.
          </p>
          {data.transpilers.length === 0 ? (
            <p className="text-muted-foreground">No converters.</p>
          ) : (
            <Chips items={data.transpilers} label="Converters" />
          )}
        </>
      )}
      {data && "tier1" in data && (
        <div className="flex flex-col gap-2">
          <p className="text-muted-foreground">Switchable style (native toggle)</p>
          <Chips items={data.tier1} label="Native-toggle clients" />
          <p className="text-muted-foreground">Always-on rule</p>
          <Chips items={data.tier2} label="Always-on clients" />
        </div>
      )}
    </section>
  );
}
