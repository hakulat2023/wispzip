// Integration tests: simulate common user flows
import assert from "node:assert/strict";
import { test } from "node:test";
import { ArchiveIndex } from "../src/lib/archiveTree.ts";

// ---- Scenario 1: open archive -> browse -> select files -> extract ----

test("integration: open an archive and browse into a subdirectory", () => {
  // Simulated archive contents
  const entries = [
    { name: "README.md", isDir: false, size: 1234, compressedSize: 500 },
    { name: "src", isDir: true, size: 0 },
    { name: "src/index.ts", isDir: false, size: 5678, compressedSize: 2000 },
    { name: "src/utils", isDir: true, size: 0 },
    { name: "src/utils/helper.ts", isDir: false, size: 2345, compressedSize: 800 },
    { name: "package.json", isDir: false, size: 890, compressedSize: 400 },
  ];

  const idx = new ArchiveIndex(entries);

  // 1. Open the root directory
  const rootKids = idx.children("").map((e) => e.name).sort();
  assert.deepEqual(rootKids, ["README.md", "package.json", "src"]);

  // 2. Enter the src directory
  const srcKids = idx.children("src").map((e) => e.name).sort();
  assert.deepEqual(srcKids, ["index.ts", "utils"]);

  // 3. Enter the utils directory
  const utilsKids = idx.children("src/utils").map((e) => e.name);
  assert.deepEqual(utilsKids, ["helper.ts"]);

  // 4. Check the stats
  const rootStat = idx.stat("");
  assert.equal(rootStat.files, 4); // README.md, src/index.ts, src/utils/helper.ts, package.json
  assert.equal(rootStat.dirs, 2); // src, src/utils
});

test("integration: selecting multiple files and directories expands to a file list", () => {
  const entries = [
    { name: "docs", isDir: true, size: 0 },
    { name: "docs/guide.md", isDir: false, size: 5000 },
    { name: "docs/api.md", isDir: false, size: 3000 },
    { name: "src", isDir: true, size: 0 },
    { name: "src/index.ts", isDir: false, size: 1000 },
    { name: "README.md", isDir: false, size: 2000 },
  ];

  const idx = new ArchiveIndex(entries);

  // The user selected the docs directory and README.md
  const selected = ["docs", "README.md"];
  const expanded = idx.expandToFiles(selected);

  // docs expands to two files, plus README.md
  assert.equal(expanded.length, 3);
  assert.ok(expanded.includes("docs/guide.md"));
  assert.ok(expanded.includes("docs/api.md"));
  assert.ok(expanded.includes("README.md"));
});

// ---- Scenario 2: search -> select a result -> extract ----

test("integration: search for a file and select it", () => {
  const entries = [
    { name: "src/components/Button.vue", isDir: false, size: 1500 },
    { name: "src/components/Input.vue", isDir: false, size: 1200 },
    { name: "src/utils/format.ts", isDir: false, size: 800 },
    { name: "tests/button.test.ts", isDir: false, size: 600 },
  ];

  const idx = new ArchiveIndex(entries);

  // Search for files containing "button" (case insensitive)
  const query = "button";
  const results = idx.all.filter((e) => e.name.toLowerCase().includes(query));
  assert.equal(results.length, 2);
  assert.ok(results.some((e) => e.name === "src/components/Button.vue"));
  assert.ok(results.some((e) => e.name === "tests/button.test.ts"));

  // Select the first file in the results
  const selected = [results[0].name];
  const expanded = idx.expandToFiles(selected);
  assert.equal(expanded.length, 1);
  assert.equal(expanded[0], results[0].name);
});

// ---- Scenario 3: split archive handling ----

test("integration: split archive entry path handling", () => {
  // Simulated split-archive entries (may contain "part1", "part2" dirs)
  const entries = [
    { name: "part1", isDir: true, size: 0 },
    { name: "part1/data.bin", isDir: false, size: 104857600 }, // 100MB
    { name: "part2", isDir: true, size: 0 },
    { name: "part2/data.bin", isDir: false, size: 104857600 },
  ];

  const idx = new ArchiveIndex(entries);

  // Verify the stats are correct
  const totalStat = idx.stat("");
  assert.equal(totalStat.files, 2);
  assert.equal(totalStat.size, 209715200); // 200MB

  // Select part1 for extraction
  const expanded = idx.expandToFiles(["part1"]);
  assert.deepEqual(expanded, ["part1/data.bin"]);
});

