// Builds the Learn panel into dist/ (or, with --check, into a temp folder that is
// compared with dist/). dist/ is committed and embedded by the Rust server; the
// Rust test `the_committed_bundle_matches_its_source` recomputes SOURCE_HASH the
// same way, so a source change without a rebuild fails `cargo test`.
//
// Usage: node build.mjs [--check]

import { build } from "esbuild";
import { copyFileSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";

const root = dirname(fileURLToPath(import.meta.url));
const OUTPUTS = ["index.html", "app.js", "app.css", "SOURCE_HASH"];

/** Every file that decides the bundle, as `/`-separated paths relative to `root`,
 * sorted by path. A stray `.DS_Store` is not source. */
function sourceFiles() {
  const out = [];
  const walk = (rel) => {
    for (const e of readdirSync(join(root, rel), { withFileTypes: true })) {
      const p = `${rel}/${e.name}`;
      if (e.isDirectory()) walk(p);
      else if (e.name !== ".DS_Store") out.push(p);
    }
  };
  walk("src");
  out.push("build.mjs", "package.json", "package-lock.json", "tsconfig.json");
  return out.sort();
}

/** FNV-1a, 64 bits, as 16 lowercase hex digits. Each file is fed as
 * `path \0 content \0` with every carriage return removed from the content, so a
 * Windows checkout with CRLF line ends gives the same hash. */
function sourceHash() {
  const MASK = 0xffffffffffffffffn;
  const PRIME = 0x100000001b3n;
  let h = 0xcbf29ce484222325n;
  const feed = (bytes) => {
    for (const b of bytes) h = ((h ^ BigInt(b)) * PRIME) & MASK;
  };
  for (const path of sourceFiles()) {
    feed(Buffer.from(path, "utf8"));
    feed([0]);
    feed(readFileSync(join(root, path)).filter((b) => b !== 0x0d));
    feed([0]);
  }
  return h.toString(16).padStart(16, "0");
}

async function buildInto(outDir) {
  mkdirSync(outDir, { recursive: true });
  const common = { bundle: true, minify: true, legalComments: "none", sourcemap: false, logLevel: "warning" };
  await build({
    ...common,
    entryPoints: [join(root, "src/main.ts")],
    outfile: join(outDir, "app.js"),
    format: "iife",
    target: "es2022",
  });
  await build({
    ...common,
    entryPoints: [join(root, "src/styles.css")],
    outfile: join(outDir, "app.css"),
  });
  copyFileSync(join(root, "src/index.html"), join(outDir, "index.html"));
  writeFileSync(join(outDir, "SOURCE_HASH"), `${sourceHash()}\n`);
}

const check = process.argv.includes("--check");
if (!check) {
  await buildInto(join(root, "dist"));
  const gz = gzipSync(readFileSync(join(root, "dist/app.js")), { level: 9 }).length;
  console.log(`dist/app.js: ${(gz / 1024).toFixed(1)} KB gzipped (limit 150 KB)`);
} else {
  const tmp = mkdtempSync(join(tmpdir(), "learn-dist-"));
  try {
    await buildInto(tmp);
    const dist = join(root, "dist");
    const present = new Set(readdirSync(dist));
    const stale = [];
    for (const name of OUTPUTS) {
      let committed = null;
      try {
        committed = readFileSync(join(dist, name));
      } catch {
        // missing: reported below
      }
      if (committed === null || !committed.equals(readFileSync(join(tmp, name)))) stale.push(name);
      present.delete(name);
    }
    stale.push(...[...present].filter((n) => n !== ".DS_Store"));
    if (stale.length > 0) {
      console.error(`dist/ is out of date (${stale.join(", ")}); run \`npm run build\` and commit ui/learn/dist`);
      process.exit(1);
    }
    console.log("dist/ matches its source");
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
}
