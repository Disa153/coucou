// Writes a channel's update.json, signed with the owner's Ed25519 key.
// Coucou (windows/src-tauri/src/updater.rs) checks this signature against the
// public key it ships with, and the installer against `sha256`.
//
//   COUCOU_UPDATE_KEY=<base64 PKCS#8> node sign-update.mjs \
//     --channel beta --build 57 --version 0.5.0 \
//     --url https://github.com/Disa153/coucou/releases/download/beta-57/Coucou-Windows-setup.exe \
//     --file windows/release/Coucou-Windows-setup.exe [--notes "…"] --out update.json
//
// `--sha256` may be given instead of `--file`. The signed text must stay the
// same as `signed_text` in updater.rs.

import { createHash, createPrivateKey, createPublicKey, sign, verify } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";

const args = {};
for (let i = 2; i < process.argv.length; i += 2) args[process.argv[i].replace(/^--/, "")] = process.argv[i + 1];

const need = (k) => {
  if (!args[k]) {
    console.error(`missing --${k}`);
    process.exit(2);
  }
  return args[k];
};

const keyB64 = process.env.COUCOU_UPDATE_KEY?.trim();
if (!keyB64) {
  console.error("COUCOU_UPDATE_KEY is not set");
  process.exit(2);
}
const key = createPrivateKey({ key: Buffer.from(keyB64, "base64"), format: "der", type: "pkcs8" });

const channel = need("channel");
if (channel !== "beta" && channel !== "stable") throw new Error(`unknown channel ${channel}`);
const build = Number(need("build"));
if (!Number.isInteger(build) || build <= 0) throw new Error(`bad build ${args.build}`);
const version = need("version");
const url = need("url");
if (!url.startsWith("https://github.com/Disa153/coucou/releases/download/")) throw new Error(`bad url ${url}`);
const sha256 = (args.sha256 ?? createHash("sha256").update(readFileSync(need("file"))).digest("hex")).toLowerCase();

const text = `coucou-update\n${channel}\n${build}\n${version}\n${url}\n${sha256}`;
const signature = sign(null, Buffer.from(text), key);
// The key can check its own signature: a broken secret fails here, not on every PC.
if (!verify(null, Buffer.from(text), createPublicKey(key), signature)) throw new Error("signature does not verify");

const manifest = { channel, build, version, notes: args.notes ?? "", url, sha256, signature: signature.toString("base64") };
writeFileSync(need("out"), JSON.stringify(manifest, null, 2) + "\n");
const pub = createPublicKey(key).export({ format: "der", type: "spki" }).subarray(-32).toString("base64");
console.log(`signed ${channel} build ${build} (${sha256.slice(0, 12)}…) with key ${pub}`);
