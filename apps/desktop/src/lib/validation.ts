// Input validation schemas: zod rules for frontend user input.

import { z } from "zod";

// Password: optional, an empty string is treated as undefined
export const passwordSchema = z.string().transform((val) => val.trim() || undefined).optional();

// Destination path: required and non-empty
export const destPathSchema = z.string().min(1, "目标路径必填");

// Compression level: 0-9 (zstd allows 1-22, but a common range is used here)
export const levelSchema = z.number().int().min(0, "压缩级别最小为 0").max(22, "压缩级别最大为 22");

// Split size: optional, but must be a positive number of bytes when provided
export const splitBytesSchema = z.number().positive("分卷大小必须为正数").optional();

// Extract dialog input
export const extractOptionsSchema = z.object({
  dest: destPathSchema,
  entries: z.array(z.string()).optional(),
  conflict: z.enum(["skip", "overwrite", "rename"]),
  encoding: z.enum(["auto", "utf8", "gbk"]),
  password: passwordSchema,
});

// Compress dialog input
export const compressOptionsSchema = z.object({
  out: z.string().min(1, "输出路径必填"),
  level: levelSchema,
  password: passwordSchema,
  splitBytes: splitBytesSchema,
  excludeHidden: z.boolean(),
});

// Type exports
export type ExtractOptionsInput = z.infer<typeof extractOptionsSchema>;
export type CompressOptionsInput = z.infer<typeof compressOptionsSchema>;

// Validation helpers
export function validateExtractOptions(input: unknown): ExtractOptionsInput {
  return extractOptionsSchema.parse(input);
}

export function validateCompressOptions(input: unknown): CompressOptionsInput {
  return compressOptionsSchema.parse(input);
}
