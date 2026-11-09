// Die Extension als Paket: jedes Modul laedt, und alles, was sie braucht, liegt
// direkt in extension/.
//
// deck-services::vscode_ext (vscode_ext_files.rs) kopiert nur die Dateien DIESER
// Ebene nach ~/.vscode/extensions/agent-deck-bridge. Ein Modul in einem Unterordner
// fehlte dort, `require` wuerfe beim Aktivieren, und VS Code laedt dann GAR NICHTS
// (CLAUDE.md, Daten und Vertraege 6).

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("fs");
const path = require("path");

const DIR = path.join(__dirname, "..");
const jsFiles = fs.readdirSync(DIR).filter((f) => f.endsWith(".js"));
const localRequires = (file) => [...fs.readFileSync(path.join(DIR, file), "utf8")
  .matchAll(/require\("(\.[^"]+)"\)/g)].map((m) => m[1]);

test("package.json main points to a file next to it", () => {
  const pkg = JSON.parse(fs.readFileSync(path.join(DIR, "package.json"), "utf8"));
  assert.ok(fs.statSync(path.join(DIR, pkg.main)).isFile(), pkg.main);
});

test("every local require resolves to a file directly in extension/", () => {
  for (const file of jsFiles) {
    for (const rel of localRequires(file)) {
      assert.ok(/^\.\/[\w-]+$/.test(rel), `${file}: require("${rel}") zeigt aus der obersten Ebene heraus`);
      assert.ok(jsFiles.includes(rel.slice(2) + ".js"), `${file}: ${rel}.js fehlt`);
    }
  }
});

test("only extension.js loads vscode, every other module loads with node alone", () => {
  for (const file of jsFiles) {
    if (file === "extension.js") continue;
    assert.doesNotThrow(() => require(path.join(DIR, file)), file);
    assert.ok(!/require\("vscode"\)/.test(fs.readFileSync(path.join(DIR, file), "utf8")), `${file} laedt vscode`);
  }
});

test("each module stays below 400 lines", () => {
  for (const file of jsFiles) {
    const lines = fs.readFileSync(path.join(DIR, file), "utf8").split("\n").length;
    assert.ok(lines < 400, `${file}: ${lines} Zeilen`);
  }
});
