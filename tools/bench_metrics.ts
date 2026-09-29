#!/usr/bin/env node

import { glob, readFile } from "node:fs/promises";
import { join } from "node:path";
import { parseArgs } from "node:util";

interface Result {
  benchmark: string;
  memory: number;
  time: number;
  tool: string;
}

interface MetricSet {
  key: string;
  metrics: { key: string; value: number }[];
  name: string;
}

const metricNames = [
  ["time", "build time"],
  ["memory", "peak memory usage"],
] as const;

const parseResult = (result: unknown): Result => {
  if (
    typeof result !== "object" ||
    result === null ||
    !("command" in result) ||
    typeof result.command !== "string" ||
    !("mean" in result) ||
    typeof result.mean !== "number" ||
    !("memory_usage_byte" in result) ||
    !Array.isArray(result.memory_usage_byte) ||
    result.memory_usage_byte.length === 0 ||
    !result.memory_usage_byte.every(
      (value: unknown) => typeof value === "number",
    )
  ) {
    throw new Error(`invalid result: ${JSON.stringify(result)}`);
  }

  const [, tool, benchmark] = /^(\S+) \((.+)\)$/.exec(result.command) ?? [];

  if (tool === undefined || benchmark === undefined) {
    throw new Error(`invalid command: ${result.command}`);
  }

  return {
    benchmark,
    memory: Math.max(...result.memory_usage_byte),
    time: result.mean,
    tool,
  };
};

const parseResults = (report: unknown): Result[] => {
  if (
    typeof report !== "object" ||
    report === null ||
    !("results" in report) ||
    !Array.isArray(report.results)
  ) {
    throw new Error(`invalid report: ${JSON.stringify(report)}`);
  }

  return report.results.map(parseResult);
};

const findResult = (
  results: Result[],
  tool: string,
  benchmark: string,
): Result => {
  const result = results.find(
    (result) => result.tool === tool && result.benchmark === benchmark,
  );

  if (!result) {
    throw new Error(`result not found: ${tool} (${benchmark})`);
  }

  return result;
};

const compileMetrics = (os: string, results: Result[]): MetricSet[] =>
  metricNames.map(([key, name]) => ({
    key: `${key}-${os}`,
    metrics: [...new Set(results.map(({ benchmark }) => benchmark))]
      .toSorted()
      .map((benchmark) => ({
        key: benchmark,
        value:
          findResult(results, "turtle", benchmark)[key] /
          findResult(results, "ninja", benchmark)[key],
      })),
    name: `${name} relative to Ninja on ${os}`,
  }));

const readResults = async (directory: string): Promise<Result[]> =>
  (
    await Promise.all(
      (
        await Array.fromAsync(glob("*/tmp/*.json", { cwd: directory }))
      ).map(async (path) =>
        parseResults(
          JSON.parse(await readFile(join(directory, path), "utf-8")),
        ),
      ),
    )
  ).flat();

const {
  positionals: [os],
} = parseArgs({ allowPositionals: true });

if (!os) {
  throw new Error("os argument not defined");
}

for (const metricSet of compileMetrics(
  os,
  await readResults(join(import.meta.dirname, "../bench")),
)) {
  console.log(JSON.stringify(metricSet, null, 2));
}
