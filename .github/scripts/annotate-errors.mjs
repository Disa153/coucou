// Turns the errors in a build or test log into workflow annotations, so they can be read
// from the run's summary (and the API) without downloading the whole log.
import { readFileSync } from "node:fs";

const text = readFileSync(process.argv[2], "utf8").replace(/\x1b\[[0-9;]*m/g, "");
const lines = text.split(/\r?\n/);
const blocks = [];
for (let i = 0; i < lines.length && blocks.length < 10; i++) {
  const l = lines[i];
  const start = /^error(\[E\d+\])?:/.test(l) || /\.swift:\d+:\d+: error:/.test(l) || /error TS\d+/.test(l) || /^---- .* stdout ----$/.test(l) || /panicked at/.test(l);
  if (!start || /^error: (could not compile|aborting|test failed)/.test(l)) continue;
  const block = [l];
  for (let j = i + 1; j < lines.length && block.length < 25; j++) {
    if (lines[j].trim() === "" && block.length > 3) break;
    block.push(lines[j]);
    i = j;
  }
  blocks.push(block.join("\n"));
}
if (blocks.length === 0) blocks.push(lines.slice(-40).join("\n"));
const esc = (s) => s.replace(/%/g, "%25").replace(/\r/g, "").replace(/\n/g, "%0A");
for (const b of blocks) console.log(`::error title=${esc(b.split("\n")[0].slice(0, 120))}::${esc(b.slice(0, 3500))}`);
