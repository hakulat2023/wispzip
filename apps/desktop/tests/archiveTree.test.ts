// ArchiveIndex regression tests (node --test, zero dependencies).
// Regression background: children used to store full path names while expandToFiles matched
// by basename, so "files inside a subdirectory" expanded to nothing -> 0 files extracted.
// Invariants:
//   children(dir) -> entry names are relative basenames;
//   all           -> entry names are full paths (separator always "/").
import assert from "node:assert/strict";
import { test } from "node:test";
import { ArchiveIndex } from "../src/lib/archiveTree.ts";

const FIXTURE = [
  { name: "项目甲", isDir: true, size: 0 },
  { name: "项目甲/附件1 报告-0731.docx", isDir: false, size: 598 },
  { name: "项目甲/风险用例-0809.xlsx", isDir: false, size: 14 },
  { name: "项目甲/深/嵌套.txt", isDir: false, size: 3 },
  { name: "top.txt", isDir: false, size: 3 },
];

function makeIndex(extraEntries = []) {
  return new ArchiveIndex([...FIXTURE, ...extraEntries]);
}

test("children stores relative basenames, all stores full paths", () => {
  const idx = makeIndex();
  const kids = idx.children("项目甲").map((e) => e.name);
  assert.deepEqual(kids.sort(), ["附件1 报告-0731.docx", "风险用例-0809.xlsx", "深"].sort());
  // all holds full paths with no duplicated prefix (nested dirs must not become dir/dir/...)
  assert.ok(idx.all.every((e) => !e.name.startsWith("项目甲/项目甲/")));
  assert.ok(idx.all.some((e) => e.name === "项目甲/深/嵌套.txt"));
  // Implicit directory nodes (archives without explicit dir entries) also appear in all
  assert.ok(idx.all.some((e) => e.isDir && e.name === "项目甲/深"));
});

test("stats: root and subdirectory aggregates are correct (including nesting)", () => {
  const idx = makeIndex();
  const root = idx.stat("");
  assert.equal(root.files, 4);
  assert.equal(root.dirs, 2); // explicit dir + implicit nested dir
  const sub = idx.stat("项目甲");
  assert.equal(sub.files, 3);
  assert.equal(sub.dirs, 1);
  assert.equal(idx.stat("项目甲/深").files, 1);
});

test("expandToFiles: files inside a subdirectory (main regression case)", () => {
  const idx = makeIndex();
  assert.deepEqual(idx.expandToFiles(["项目甲/风险用例-0809.xlsx"]), [
    "项目甲/风险用例-0809.xlsx",
  ]);
});

test("expandToFiles: file at the root", () => {
  const idx = makeIndex();
  assert.deepEqual(idx.expandToFiles(["top.txt"]), ["top.txt"]);
});

test("expandToFiles: a folder expands to all its files", () => {
  const idx = makeIndex();
  const out = idx.expandToFiles(["项目甲"]);
  assert.equal(out.length, 3);
  assert.ok(out.includes("项目甲/附件1 报告-0731.docx"));
  assert.ok(out.includes("项目甲/深/嵌套.txt"));
});

test("expandToFiles: multi-select merge; a missing selection returns empty", () => {
  const idx = makeIndex();
  const out = idx.expandToFiles(["top.txt", "项目甲/深/嵌套.txt"]);
  assert.equal(out.length, 2);
  assert.deepEqual(idx.expandToFiles(["不存在.txt"]), []);
});

test("expandToFiles: backslash entry names normalize to forward slashes", () => {
  const idx = new ArchiveIndex([
    { name: "dir\\file.txt", isDir: false, size: 5 },
    { name: "dir\\sub\\inner.txt", isDir: false, size: 6 },
    { name: "dir\\", isDir: true, size: 0 }, // trailing-slash fallback: must not create an empty name
  ]);
  // Both display and selection use "/"
  assert.deepEqual(idx.expandToFiles(["dir/file.txt"]), ["dir/file.txt"]);
  assert.deepEqual(idx.expandToFiles(["dir/sub"]), ["dir/sub/inner.txt"]);
  assert.ok(idx.all.some((e) => e.name === "dir/sub/inner.txt"));
  assert.ok(idx.all.every((e) => e.name !== "" && !e.name.endsWith("/")));
});
