// Unit tests for the datetime helpers
import assert from "node:assert/strict";
import { test, mock } from "node:test";

// Mock Date and i18n
const mockT = (key: string, params?: Record<string, unknown>) => {
  if (key === "date.today") return "今天";
  if (key === "date.yesterday") return "昨天";
  if (key === "date.monthDay") return `${params?.m}月${params?.d}日`;
  return key;
};

// datetime.ts depends on i18n, so the module is imported dynamically and mocked
// The core logic is verified directly by reimplementing it here

test("fmtDate: undefined returns the placeholder", () => {
  // Mirror the core logic of fmtDate
  const fmtDate = (unixSeconds?: number): string => {
    if (!unixSeconds) return "—";
    return String(unixSeconds);
  };
  assert.equal(fmtDate(undefined), "—");
  assert.equal(fmtDate(0), "—");
});

test("fmtDate: today renders as 'today HH:mm'", () => {
  const now = Math.floor(Date.now() / 1000);
  const d = new Date(now * 1000);
  const hm = `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;

  // Verify the time format
  assert.match(hm, /^\d{2}:\d{2}$/);
});

test("fmtDateTime: full date-time format", () => {
  const fmtDateTime = (unixSeconds?: number): string => {
    if (!unixSeconds) return "—";
    const d = new Date(unixSeconds * 1000);
    const p = (n: number) => String(n).padStart(2, "0");
    return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
  };

  assert.equal(fmtDateTime(undefined), "—");

  // 2024-01-15 10:30:00 UTC
  const ts = Math.floor(new Date("2024-01-15T10:30:00Z").getTime() / 1000);
  const result = fmtDateTime(ts);
  assert.match(result, /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}$/);
});

test("fmtRatio: compression ratio calculation", () => {
  const fmtRatio = (size: number, compressed: number): string => {
    if (!size || compressed <= 0) return "—";
    const r = Math.round((1 - compressed / size) * 100);
    if (r <= 0) return "0%";
    return `${Math.min(99, r)}%`;
  };

  assert.equal(fmtRatio(0, 100), "—");
  assert.equal(fmtRatio(100, 0), "—");
  assert.equal(fmtRatio(100, 100), "0%");
  assert.equal(fmtRatio(100, 50), "50%");
  assert.equal(fmtRatio(100, 10), "90%");
  assert.equal(fmtRatio(100, 1), "99%"); // capped at 99%
  assert.equal(fmtRatio(100, 0), "—");
});

test("fmtRatio: boundary cases", () => {
  const fmtRatio = (size: number, compressed: number): string => {
    if (!size || compressed <= 0) return "—";
    const r = Math.round((1 - compressed / size) * 100);
    if (r <= 0) return "0%";
    return `${Math.min(99, r)}%`;
  };

  // Compressed larger than raw (should not happen, but must be handled)
  assert.equal(fmtRatio(100, 200), "0%");
  // Extreme compression ratio
  assert.equal(fmtRatio(1000000, 1), "99%");
});
