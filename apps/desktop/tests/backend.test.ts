// Unit tests for the backend helper functions
import assert from "node:assert/strict";
import { test } from "node:test";

// Reimplement the backend.ts helpers to test them

function fmtBytes(n: number): string {
  if (!n || n <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  let v = n;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${i === 0 ? v.toFixed(0) : v.toFixed(1)} ${units[i]}`;
}

function fileName(p: string): string {
  return p.split(/[\\/]/).pop() ?? p;
}

// ---- fmtBytes ----

test("fmtBytes: 0 and negatives return '0 B'", () => {
  assert.equal(fmtBytes(0), "0 B");
  assert.equal(fmtBytes(-100), "0 B");
  assert.equal(fmtBytes(NaN), "0 B");
});

test("fmtBytes: byte unit", () => {
  assert.equal(fmtBytes(1), "1 B");
  assert.equal(fmtBytes(100), "100 B");
  assert.equal(fmtBytes(1023), "1023 B");
});

test("fmtBytes: KB unit", () => {
  assert.equal(fmtBytes(1024), "1.0 KB");
  assert.equal(fmtBytes(1536), "1.5 KB");
  assert.equal(fmtBytes(10240), "10.0 KB");
});

test("fmtBytes: MB unit", () => {
  assert.equal(fmtBytes(1048576), "1.0 MB");
  assert.equal(fmtBytes(1572864), "1.5 MB");
  assert.equal(fmtBytes(10485760), "10.0 MB");
});

test("fmtBytes: GB unit", () => {
  assert.equal(fmtBytes(1073741824), "1.0 GB");
  assert.equal(fmtBytes(1610612736), "1.5 GB");
});

test("fmtBytes: TB unit", () => {
  assert.equal(fmtBytes(1099511627776), "1.0 TB");
});

test("fmtBytes: boundary values", () => {
  // exactly 1024
  assert.equal(fmtBytes(1024), "1.0 KB");
  // just below 1024
  assert.equal(fmtBytes(1023), "1023 B");
  // huge value (1e15 bytes ~ 909.5 TB)
  assert.equal(fmtBytes(1e15), "909.5 TB");
});

// ---- fileName ----

test("fileName: Unix path", () => {
  assert.equal(fileName("/Users/test/file.txt"), "file.txt");
  assert.equal(fileName("relative/path/file.txt"), "file.txt");
  assert.equal(fileName("file.txt"), "file.txt");
});

test("fileName: Windows path", () => {
  assert.equal(fileName("C:\\Users\\test\\file.txt"), "file.txt");
  assert.equal(fileName("relative\\path\\file.txt"), "file.txt");
});

test("fileName: mixed path", () => {
  assert.equal(fileName("path/to\\file.txt"), "file.txt");
});

test("fileName: empty string", () => {
  assert.equal(fileName(""), "");
});

test("fileName: separators only", () => {
  assert.equal(fileName("/"), "");
  assert.equal(fileName("\\"), "");
});

test("fileName: directory path (no filename)", () => {
  assert.equal(fileName("/path/to/dir/"), "");
  assert.equal(fileName("C:\\path\\to\\dir\\"), "");
});

test("fileName: CJK filename", () => {
  assert.equal(fileName("/Users/test/照片备份.zip"), "照片备份.zip");
  assert.equal(fileName("C:\\Users\\test\\文档.7z"), "文档.7z");
});

test("fileName: special characters", () => {
  assert.equal(fileName("path/file with spaces.txt"), "file with spaces.txt");
  assert.equal(fileName("path/file@2x.png"), "file@2x.png");
});
