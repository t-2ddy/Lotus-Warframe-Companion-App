// Sets the app version everywhere it's written: package.json (which tauri.conf.json reads),
// package-lock.json, src-tauri/Cargo.toml and src-tauri/Cargo.lock.
//
//   npm run bump -- 0.2.0
//
// The version needs its bug-fix notes in src/lib/releaseNotes.ts first (shown once in the app after
// it updates).
//
// Then commit, tag v0.2.0 and push the tag: the Release workflow builds the installer.
import { readFileSync, writeFileSync } from "node:fs";

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version ?? "")) {
  console.error("Usage: npm run bump -- <x.y.z>   (e.g. 0.2.0 or 0.2.0-beta.1)");
  process.exit(1);
}

/** Replaces the version matched by `pattern` (groups: before, old version, after). */
function update(path, pattern, limit = 1) {
  const text = readFileSync(path, "utf8");
  let n = 0;
  const out = text.replace(pattern, (m, a, _old, b) => (n++ < limit ? `${a}${version}${b}` : m));
  if (n === 0) throw new Error(`${path}: version not found`);
  writeFileSync(path, out);
  console.log(`  ${path}`);
}

const notes = readFileSync("src/lib/releaseNotes.ts", "utf8");
if (!notes.includes(`version: "${version}"`)) {
  console.error(`src/lib/releaseNotes.ts has no notes for ${version}: add a short list of what it fixes first.`);
  process.exit(1);
}

console.log(`Version ${version}:`);
update("package.json", /("version":\s*")([^"]+)(")/);
// The lock file's own version: the root and packages[""] (the first two "version" fields).
update("package-lock.json", /("version":\s*")([^"]+)(")/g, 2);
update("src-tauri/Cargo.toml", /(\[package\][^[]*?\nversion = ")([^"]+)(")/);
update("src-tauri/Cargo.lock", /(\[\[package\]\]\r?\nname = "wf-companion"\r?\nversion = ")([^"]+)(")/);
console.log(`Next: git commit -am "Release v${version}" && git tag v${version} && git push && git push --tags`);
