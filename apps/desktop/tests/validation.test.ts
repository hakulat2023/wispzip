// Input validation schema tests (node --test, zero dependencies).
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  validateExtractOptions,
  validateCompressOptions,
} from "../src/lib/validation.ts";

test("validateExtractOptions: valid input passes", () => {
  const input = {
    dest: "/Users/test/output",
    entries: ["file1.txt", "dir/file2.txt"],
    conflict: "rename",
    encoding: "auto",
    password: "secret",
  };
  const result = validateExtractOptions(input);
  assert.equal(result.dest, "/Users/test/output");
  assert.equal(result.password, "secret");
});

test("validateExtractOptions: empty password is treated as undefined", () => {
  const input = {
    dest: "/output",
    entries: undefined,
    conflict: "skip",
    encoding: "utf8",
    password: "",
  };
  const result = validateExtractOptions(input);
  assert.equal(result.password, undefined);
});

test("validateExtractOptions: empty destination path throws", () => {
  const input = {
    dest: "",
    entries: undefined,
    conflict: "rename",
    encoding: "auto",
  };
  assert.throws(() => validateExtractOptions(input), /目标路径必填/);
});

test("validateExtractOptions: invalid conflict value throws", () => {
  const input = {
    dest: "/output",
    conflict: "invalid",
    encoding: "auto",
  };
  assert.throws(() => validateExtractOptions(input));
});

test("validateCompressOptions: valid input passes", () => {
  const input = {
    out: "/output/archive.zip",
    level: 6,
    password: "secret",
    splitBytes: 104857600,
    excludeHidden: true,
  };
  const result = validateCompressOptions(input);
  assert.equal(result.out, "/output/archive.zip");
  assert.equal(result.level, 6);
  assert.equal(result.excludeHidden, true);
});

test("validateCompressOptions: compression level bounds", () => {
  // zstd supports 1-22
  const input1 = {
    out: "/output/archive.zst",
    level: 22,
    excludeHidden: false,
  };
  const result1 = validateCompressOptions(input1);
  assert.equal(result1.level, 22);

  // Out of range
  const input2 = {
    out: "/output/archive.zip",
    level: 23,
    excludeHidden: false,
  };
  assert.throws(() => validateCompressOptions(input2), /压缩级别最大为 22/);
});

test("validateCompressOptions: empty output path throws", () => {
  const input = {
    out: "",
    level: 6,
    excludeHidden: false,
  };
  assert.throws(() => validateCompressOptions(input), /输出路径必填/);
});

test("validateCompressOptions: split size must be positive", () => {
  const input = {
    out: "/output/archive.zip",
    level: 6,
    splitBytes: -100,
    excludeHidden: false,
  };
  assert.throws(() => validateCompressOptions(input), /分卷大小必须为正数/);
});