// ---- Scenario 4: GBK filename handling ----

test("integration: entries from a Chinese Windows archive", () => {
  // Simulated archive packed by Chinese Windows; filenames may use backslashes
  const entries = [
    { name: "工作文档\\2024年报告\\Q1总结.docx", isDir: false, size: 50000 },
    { name: "工作文档\\2024年报告\\Q2总结.docx", isDir: false, size: 60000 },
    { name: "工作文档\\会议纪要\\周会.txt", isDir: false, size: 5000 },
  ];

  const idx = new ArchiveIndex(entries);

  // Backslashes should normalize to forward slashes
  assert.ok(idx.all.every((e) => !e.name.includes("\\")));

  // The directory structure should be correct
  const rootKids = idx.children("").map((e) => e.name);
  assert.deepEqual(rootKids, ["工作文档"]);

  const subKids = idx.children("工作文档").map((e) => e.name).sort();
  assert.deepEqual(subKids, ["2024年报告", "会议纪要"]);

  // Select the whole directory
  const expanded = idx.expandToFiles(["工作文档"]);
  assert.equal(expanded.length, 3);
});

// ---- Scenario 5: empty directories ----

test("integration: archive containing empty directories", () => {
  const entries = [
    { name: "empty-dir", isDir: true, size: 0 },
    { name: "non-empty", isDir: true, size: 0 },
    { name: "non-empty/file.txt", isDir: false, size: 100 },
  ];

  const idx = new ArchiveIndex(entries);

  // Empty directories should be listed
  const rootKids = idx.children("").map((e) => e.name).sort();
  assert.deepEqual(rootKids, ["empty-dir", "non-empty"]);

  // Stats for the empty directory
  const emptyStat = idx.stat("empty-dir");
  assert.equal(emptyStat.files, 0);
  assert.equal(emptyStat.dirs, 0);

  // Expanding an empty directory should return nothing
  const expanded = idx.expandToFiles(["empty-dir"]);
  assert.deepEqual(expanded, []);
});

// ---- Scenario 6: deep nested selection ----

test("integration: select a deeply nested file", () => {
  const entries = [
    { name: "a/b/c/d/e/f/g/h/i/j/deep.txt", isDir: false, size: 100 },
  ];

  const idx = new ArchiveIndex(entries);

  // Select the deepest file
  const expanded = idx.expandToFiles(["a/b/c/d/e/f/g/h/i/j/deep.txt"]);
  assert.deepEqual(expanded, ["a/b/c/d/e/f/g/h/i/j/deep.txt"]);

  // Selecting the intermediate directory should expand to that file
  const expanded2 = idx.expandToFiles(["a/b/c"]);
  assert.deepEqual(expanded2, ["a/b/c/d/e/f/g/h/i/j/deep.txt"]);
});

// ---- Scenario 7: many small files ----

test("integration: many small files (node_modules simulation)", () => {
  const entries = [];
  // Simulate 100 packages with 10 files each
  for (let i = 0; i < 100; i++) {
    const pkg = `node_modules/package-${i}`;
    entries.push({ name: pkg, isDir: true, size: 0 });
    for (let j = 0; j < 10; j++) {
      entries.push({ name: `${pkg}/file-${j}.js`, isDir: false, size: 1000 });
    }
  }

  const idx = new ArchiveIndex(entries);

  // Verify the stats
  const totalStat = idx.stat("");
  assert.equal(totalStat.files, 1000);
  assert.equal(totalStat.dirs, 101); // node_modules + 100 packages

  // Select a single package
  const expanded = idx.expandToFiles(["node_modules/package-50"]);
  assert.equal(expanded.length, 10);
});
