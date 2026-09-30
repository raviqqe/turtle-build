#!/usr/bin/env node

import { glob, readFile } from "node:fs/promises";
import { join } from "node:path";
import { parseArgs } from "node:util";
import {
  array,
  type InferOutput,
  minLength,
  number,
  object,
  parse,
  pipe,
  string,
} from "valibot";

const reportSchema = object({
  results: array(
    object({
      command: string(),
      mean: number(),
      memory_usage_byte: pipe(array(number()), minLength(1)),
    }),
  ),
});

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

const parseResult = ({
  command,
  mean,
  memory_usage_byte: memory,
}: InferOutput<typeof reportSchema>["results"][number]): Result => {
  const [, tool, benchmark] = /^(\S+) \((.+)\)$/.exec(command) ?? [];

  if (tool === undefined || benchmark === undefined) {
    throw new Error(`invalid command: ${command}`);
  }

  return { benchmark, memory: Math.max(...memory), time: mean, tool };
};

const readResults = async (directory: string): Promise<Result[]> =>
  (
    await Promise.all(
      (
        await Array.fromAsync(glob("*/tmp/*.json", { cwd: directory }))
      ).map(async (path) =>
        parse(
          reportSchema,
          JSON.parse(await readFile(join(directory, path), "utf-8")),
        ).results.map(parseResult),
      ),
    )
  ).flat();

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
