// The Lyrics view's pure part: LRC lines ("[01:02.35] words") read into timed
// lines, and the line that is sung at a given second.

export interface LyricLine {
  /** Seconds into the song. */
  t: number;
  text: string;
}

const STAMP = /\[(\d{1,3}):(\d{1,2})(?:[.:](\d{1,3}))?\]/g;

/** Timed lines, in order; a line with several stamps is sung at each of them. Tags like [ar:…] are skipped. */
export function parseLrc(lrc: string): LyricLine[] {
  const out: LyricLine[] = [];
  for (const raw of lrc.split(/\r?\n/)) {
    const stamps: number[] = [];
    let rest = raw;
    STAMP.lastIndex = 0;
    let m: RegExpExecArray | null;
    let end = 0;
    while ((m = STAMP.exec(raw)) && m.index === end) {
      const frac = m[3] ? Number(m[3]) / 10 ** m[3].length : 0;
      stamps.push(Number(m[1]) * 60 + Number(m[2]) + frac);
      end = STAMP.lastIndex;
    }
    if (!stamps.length) continue;
    rest = raw.slice(end).trim();
    for (const t of stamps) out.push({ t, text: rest });
  }
  return out.sort((a, b) => a.t - b.t);
}

/** Plain words as lines, without empty runs at the ends. */
export function plainLines(plain: string): string[] {
  const lines = plain.split(/\r?\n/).map((l) => l.trim());
  while (lines.length && !lines[0]) lines.shift();
  while (lines.length && !lines[lines.length - 1]) lines.pop();
  return lines;
}

/** The line sung at `seconds` (-1 before the first). */
export function lineAt(lines: readonly LyricLine[], seconds: number): number {
  let lo = 0;
  let hi = lines.length - 1;
  let found = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (lines[mid].t <= seconds) {
      found = mid;
      lo = mid + 1;
    } else hi = mid - 1;
  }
  return found;
}
