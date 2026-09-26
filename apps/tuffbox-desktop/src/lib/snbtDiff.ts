/** Line-based unified diff for SNBT preflight (small/medium chapter files). */

export type DiffLineKind = "same" | "add" | "del";

export interface DiffLine {
  kind: DiffLineKind;
  text: string;
}

export interface SnbtDiffFile {
  id: string;
  label: string;
  leftText: string;
  rightText: string;
  leftLabel?: string;
  rightLabel?: string;
}

function normalizeNewlines(text: string): string {
  return text.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
}

/** True when texts are identical after newline normalization. */
export function snbtTextsEqual(left: string, right: string): boolean {
  return normalizeNewlines(left) === normalizeNewlines(right);
}

/**
 * Line LCS unified diff. Falls back to whole-file +/- when matrices would be huge.
 */
export function unifiedDiffLines(oldText: string, newText: string): DiffLine[] {
  const A = normalizeNewlines(oldText).split("\n");
  const B = normalizeNewlines(newText).split("\n");
  const n = A.length;
  const m = B.length;

  if (n * m > 4_000_000) {
    const out: DiffLine[] = [];
    for (const text of A) out.push({ kind: "del", text });
    for (const text of B) out.push({ kind: "add", text });
    return out;
  }

  const dp: Uint16Array[] = Array.from({ length: n + 1 }, () => new Uint16Array(m + 1));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i][j] =
        A[i] === B[j]
          ? (dp[i + 1][j + 1] + 1) as number
          : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }

  const out: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (A[i] === B[j]) {
      out.push({ kind: "same", text: A[i]! });
      i++;
      j++;
    } else if (dp[i + 1]![j]! >= dp[i]![j + 1]!) {
      out.push({ kind: "del", text: A[i]! });
      i++;
    } else {
      out.push({ kind: "add", text: B[j]! });
      j++;
    }
  }
  while (i < n) {
    out.push({ kind: "del", text: A[i]! });
    i++;
  }
  while (j < m) {
    out.push({ kind: "add", text: B[j]! });
    j++;
  }
  return out;
}
