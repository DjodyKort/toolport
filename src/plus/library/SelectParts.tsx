import { Badge } from "@/components/ui/badge";
import type { SelectItem, SelectOffer, SelectPolicy } from "./selectData";
import { isOn, itemRef, type Staged } from "./selectModel";

export function ItemPill({
  item,
  staged,
  hooksOff,
}: {
  item: SelectItem;
  staged: Staged;
  hooksOff: boolean;
}) {
  const on = isOn(item, staged);
  if (item.state === "removed" && on) return <Badge variant="destructive">removed</Badge>;
  if (item.kind === "hook" && on && hooksOff) {
    return <Badge variant="warning">off by setting</Badge>;
  }
  if (item.pending && staged[itemRef(item)] !== "tick") {
    return <Badge variant="warning">tick</Badge>;
  }
  if (on)
    return <Badge variant="success">{item.kind === "hook" ? "approved" : "on"}</Badge>;
  if (item.state === "new") return <Badge variant="partial">new</Badge>;
  return <Badge variant="off">off</Badge>;
}

export function Banner({
  tone,
  children,
}: {
  tone: "warning" | "destructive" | "info";
  children: React.ReactNode;
}) {
  const color =
    tone === "destructive"
      ? "border-destructive/40 bg-destructive/5"
      : tone === "warning"
        ? "border-warning/40 bg-warning/5"
        : "bg-muted/40";
  return (
    <div role="status" className={`rounded-lg border px-3 py-2 text-meta ${color}`}>
      {children}
    </div>
  );
}

export function PolicyBanners({ policy }: { policy: SelectPolicy }) {
  return (
    <>
      {policy.blocked && (
        <Banner tone="destructive">
          Blocked by policy: {policy.blocked} does not allow the Toolport marketplace, so
          choosing items is off for this source.
        </Banner>
      )}
      {policy.hooksOff && (
        <Banner tone="warning">
          Hooks are off by {policy.hooksOff} (<code>disableAllHooks</code>). Ticked hooks
          are kept but will not run.
        </Banner>
      )}
    </>
  );
}

export function OfferBanner({
  offer,
  checked,
  onChange,
}: {
  offer: SelectOffer;
  checked: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <Banner tone="info">
      <label className="flex items-start gap-2">
        <input
          type="checkbox"
          className="mt-0.5"
          checked={checked}
          onChange={(event) => onChange(event.target.checked)}
        />
        <span>
          <b>Installed whole</b> in Claude Code as <code>{offer.whole}</code>.{" "}
          <span className="font-medium">Switch to chosen items</span>: turn the whole
          plugin off when you apply, so only the chosen items load. Never automatic.
        </span>
      </label>
    </Banner>
  );
}

export function CliFooter({ lines }: { lines: string[] }) {
  return (
    <section aria-label="From a terminal" className="flex flex-col gap-1.5">
      <h3 className="text-meta font-semibold tracking-wide text-muted-foreground uppercase">
        Same from a terminal
      </h3>
      <pre className="overflow-x-auto rounded-lg bg-muted px-3 py-2 font-mono text-xs leading-relaxed whitespace-pre-wrap break-all">
        {lines.join("\n")}
      </pre>
    </section>
  );
}
