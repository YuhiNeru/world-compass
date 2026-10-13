import { readFileSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { CatalogSchema } from "../../packages/contracts/index.ts";
const [source, output, grantFile] = process.argv.slice(2);
if (!source || !output)
  throw Error(
    "Usage: node tools/catalog-builder/build.mjs input.json output.json [grants.json]",
  );
const catalog = CatalogSchema.parse(JSON.parse(readFileSync(source, "utf8")));
if (!catalog.fixture) {
  if (!grantFile)
    throw Error("Production catalog requires explicit redistribution grants");
  const grants = JSON.parse(readFileSync(grantFile, "utf8"));
  for (const w of catalog.worlds)
    if (
      !grants.some(
        (g) =>
          g.id === w.permissionRef &&
          g.license === w.sourceLicense &&
          g.redistributionAllowed === true,
      )
    )
      throw Error(`Missing grant: ${w.worldId}`);
}
const data = JSON.stringify(catalog, null, 2);
writeFileSync(output, data);
console.log(
  JSON.stringify({
    version: catalog.version,
    fixture: catalog.fixture,
    worldCount: catalog.worlds.length,
    size: Buffer.byteLength(data),
    sha256: createHash("sha256").update(data).digest("hex"),
  }),
);
