#!/bin/sh

set -e

cd $(dirname $0)/..

jq -n --arg os $1 -f tools/bench_metrics.jq bench/*/tmp/*/*.json
