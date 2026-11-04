// Single mapping from file type to icon / display label / color tone / preview capability.
import { t } from "./i18n";

export type Kind =
  | "folder"
  | "archive"
  | "image"
  | "video"
  | "audio"
  | "pdf"
  | "code"
  | "text"
  | "file";

export interface KindInfo {
  kind: Kind;
  icon: string;
  /** Type-column text: Folder / MD / JSON / ... */
  label: string;
  tone: string;
  preview: "none" | "text" | "image";
}

const EXT_MAP: Record<string, [Kind, string]> = {};
function reg(kind: Kind, icon: string, label: string, exts: string[]) {
  for (const e of exts) EXT_MAP[e] = [kind, label];
}

reg("code", "code", "JS", ["js", "mjs", "cjs"]);
reg("code", "code", "TS", ["ts", "tsx", "jsx"]);
reg("code", "code", "VUE", ["vue"]);
reg("code", "code", "JSON", ["json"]);
reg("code", "code", "HTML", ["html", "htm"]);
reg("code", "code", "CSS", ["css", "scss", "less"]);
reg("code", "code", "RS", ["rs"]);
reg("code", "code", "GO", ["go"]);
reg("code", "code", "PY", ["py"]);
reg("code", "code", "JAVA", ["java"]);
reg("code", "code", "C", ["c", "h"]);
reg("code", "code", "CPP", ["cpp", "hpp", "cc"]);
reg("code", "code", "CS", ["cs"]);
reg("code", "code", "SWIFT", ["swift"]);
reg("code", "code", "KT", ["kt"]);
reg("code", "code", "RB", ["rb"]);
reg("code", "code", "PHP", ["php"]);
reg("code", "code", "SQL", ["sql"]);
reg("code", "code", "SH", ["sh", "bash", "zsh", "bat", "ps1"]);
reg("code", "code", "YAML", ["yml", "yaml"]);
reg("code", "code", "TOML", ["toml"]);
reg("code", "code", "XML", ["xml"]);

reg("text", "file-text", "MD", ["md", "markdown"]);
reg("text", "file-text", "TXT", ["txt", "log"]);
reg("text", "file-text", "CSV", ["csv", "tsv"]);
reg("text", "file-text", "CONF", ["ini", "conf", "cfg", "env", "properties", "lock", "gitignore", "editorconfig"]);

reg("image", "image", "PNG", ["png"]);
reg("image", "image", "JPG", ["jpg", "jpeg"]);
reg("image", "image", "GIF", ["gif"]);
reg("image", "image", "WEBP", ["webp"]);
reg("image", "image", "SVG", ["svg"]);
reg("image", "image", "ICO", ["ico"]);
reg("image", "image", "BMP", ["bmp"]);
reg("image", "image", "AVIF", ["avif"]);
reg("image", "image", "TIFF", ["tif", "tiff"]);
reg("image", "image", "HEIC", ["heic"]);

reg("video", "video", "MP4", ["mp4"]);
reg("video", "video", "MOV", ["mov"]);
reg("video", "video", "MKV", ["mkv"]);
reg("video", "video", "AVI", ["avi"]);
reg("video", "video", "WEBM", ["webm"]);

reg("audio", "audio", "MP3", ["mp3"]);
reg("audio", "audio", "WAV", ["wav"]);
reg("audio", "audio", "FLAC", ["flac"]);
reg("audio", "audio", "AAC", ["aac"]);
reg("audio", "audio", "OGG", ["ogg"]);

reg("pdf", "pdf", "PDF", ["pdf"]);

reg("archive", "archive", "ZIP", ["zip"]);
reg("archive", "archive", "7Z", ["7z"]);
reg("archive", "archive", "RAR", ["rar"]);
reg("archive", "archive", "TAR", ["tar"]);
reg("archive", "archive", "GZ", ["gz", "tgz"]);
reg("archive", "archive", "BZ2", ["bz2", "tbz2"]);
reg("archive", "archive", "XZ", ["xz", "txz"]);
reg("archive", "archive", "ZST", ["zst", "tzst"]);

const TONES: Record<Kind, string> = {
  folder: "tone-folder",
  archive: "tone-archive",
  image: "tone-image",
  video: "tone-video",
  audio: "tone-audio",
  pdf: "tone-pdf",
  code: "tone-code",
  text: "tone-text",
  file: "tone-file",
};

export function kindOf(name: string, isDir: boolean): KindInfo {
  if (isDir) return { kind: "folder", icon: "folder", label: t("info.kindFolder"), tone: TONES.folder, preview: "none" };
  const base = name.replace(/\\/g, "/");
  const ext = base.includes(".") ? base.split(".").pop()!.toLowerCase() : "";
  const hit = ext ? EXT_MAP[ext] : undefined;
  if (hit) {
    const [kind, label] = hit;
    return {
      kind,
      icon: kind === "pdf" ? "pdf" : kind === "archive" ? "archive" : kind,
      label,
      tone: TONES[kind],
      preview: kind === "image" ? "image" : kind === "text" || kind === "code" ? "text" : "none",
    };
  }
  return { kind: "file", icon: "file", label: ext ? ext.toUpperCase() : t("info.kindFile"), tone: TONES.file, preview: "text" };
}

const TEXT_EXTRA = new Set(["makefile", "dockerfile", "license", "readme", "changelog", "gitignore"]);
export function isPreviewableText(name: string): boolean {
  const base = name.replace(/\\/g, "/").split("/").pop()!.toLowerCase();
  if (!base.includes(".") && TEXT_EXTRA.has(base)) return true;
  return kindOf(base, false).preview === "text";
}
