#!/usr/bin/env node
// Regenerate the three embedded string constants in worker.ts from their
// source files on disk. Run this after editing install.sh, page.html or
// llms.txt. check-sync.sh validates the result.
//
// Usage:  node site/sync.mjs
//
// Design: keep the escaping in one place. The previous workflow required
// hand-editing the template literal, which bit us once already
// (commit bff35e9: "escape backticks in LLMS_TXT to restore getpurple.sh").

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const workerPath = join(here, "worker.ts");

const sources = [
    { constant: "INSTALL_SCRIPT", file: join(here, "install.sh") },
    { constant: "LANDING_PAGE", file: join(here, "page.html") },
    { constant: "LLMS_TXT", file: join(here, "..", "llms.txt") },
];

function escapeForTemplate(raw) {
    // Order matters: backslash first, otherwise we'd double-escape the
    // escapes we insert ourselves.
    return raw
        .replace(/\\/g, "\\\\")
        .replace(/`/g, "\\`")
        .replace(/\$\{/g, "\\${");
}

function replaceConstant(worker, constant, body) {
    const startMarker = `const ${constant} = \``;
    const start = worker.indexOf(startMarker);
    if (start === -1) {
        throw new Error(`${constant} not found in worker.ts`);
    }
    const bodyStart = start + startMarker.length;
    const endMarker = "`;";
    const end = worker.indexOf(endMarker, bodyStart);
    if (end === -1) {
        throw new Error(`closing backtick for ${constant} not found`);
    }
    return worker.slice(0, bodyStart) + body + worker.slice(end);
}

let worker = readFileSync(workerPath, "utf8");

for (const { constant, file } of sources) {
    const raw = readFileSync(file, "utf8");
    const escaped = escapeForTemplate(raw);
    worker = replaceConstant(worker, constant, escaped);
    process.stdout.write(`  ${constant} <- ${file}\n`);
}

writeFileSync(workerPath, worker);
process.stdout.write("worker.ts regenerated.\n");
