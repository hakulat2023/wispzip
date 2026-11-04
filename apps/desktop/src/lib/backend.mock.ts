// Mock implementation of the WispZip frontend transport layer: for non-Tauri environments only
// (opening `pnpm dev` directly). Fake data for UI work and visual review, zero component changes.

import type { EntryInfo, PreviewResult } from "./backend";

const now = Math.floor(Date.now() / 1000);
const H = 3600;
let mockJob: number | null = null;
const mockTimers: number[] = [];

export function mockSubEntries(dir: string): EntryInfo[] {
  const t = (ago: number) => now - ago;
  const table: Record<string, [string, number, number, number][]> = {
    "": [
      ["src", 0, 0, 3 * H],
      ["assets", 0, 0, 5 * H],
      ["docs", 0, 0, 26 * H],
      ["README.md", 12380, 4210, 2 * H],
      ["package.json", 3120, 1140, 2 * H],
      ["LICENSE", 1070, 620, 27 * H],
      [".gitignore", 214, 160, 27 * H],
      ["logo.png", 839680, 655300, 28 * H],
      ["demo.mp4", 85983232, 84201100, 30 * H],
    ],
    src: [
      ["components", 0, 0, 3 * H],
      ["utils", 0, 0, 4 * H],
      ["index.js", 28672, 8900, 2 * H],
      ["config.json", 6144, 1700, 2 * H],
      ["app.ts", 18432, 5600, 6 * H],
    ],
    "src/components": [
      ["Browser.vue", 21504, 5900, 2 * H],
      ["Table.vue", 12288, 3600, 3 * H],
      ["Dialog.vue", 10240, 3100, 3 * H],
    ],
    "src/utils": [
      ["format.ts", 4608, 1500, 4 * H],
      ["tree.ts", 7168, 2200, 4 * H],
    ],
    assets: [
      ["logo.png", 839680, 655300, 28 * H],
      ["banner.jpg", 2457600, 2300100, 29 * H],
      ["icons.svg", 18432, 5400, 29 * H],
    ],
    docs: [
      ["design.md", 32768, 11200, 26 * H],
      ["roadmap.md", 9216, 3400, 27 * H],
      ["api.md", 15360, 5100, 25 * H],
    ],
  };
  return (table[dir] ?? []).map(([name, size, comp, ago]) => ({
    name,
    isDir: size === 0,
    size,
    compressedSize: comp,
    mtime: t(ago),
  }));
}

let mockList: EntryInfo[] | null = null;
export function mockAllEntries(): EntryInfo[] {
  if (mockList) return mockList;
  const all: EntryInfo[] = [];
  const walk = (dir: string) => {
    for (const e of mockSubEntries(dir)) {
      const name = dir ? `${dir}/${e.name}` : e.name;
      all.push({ ...e, name });
      if (e.isDir) walk(name);
    }
  };
  walk("");
  mockList = all;
  return mockList;
}

const MOCK_TEXT = `# Project

This project is a modern archive browser.

双击压缩包，像打开文件夹一样浏览内容 —— 不解压，也能查看、打开、提取里面的文件。

## Installation

\`\`\`bash
npm install
\`\`\`

## Usage

- 拖入压缩包直接浏览
- 右键任意文件可解压、查看或查看文件信息
- 支持 ZIP / 7Z / RAR / TAR 等常见格式

## License

MIT
`;

type MockEmitFn = <T>(event: string, payload: T) => void;

export function createMockInvoke(emit: MockEmitFn) {
  return function mockInvoke<T>(cmd: string, args: Record<string, unknown>): Promise<T> {
    const p = (v: unknown, ms = 120) => new Promise<T>((r) => setTimeout(() => r(v as T), ms));
    switch (cmd) {
      case "pick_files":
        return p([String(args.filters ? "/Users/demo/照片备份.zip" : "/Users/demo/项目资料")], 10);
      case "pick_folder":
        return p("/Users/demo/Downloads", 10);
      case "pick_save":
        return p("/Users/demo/新建压缩包.zip", 10);
      case "launch_args":
        return p([] as string[], 0);
      case "detect_archive":
        return p(
          {
            format: { supports_password: true, level_range: [0, 9] },
            classification: "archive",
            supportsPassword: true,
            levelRange: { min: 0, max: 9 },
          } as unknown as T,
          10,
        );
      case "stat_path":
        return p({ size: 41_943_040, isDir: false } as unknown as T, 10);
      case "list_archive": {
        if (args.password === "wrong") {
          return Promise.reject({ kind: "password", message: "需要密码" });
        }
        return p([...mockAllEntries()] as unknown as T, 260);
      }
      case "scan_inputs":
        return p({ files: 2584, dirs: 312, bytes: 1_932_735_283, skipped: 0 } as unknown as T, 200);
      case "start_compress":
      case "start_extract": {
        const jobId = 101;
        const total = cmd === "start_extract" ? 86_000_000 : 1_900_000_000;
        let done = 0;
        const iv = window.setInterval(() => {
          done += total / 22;
          emit("wispzip:progress", {
            jobId,
            phase: cmd === "start_extract" ? "extract" : "compress",
            currentFile: "src/components/Browser.vue",
            processedBytes: Math.min(done, total),
            totalBytes: total,
            outputBytes: Math.min(done, total),
            filesDone: Math.floor((done / total) * 2584),
            filesTotal: 2584,
          });
          if (done >= total) {
            window.clearInterval(iv);
            emit("wispzip:done", {
              jobId,
              ok: true,
              result: cmd === "start_extract"
                ? { files: 2584, dirs: 312, bytes: total, skipped: 0, dest: "/Users/demo/Downloads/project" }
                : { inputFiles: 2584, inputBytes: total, outputBytes: total * 0.42, skipped: 0, outputPath: "/Users/demo/项目归档.zip" },
            });
          }
        }, 180);
        mockTimers.push(iv);
        return p(jobId as unknown as T, 60);
      }
      case "cancel_job":
        if (mockJob) mockJob = null;
        return p(true as unknown as T, 0);
      case "pause_job":
      case "resume_job":
        return p(true as unknown as T, 0);
      case "reveal_path":
      case "open_path":
        return p(undefined as unknown as T, 10);
      case "open_entry":
        return p("/tmp/mock/README.md" as unknown as T, 500);
      case "preview_entry": {
        const entry = String(args.entry ?? "");
        const res: PreviewResult = /\.(png|jpe?g|gif|bmp|webp|svg|ico|avif)$/i.test(entry)
          ? {
              kind: "image",
              mime: "image/svg+xml",
              content:
                "data:image/svg+xml;charset=utf-8," +
                encodeURIComponent(
                  `<svg xmlns="http://www.w3.org/2000/svg" width="960" height="540"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#6d7cff"/><stop offset="1" stop-color="#b16dff"/></linearGradient></defs><rect width="960" height="540" fill="url(#g)" rx="24"/><circle cx="480" cy="230" r="90" fill="rgba(255,255,255,.92)"/><path d="M430 230l80 45-80 45z" fill="#6d7cff"/><text x="480" y="420" font-family="PingFang SC, sans-serif" font-size="34" fill="#fff" text-anchor="middle">logo.png 示意图</text></svg>`,
                ),
              truncated: false,
              size: 839_680,
            }
          : { kind: "text", content: MOCK_TEXT, truncated: false, size: 12_380 };
        return p(res as unknown as T, 420);
      }
      case "install_finder_services":
        return p(undefined as unknown as T, 300);
      default:
        console.warn("[mock] unhandled command", cmd, args);
        return p(undefined as unknown as T, 0);
    }
  };
}
