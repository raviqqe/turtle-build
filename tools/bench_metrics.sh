#!/bin/sh

set -e

cd $(dirname $0)/../bench

jq -n --arg os $1 '
  def metrics(key; name; value): {
    key: "\(key)-\($os)",
    name: "\(name) relative to Ninja on \($os)",
    metrics: map({
      key: .turtle.name,
      value: ((.turtle | value) / (.ninja | value))
    })
  };

  [inputs.results[] | . + (.command | capture("(?<tool>.+) \\((?<name>.+)\\)"))]
  | group_by(.name)
  | map(INDEX(.tool))
  | metrics("time"; "build time"; .mean),
    metrics("memory"; "peak memory usage"; .memory_usage_byte | max)
' */tmp/*.json
