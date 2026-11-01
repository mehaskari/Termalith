// Appends the frontend (npm) packages and their license texts to the
// third-party notices. Input: `pnpm licenses list --prod --json` on stdin.
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

const input = JSON.parse(readFileSync(0, "utf8"));
const packages = Object.values(input)
  .flat()
  .map((p) => ({ ...p, version: p.versions.join(", "), path: p.paths[0] }))
  .sort((a, b) => a.name.localeCompare(b.name));

/** All license files (LICENSE, LICENSE-MIT, LICENSE-APACHE-2.0, COPYING…). */
function licenseText(dir) {
  const files = readdirSync(dir)
    .filter((f) => /^(licen[cs]e|copying)([-_.].*)?$/i.test(f) && !/\.spdx$/i.test(f))
    .sort();
  return files.length ? files.map((f) => readFileSync(join(dir, f), "utf8").trim()).join("\n\n---\n\n") : null;
}

const out = ["", "## Frontend packages", "", "| Package | Version | License |", "|---|---|---|"];
for (const p of packages) out.push(`| \`${p.name}\` | ${p.version} | ${p.license} |`);
out.push("");
for (const p of packages) {
  const text = licenseText(p.path);
  out.push(`### ${p.name} ${p.version}`, "", "```text", text ?? `${p.license} (no license file in the package)`, "```", "");
}
process.stdout.write(out.join("\n"));
