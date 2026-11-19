export interface DiffLine {
  sign: " " | "+" | "-";
  text: string;
}

export interface LineDiff {
  lines: DiffLine[];
  added: number;
  removed: number;
  /** True when the changed middle was too large to align and is shown as one removed block and
   * one added block. */
  coarse: boolean;
}

/** Above this many cells the line alignment is skipped: a body that long is rewritten, not
 * edited, and the table would cost more memory than the dialog is worth. */
const MAX_CELLS = 4_000_000;

const CONTEXT = 3;

/** The lines of a body without its trailing blank lines, which the writer normalises. */
function split(text: string): string[] {
  const trimmed = text.trimEnd();
  return trimmed === "" ? [] : trimmed.split("\n");
}

/** The lines of `after` against `before`, with the unchanged lines far from a change left out. */
export function lineDiff(before: string, after: string): LineDiff {
  const a = split(before);
  const b = split(after);
  let head = 0;
  while (head < a.length && head < b.length && a[head] === b[head]) head += 1;
  let tail = 0;
  while (
    tail < a.length - head &&
    tail < b.length - head &&
    a[a.length - 1 - tail] === b[b.length - 1 - tail]
  )
    tail += 1;
  const midA = a.slice(head, a.length - tail);
  const midB = b.slice(head, b.length - tail);

  const middle: DiffLine[] = [];
  const coarse = midA.length * midB.length > MAX_CELLS;
  if (coarse) {
    for (const text of midA) middle.push({ sign: "-", text });
    for (const text of midB) middle.push({ sign: "+", text });
  } else {
    const rows = midA.length + 1;
    const cols = midB.length + 1;
    const table = new Uint32Array(rows * cols);
    for (let i = midA.length - 1; i >= 0; i -= 1) {
      for (let j = midB.length - 1; j >= 0; j -= 1) {
        table[i * cols + j] =
          midA[i] === midB[j]
            ? table[(i + 1) * cols + j + 1] + 1
            : Math.max(table[(i + 1) * cols + j], table[i * cols + j + 1]);
      }
    }
    let i = 0;
    let j = 0;
    while (i < midA.length && j < midB.length) {
      if (midA[i] === midB[j]) {
        middle.push({ sign: " ", text: midA[i] });
        i += 1;
        j += 1;
      } else if (table[(i + 1) * cols + j] >= table[i * cols + j + 1]) {
        middle.push({ sign: "-", text: midA[i] });
        i += 1;
      } else {
        middle.push({ sign: "+", text: midB[j] });
        j += 1;
      }
    }
    for (; i < midA.length; i += 1) middle.push({ sign: "-", text: midA[i] });
    for (; j < midB.length; j += 1) middle.push({ sign: "+", text: midB[j] });
  }

  const before3 = a.slice(Math.max(0, head - CONTEXT), head);
  const after3 = a.slice(a.length - tail, a.length - tail + CONTEXT);
  const lines: DiffLine[] = [
    ...before3.map((text): DiffLine => ({ sign: " ", text })),
    ...middle,
    ...after3.map((text): DiffLine => ({ sign: " ", text })),
  ];
  return {
    lines,
    added: middle.filter((l) => l.sign === "+").length,
    removed: middle.filter((l) => l.sign === "-").length,
    coarse,
  };
}
