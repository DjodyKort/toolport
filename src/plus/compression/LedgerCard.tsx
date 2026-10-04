import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type {
  CompressionLedgerProvider,
  CompressionLedgerSummaryData,
} from "../types/compression";
import { AsyncView, type CtlQuery } from "../ui";
import { Field, OptionsDialog, SELECT_CLASS } from "./forms";
import { ctl, flagArgs, PROVIDERS } from "./model";
import type { WriteControl } from "./useWrite";

const NUMBER = new Intl.NumberFormat("en-US");

const percent = (row: CompressionLedgerProvider) =>
  row.savedPercent === null ? "n/a" : `${row.savedPercent.toFixed(1)}%`;

/** Before and after per provider as two bars on one scale. A figure for screen readers
 * says the same numbers; the table below repeats them. */
export function SavingsChart({ rows }: { rows: CompressionLedgerProvider[] }) {
  const scale = Math.max(
    1,
    ...rows.map((row) => Math.max(row.tokensBefore, row.tokensAfter)),
  );
  const label = rows
    .map(
      (row) =>
        `${row.provider}: ${NUMBER.format(row.tokensBefore)} tokens before, ${NUMBER.format(row.tokensAfter)} after`,
    )
    .join("; ");
  const bar = (value: number, tone: string) => (
    <div className="h-2 rounded-full bg-muted">
      <div
        className={`h-2 rounded-full ${tone}`}
        style={{
          width: `${Math.max(value > 0 ? 2 : 0, Math.round((value / scale) * 100))}%`,
        }}
      />
    </div>
  );
  return (
    <div
      role="img"
      aria-label={`Tokens before and after compression. ${label}`}
      className="flex flex-col gap-3"
    >
      {rows.map((row) => (
        <div key={row.provider} aria-hidden="true" className="flex flex-col gap-1">
          <div className="flex justify-between text-xs">
            <b>{row.provider}</b>
            <span className="text-muted-foreground">saved {percent(row)}</span>
          </div>
          {bar(row.tokensBefore, "bg-muted-foreground/60")}
          {bar(row.tokensAfter, "bg-primary")}
        </div>
      ))}
      <div aria-hidden="true" className="flex gap-4 text-xs text-muted-foreground">
        <span>
          <i className="mr-1 inline-block size-2 rounded-full bg-muted-foreground/60" />
          before
        </span>
        <span>
          <i className="mr-1 inline-block size-2 rounded-full bg-primary" />
          after
        </span>
      </div>
    </div>
  );
}

function RecordForm({ write, onClose }: { write: WriteControl; onClose: () => void }) {
  const [provider, setProvider] = useState("rtk-only");
  const [before, setBefore] = useState("");
  const [after, setAfter] = useState("");
  const [source, setSource] = useState("");
  const [session, setSession] = useState("");
  const digits = (value: string) => /^\d+$/.test(value);
  return (
    <OptionsDialog
      title="Record savings"
      description="Adds one measured entry to the ledger. It does not change the policy."
      submitLabel="Review"
      canSubmit={digits(before) && digits(after)}
      onClose={onClose}
      onSubmit={() => {
        onClose();
        write.begin({
          command: "compression ledger record",
          title: "Record a savings entry",
          argv: ctl(
            "ledger",
            "record",
            "--provider",
            provider,
            "--before",
            before,
            "--after",
            after,
            ...flagArgs([
              { flag: "--source", value: source },
              { flag: "--session", value: session },
            ]),
          ),
          confirmLabel: "Record",
          phrase: "record",
        });
      }}
    >
      <Field label="Provider">
        {(id) => (
          <select
            id={id}
            className={SELECT_CLASS}
            value={provider}
            onChange={(e) => setProvider(e.target.value)}
          >
            {PROVIDERS.map((p) => (
              <option key={p.id} value={p.id}>
                {p.id}
              </option>
            ))}
          </select>
        )}
      </Field>
      <div className="grid gap-3 sm:grid-cols-2">
        <Field label="Tokens before">
          {(id) => (
            <Input
              id={id}
              inputMode="numeric"
              aria-invalid={before !== "" && !digits(before)}
              value={before}
              onChange={(e) => setBefore(e.target.value)}
            />
          )}
        </Field>
        <Field label="Tokens after">
          {(id) => (
            <Input
              id={id}
              inputMode="numeric"
              aria-invalid={after !== "" && !digits(after)}
              value={after}
              onChange={(e) => setAfter(e.target.value)}
            />
          )}
        </Field>
        <Field label="Source" hint="Optional.">
          {(id, hint) => (
            <Input
              id={id}
              aria-describedby={hint}
              value={source}
              onChange={(e) => setSource(e.target.value)}
            />
          )}
        </Field>
        <Field label="Session" hint="Optional.">
          {(id, hint) => (
            <Input
              id={id}
              aria-describedby={hint}
              value={session}
              onChange={(e) => setSession(e.target.value)}
            />
          )}
        </Field>
      </div>
    </OptionsDialog>
  );
}

/** What compression saved, per provider, from the launch and savings ledgers. */
export function LedgerCard({
  query,
  write,
}: {
  query: CtlQuery<CompressionLedgerSummaryData>;
  write: WriteControl;
}) {
  const [recording, setRecording] = useState(false);
  return (
    <section aria-label="Savings ledger" className="flex flex-col gap-2">
      <div className="flex items-center justify-between gap-2">
        <h3 className="text-sm font-semibold">Savings ledger</h3>
        <Button size="sm" variant="outline" onClick={() => setRecording(true)}>
          Record savings…
        </Button>
      </div>
      <AsyncView
        query={query}
        errorTitle="Couldn't read the savings ledger"
        context="compression ledger summary"
        isEmpty={(data) => data.providers.length === 0}
        empty={
          <div className="rounded-lg border px-3 py-6 text-center">
            <p className="font-medium">No launches recorded yet</p>
            <p className="mx-auto mt-1 max-w-md text-sm text-muted-foreground">
              The ledger fills when Claude is started under a compression policy.
            </p>
          </div>
        }
      >
        {(data) => (
          <div className="flex flex-col gap-3 rounded-lg border p-3">
            <p className="text-sm">
              <b>{NUMBER.format(data.tokensSaved)}</b> tokens saved in total
            </p>
            <SavingsChart rows={data.providers} />
            <table aria-label="Savings by provider" className="w-full text-xs">
              <thead>
                <tr className="text-muted-foreground">
                  {["Provider", "Launches", "Routed", "Plain", "Saved", "Saved %"].map(
                    (heading, i) => (
                      <th
                        key={heading}
                        scope="col"
                        className={`font-normal ${i === 0 ? "text-left" : "px-2 text-right"}`}
                      >
                        {heading}
                      </th>
                    ),
                  )}
                </tr>
              </thead>
              <tbody>
                {data.providers.map((row) => (
                  <tr key={row.provider} className="border-t">
                    <th scope="row" className="py-1.5 text-left font-medium">
                      {row.provider}
                    </th>
                    <td className="px-2 text-right">{row.launches}</td>
                    <td className="px-2 text-right">{row.routed}</td>
                    <td className="px-2 text-right">{row.plain}</td>
                    <td className="px-2 text-right">{NUMBER.format(row.tokensSaved)}</td>
                    <td className="px-2 text-right">{percent(row)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </AsyncView>
      {recording && <RecordForm write={write} onClose={() => setRecording(false)} />}
    </section>
  );
}
