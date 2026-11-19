import type { DiffLine } from "./lineDiff";

const LINE_TONE: Record<string, string> = {
  "+": "bg-success/10 text-success",
  "-": "bg-destructive/10 text-destructive",
  "@": "text-muted-foreground",
};

const FRAME =
  "max-h-80 overflow-auto rounded-md border bg-muted/30 py-1 font-mono text-xs";

/** A unified diff, line by line, with the sign of each line in words for the reader who cannot
 * tell green from red. */
export function DiffView({ diff }: { diff: string }) {
  const lines = diff.replace(/\n$/, "").split("\n");
  return (
    <pre aria-label="Differences" className={FRAME}>
      {lines.map((line, index) => {
        const sign = line[0] ?? " ";
        const header = line.startsWith("---") || line.startsWith("+++");
        return (
          <span
            key={`${index}-${line}`}
            className={`block px-3 whitespace-pre-wrap break-all ${header ? "text-muted-foreground" : (LINE_TONE[sign] ?? "")}`}
          >
            {line || " "}
          </span>
        );
      })}
    </pre>
  );
}

const SIGN_WORD = { "+": "Added", "-": "Removed", " ": "Unchanged" } as const;

/** The lines an edit changes, each with its sign as text and as a hidden word. */
export function LineDiffView({ lines }: { lines: DiffLine[] }) {
  return (
    <pre aria-label="Changes" className={FRAME}>
      {lines.map((line, index) => (
        <span
          key={`${index}-${line.sign}-${line.text}`}
          className={`block px-3 whitespace-pre-wrap break-all ${LINE_TONE[line.sign] ?? ""}`}
        >
          <span className="sr-only">{SIGN_WORD[line.sign]}: </span>
          <span aria-hidden="true">{line.sign}</span> {line.text || " "}
        </span>
      ))}
    </pre>
  );
}
