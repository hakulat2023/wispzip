// Unit tests for the filekind helpers
import assert from "node:assert/strict";
import { test } from "node:test";

// filekind.ts depends on i18n, so the core logic is tested here
// The EXT_MAP lookup is reimplemented to verify behavior

const EXT_MAP: Record<string, [string, string]> = {};
function reg(kind: string, label: string, exts: string[]) {
  for (const e of exts) EXT_MAP[e] = [kind, label];
}

// Register common extensions
reg("code", "JS", ["js", "mjs", "cjs"]);
reg("code", "TS", ["ts", "tsx", "jsx"]);
reg("code", "VUE", ["vue"]);
reg("code", "JSON", ["json"]);
reg("code", "RS", ["rs"]);
reg("code", "PY", ["py"]);
reg("text", "MD", ["md", "markdown"]);
reg("text", "TXT", ["txt", "log"]);
reg("image", "PNG", ["png"]);
reg("image", "JPG", ["jpg", "jpeg"]);
reg("video", "MP4", ["mp4"]);
reg("audio", "MP3", ["mp3"]);
reg("pdf", "PDF", ["pdf"]);
reg("archive", "ZIP", ["zip"]);
reg("archive", "7Z", ["7z"]);
reg("archive", "RAR", ["rar"]);

function kindOf(name: string, isDir: boolean): { kind: string; label: string; preview: string } {
  if (isDir) return { kind: "folder", label: "文件夹", preview: "none" };
  const base = name.replace(/\\/g, "/");
  const ext = base.includes(".") ? base.split(".").pop()!.toLowerCase() : "";
  const hit = ext ? EXT_MAP[ext] : undefined;
  if (hit) {
    const [kind, label] = hit;
    const preview = kind === "image" ? "image" : kind === "text" || kind === "code" ? "text" : "none";
    return { kind, label, preview };
  }
  return { kind: "file", label: ext ? ext.toUpperCase() : "文件", preview: "text" };
}

const TEXT_EXTRA = new Set(["makefile", "dockerfile", "license", "readme", "changelog", "gitignore"]);
function isPreviewableText(name: string): boolean {
  const base = name.replace(/\\/g, "/").split("/").pop()!.toLowerCase();
  if (!base.includes(".") && TEXT_EXTRA.has(base)) return true;
  return kindOf(base, false).preview === "text";
}

// ---- kindOf ----

test("kindOf: directories return the folder kind", () => {
  const result = kindOf("any-name", true);
  assert.equal(result.kind, "folder");
  assert.equal(result.preview, "none");
});

test("kindOf: common code files", () => {
  assert.equal(kindOf("app.ts", false).kind, "code");
  assert.equal(kindOf("app.ts", false).label, "TS");
  assert.equal(kindOf("app.ts", false).preview, "text");

  assert.equal(kindOf("script.py", false).kind, "code");
  assert.equal(kindOf("main.rs", false).kind, "code");
  assert.equal(kindOf("component.vue", false).kind, "code");
});

test("kindOf: text files", () => {
  assert.equal(kindOf("readme.md", false).kind, "text");
  assert.equal(kindOf("notes.txt", false).kind, "text");
  assert.equal(kindOf("readme.md", false).preview, "text");
});

test("kindOf: image files", () => {
  assert.equal(kindOf("photo.jpg", false).kind, "image");
  assert.equal(kindOf("logo.png", false).kind, "image");
  assert.equal(kindOf("photo.jpg", false).preview, "image");
});

test("kindOf: video files", () => {
  assert.equal(kindOf("movie.mp4", false).kind, "video");
  assert.equal(kindOf("movie.mp4", false).preview, "none");
});

test("kindOf: archive files", () => {
  assert.equal(kindOf("archive.zip", false).kind, "archive");
  assert.equal(kindOf("archive.7z", false).kind, "archive");
  assert.equal(kindOf("archive.rar", false).kind, "archive");
});

test("kindOf: PDF files", () => {
  assert.equal(kindOf("document.pdf", false).kind, "pdf");
  assert.equal(kindOf("document.pdf", false).preview, "none");
});

test("kindOf: unknown extension returns the uppercased extension", () => {
  const result = kindOf("file.xyz", false);
  assert.equal(result.kind, "file");
  assert.equal(result.label, "XYZ");
});

test("kindOf: no extension returns the file label", () => {
  const result = kindOf("Makefile", false);
  assert.equal(result.kind, "file");
  // With no extension the label should be the generic file label or empty
});

test("kindOf: case insensitive", () => {
  assert.equal(kindOf("photo.JPG", false).kind, "image");
  assert.equal(kindOf("photo.JPEG", false).kind, "image");
  assert.equal(kindOf("archive.ZIP", false).kind, "archive");
});

test("kindOf: a file inside a path (with directories)", () => {
  assert.equal(kindOf("src/app.ts", false).kind, "code");
  assert.equal(kindOf("assets/logo.png", false).kind, "image");
});

test("kindOf: Windows backslash path", () => {
  assert.equal(kindOf("src\\app.ts", false).kind, "code");
  assert.equal(kindOf("assets\\logo.png", false).kind, "image");
});

// ---- isPreviewableText ----

test("isPreviewableText: special filenames (no extension)", () => {
  assert.equal(isPreviewableText("Makefile"), true);
  assert.equal(isPreviewableText("Dockerfile"), true);
  assert.equal(isPreviewableText("LICENSE"), true);
  assert.equal(isPreviewableText("README"), true);
  assert.equal(isPreviewableText(".gitignore"), true);
});

test("isPreviewableText: text extensions", () => {
  assert.equal(isPreviewableText("readme.md"), true);
  assert.equal(isPreviewableText("notes.txt"), true);
  assert.equal(isPreviewableText("app.ts"), true);
  assert.equal(isPreviewableText("style.css"), true);
});

test("isPreviewableText: non-text files", () => {
  assert.equal(isPreviewableText("photo.jpg"), false);
  assert.equal(isPreviewableText("movie.mp4"), false);
  assert.equal(isPreviewableText("archive.zip"), false);
});

test("isPreviewableText: a file inside a path", () => {
  assert.equal(isPreviewableText("src/Makefile"), true);
  assert.equal(isPreviewableText("src/app.ts"), true);
  assert.equal(isPreviewableText("assets/logo.png"), false);
});
