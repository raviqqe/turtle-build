#!/bin/sh

set -e

cd $(dirname $0)/../bench

jq -n --arg os $1 '{
  key: $os,
  name: "build time relative to Ninja on \($os) (%)",
  metrics: [
    inputs.results | INDEX(.parameters.tool) | {
      key: (.turtle.command | capture("\\((?<name>.+)\\)").name),
      value: (.turtle.mean / .ninja.mean * 100)
    }
  ]
}' */tmp/*.json
