#!/bin/sh

set -e

cd $(dirname $0)/../bench

jq -n --arg os $1 '{
  key: $os,
  name: "build time on \($os) (ms)",
  metrics: [inputs.results[] | {key: .command, value: (.mean * 1000)}]
}' */tmp/*.json
