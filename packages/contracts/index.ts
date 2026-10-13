import { z } from "zod";
export const Attributes = [
  "rain",
  "night",
  "chill",
  "photo",
  "nature",
  "game",
  "light",
] as const;
export const AttributeSchema = z.enum(Attributes);
export const ConditionsSchema = z.strictObject({
  people: z.number().int().min(1).max(100).nullable(),
  platform: z.enum(["pc", "quest"]).nullable(),
  required: z.array(AttributeSchema).max(7),
  preferred: z.array(AttributeSchema).max(7),
  excluded: z.array(AttributeSchema).max(7),
});
export type Conditions = z.infer<typeof ConditionsSchema>;
export const WorldSchema = z.strictObject({
  schemaVersion: z.literal(1),
  worldId: z
    .string()
    .regex(
      /^(fixture-[a-z0-9-]+|wrld_[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})$/,
    ),
  name: z.string().min(1).max(200),
  description: z.string().nullable(),
  author: z.string().nullable(),
  tags: z.array(z.string()),
  platforms: z.array(z.enum(["pc", "quest"])).nullable(),
  capacity: z.number().int().positive().nullable(),
  contentWarnings: z.array(z.string()).nullable(),
  thumbnail: z
    .strictObject({ url: z.string().url(), license: z.string().min(1) })
    .nullable(),
  sourceUrl: z.string().url(),
  fetchedAt: z.string().datetime(),
  sourceLicense: z.string().min(1),
  permissionRef: z.string().min(1),
  deleted: z.boolean(),
  attributes: z.record(
    AttributeSchema,
    z.strictObject({
      value: z.boolean().nullable(),
      sourceId: z.string().min(1),
    }),
  ),
  sources: z.array(
    z.strictObject({
      id: z.string(),
      label: z.string(),
      url: z.string().url(),
    }),
  ),
});
export type WorldRecord = z.infer<typeof WorldSchema>;
export const CatalogSchema = z
  .strictObject({
    schemaVersion: z.literal(1),
    version: z.string().regex(/^[a-zA-Z0-9.-]+$/),
    fixture: z.boolean(),
    worlds: z.array(WorldSchema),
  })
  .superRefine((c, ctx) => {
    const ids = new Set<string>();
    for (const w of c.worlds) {
      if (ids.has(w.worldId))
        ctx.addIssue({ code: "custom", message: "duplicate world ID" });
      ids.add(w.worldId);
      if (c.fixture !== w.worldId.startsWith("fixture-"))
        ctx.addIssue({ code: "custom", message: "fixture boundary" });
      for (const a of Object.values(w.attributes))
        if (!w.sources.some((s) => s.id === a.sourceId))
          ctx.addIssue({ code: "custom", message: "unknown source" });
    }
  });
export type Catalog = z.infer<typeof CatalogSchema>;
export const PersonalSchema = z.strictObject({
  favorite: z.boolean(),
  visited: z.boolean(),
  excluded: z.boolean(),
  note: z.string().max(4000),
});
export type Personal = z.infer<typeof PersonalSchema>;
export const emptyPersonal: Personal = {
  favorite: false,
  visited: false,
  excluded: false,
  note: "",
};
export const ReasonsSchema = z.strictObject({
  items: z
    .array(
      z.strictObject({
        worldId: z.string(),
        evidence: z
          .array(
            z.strictObject({
              field: z.enum(["capacity", "platforms", ...Attributes]),
              sourceId: z.string().nullable(),
            }),
          )
          .min(1)
          .max(10),
        unknown: z
          .array(z.enum(["capacity", "platforms", ...Attributes]))
          .max(10),
      }),
    )
    .max(6),
});
export type Reasons = z.infer<typeof ReasonsSchema>;
