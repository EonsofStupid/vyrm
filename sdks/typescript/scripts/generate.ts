import { execFileSync } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import openapiTS, { astToString } from "openapi-typescript";

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repositoryRoot = resolve(packageRoot, "../..");
const schemaOutput = resolve(packageRoot, "src/generated/rrd-openapi.ts");
const endpointsOutput = resolve(packageRoot, "src/generated/endpoints.ts");
const raw = execFileSync(
  "cargo",
  [
    "run",
    "-q",
    "--manifest-path",
    resolve(repositoryRoot, "Cargo.toml"),
    "-p",
    "rrd-contract",
    "--bin",
    "rrd-contract-export",
  ],
  { encoding: "utf8", maxBuffer: 16 * 1024 * 1024 },
);
const document: unknown = JSON.parse(raw);
const generatedSchema = `${astToString(await openapiTS(document as never))}\n`;
const source = document as {
  paths: Record<
    string,
    Record<
      string,
      {
        operationId?: string;
        security?: Record<string, unknown>[];
        "x-rrd-mutation"?: boolean;
      }
    >
  >;
};
const endpoints = Object.fromEntries(
  Object.entries(source.paths).flatMap(([path, pathItem]) =>
    Object.entries(pathItem).flatMap(([method, operation]) => {
      if (!operation.operationId) return [];
      const scheme = Object.keys(operation.security?.[0] ?? {})[0];
      const authentication =
        scheme === "rrdApiKey" ? "api_key" : scheme === "rrdBearer" ? "session_bearer" : "public";
      return [
        [
          operation.operationId,
          {
            method: method.toUpperCase(),
            path,
            authentication,
            mutation: operation["x-rrd-mutation"],
          },
        ],
      ];
    }),
  ),
);
const generatedEndpoints = `// Generated from rrd-contract; do not edit.\nexport const endpoints = ${JSON.stringify(endpoints, null, 2)} as const;\n\nexport type OperationId = keyof typeof endpoints;\n`;

if (process.argv.includes("--check")) {
  const currentSchema = await readFile(schemaOutput, "utf8").catch(() => "");
  const currentEndpoints = await readFile(endpointsOutput, "utf8").catch(() => "");
  if (currentSchema !== generatedSchema || currentEndpoints !== generatedEndpoints) {
    throw new Error("generated TypeScript RRD schema is stale; run pnpm generate");
  }
} else {
  await mkdir(dirname(schemaOutput), { recursive: true });
  await writeFile(schemaOutput, generatedSchema, "utf8");
  await writeFile(endpointsOutput, generatedEndpoints, "utf8");
}
