// ArchiveIndex edge cases and stress tests
import assert from "node:assert/strict";
import { test } from "node:test";
import { ArchiveIndex } from "../src/lib/archiveTree.ts";

// ---- Edge cases ----

test("ArchiveIndex: empty entry list", () => {
  const idx = new ArchiveIndex([]);
  assert.deepEqual(idx.children(""), []);
  assert.deepEqual(idx.all, []);
  const stat = idx.stat("");
  assert.equal(stat.files, 0);
  assert.equal(stat.dirs, 0);
});

test("ArchiveIndex: files only, no directories", () => {
  const idx = new ArchiveIndex([
    { name: "file1.txt", isDir: false, size: 100 },
    { name: "file2.txt", isDir: false, size: 200 },
  ]);
  const kids = idx.children("").map((e) => e.name);
  assert.deepEqual(kids.sort(), ["file1.txt", "file2.txt"]);
  assert.equal(idx.stat("").files, 2);
  assert.equal(idx.stat("").dirs, 0);
});

test("ArchiveIndex: directories only, no files", () => {
  const idx = new ArchiveIndex([
    { name: "dir1", isDir: true, size: 0 },
    { name: "dir2", isDir: true, size: 0 },
  ]);
  const kids = idx.children("").map((e) => e.name);
  assert.deepEqual(kids.sort(), ["dir1", "dir2"]);
  assert.equal(idx.stat("").files, 0);
  assert.equal(idx.stat("").dirs, 2);
});

test("ArchiveIndex: deep nesting (10 levels)", () => {
  const entries = [];
  for (let i = 1; i <= 10; i++) {
    entries.push({ name: `level${i}`, isDir: true, size: 0 });
  }
  entries.push({ name: "level1/level2/level3/level4/level5/level6/level7/level8/level9/level10/file.txt", isDir: false, size: 100 });

  const idx = new ArchiveIndex(entries);
  assert.equal(idx.stat("level1/level2/level3/level4/level5/level6/level7/level8/level9/level10").files, 1);
  assert.ok(idx.all.some((e) => e.name === "level1/level2/level3/level4/level5/level6/level7/level8/level9/level10/file.txt"));
});

test("ArchiveIndex: same name for a file and a directory (edge case)", () => {
  // Some archives may contain both a "foo" file and a "foo/" directory
  const idx = new ArchiveIndex([
    { name: "foo", isDir: false, size: 100 },
    { name: "foo/", isDir: true, size: 0 }, // trailing slash
    { name: "foo/bar.txt", isDir: false, size: 50 },
  ]);
  // Must handle this without crashing
  assert.ok(idx.all.length > 0);
});

test("ArchiveIndex: very large entry list (10000 files)", () => {
  const entries = [];
  for (let i = 0; i < 10000; i++) {
    entries.push({ name: `dir/file${i}.txt`, isDir: false, size: i * 100 });
  }
  const idx = new ArchiveIndex(entries);
  assert.equal(idx.stat("dir").files, 10000);
  assert.equal(idx.all.length, 10001); // 10000 files + 1 implicit directory
});

test("ArchiveIndex: compressed size field", () => {
  const idx = new ArchiveIndex([
    { name: "file.txt", isDir: false, size: 1000, compressedSize: 500 },
    { name: "dir/file2.txt", isDir: false, size: 2000, compressedSize: 800 },
  ]);
  const stat = idx.stat("");
  assert.equal(stat.size, 3000);
  assert.equal(stat.compSize, 1300);
});

test("ArchiveIndex: expandToFiles with an empty selection", () => {
  const idx = new ArchiveIndex([
    { name: "file.txt", isDir: false, size: 100 },
  ]);
  assert.deepEqual(idx.expandToFiles([]), []);
});

test("ArchiveIndex: expandToFiles deduplicates a selection", () => {
  const idx = new ArchiveIndex([
    { name: "file.txt", isDir: false, size: 100 },
  ]);
  const result = idx.expandToFiles(["file.txt", "file.txt", "file.txt"]);
  assert.equal(result.length, 1);
  assert.deepEqual(result, ["file.txt"]);
});

test("ArchiveIndex: expandToFiles with mixed dirs and files", () => {
  const idx = new ArchiveIndex([
    { name: "dir", isDir: true, size: 0 },
    { name: "dir/file1.txt", isDir: false, size: 100 },
    { name: "dir/file2.txt", isDir: false, size: 200 },
    { name: "other.txt", isDir: false, size: 50 },
  ]);
  const result = idx.expandToFiles(["dir", "other.txt"]);
  assert.equal(result.length, 3);
  assert.ok(result.includes("dir/file1.txt"));
  assert.ok(result.includes("dir/file2.txt"));
  assert.ok(result.includes("other.txt"));
});

// ---- hasDir ----

test("ArchiveIndex: hasDir detects an existing directory", () => {
  const idx = new ArchiveIndex([
    { name: "dir", isDir: true, size: 0 },
    { name: "dir/file.txt", isDir: false, size: 100 },
  ]);
  assert.equal(idx.hasDir(""), true);
  assert.equal(idx.hasDir("dir"), true);
  assert.equal(idx.hasDir("nonexistent"), false);
});

test("ArchiveIndex: hasDir with implicit directories", () => {
  const idx = new ArchiveIndex([
    { name: "a/b/c/file.txt", isDir: false, size: 100 },
  ]);
  assert.equal(idx.hasDir("a"), true);
  assert.equal(idx.hasDir("a/b"), true);
  assert.equal(idx.hasDir("a/b/c"), true);
});

// ---- Path separator normalization ----

test("ArchiveIndex: mixed separator normalization", () => {
  const idx = new ArchiveIndex([
    { name: "dir\\file1.txt", isDir: false, size: 100 },
    { name: "dir/file2.txt", isDir: false, size: 200 },
    { name: "dir/sub\\file3.txt", isDir: false, size: 300 },
  ]);
  // All paths should unify to "/"
  assert.ok(idx.all.every((e) => !e.name.includes("\\")));
  assert.equal(idx.stat("dir").files, 3);
});

test("ArchiveIndex: multiple consecutive slashes", () => {
  const idx = new ArchiveIndex([
    { name: "dir//file.txt", isDir: false, size: 100 },
  ]);
  // Must handle this without crashing
  assert.ok(idx.all.length > 0);
});

// ---- Special characters ----

test("ArchiveIndex: CJK paths", () => {
  const idx = new ArchiveIndex([
    { name: "照片/2024年/春节.jpg", isDir: false, size: 1024 },
    { name: "文档/报告.docx", isDir: false, size: 2048 },
  ]);
  assert.equal(idx.stat("照片").files, 1);
  assert.equal(idx.stat("照片/2024年").files, 1);
  assert.ok(idx.all.some((e) => e.name === "照片/2024年/春节.jpg"));
});

test("ArchiveIndex: paths with spaces", () => {
  const idx = new ArchiveIndex([
    { name: "my folder/my file.txt", isDir: false, size: 100 },
  ]);
  assert.equal(idx.stat("my folder").files, 1);
  assert.ok(idx.all.some((e) => e.name === "my folder/my file.txt"));
});

test("ArchiveIndex: filenames with special symbols", () => {
  const idx = new ArchiveIndex([
    { name: "file@2x.png", isDir: false, size: 100 },
    { name: "file#1.txt", isDir: false, size: 200 },
    { name: "file&name.txt", isDir: false, size: 300 },
  ]);
  assert.equal(idx.stat("").files, 3);
});
