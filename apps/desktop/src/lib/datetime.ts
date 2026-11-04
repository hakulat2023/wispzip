// Date display: Today/Yesterday include the time, same-year dates use "M d", other years use "yyyy/M/d"
// (matching Finder). Non-Chinese locales use Today/Yesterday plus a short "Sep 1" format.
import { t, currentLocale } from "./i18n";

const LOCALE_TAG = () => (currentLocale() === "zh" ? "zh-CN" : "en-US");

export function fmtDate(unixSeconds?: number): string {
  if (!unixSeconds) return "—";
  const d = new Date(unixSeconds * 1000);
  const now = new Date();
  const startOfToday = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
  const time = d.getTime();
  const hm = `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  if (time >= startOfToday) return `${t("date.today")} ${hm}`;
  if (time >= startOfToday - 86400_000) return `${t("date.yesterday")} ${hm}`;
  const sameYear = d.getFullYear() === now.getFullYear();
  if (sameYear) {
    if (currentLocale() === "zh") return t("date.monthDay", { m: d.getMonth() + 1, d: d.getDate() });
    return d.toLocaleDateString(LOCALE_TAG(), { month: "short", day: "numeric" });
  }
  return `${d.getFullYear()}/${d.getMonth() + 1}/${d.getDate()}`;
}

/** Full timestamp (used by the file info panel) */
export function fmtDateTime(unixSeconds?: number): string {
  if (!unixSeconds) return "—";
  const d = new Date(unixSeconds * 1000);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

export function fmtRatio(size: number, compressed: number): string {
  if (!size || compressed <= 0) return "—";
  const r = Math.round((1 - compressed / size) * 100);
  if (r <= 0) return "0%";
  return `${Math.min(99, r)}%`;
}
