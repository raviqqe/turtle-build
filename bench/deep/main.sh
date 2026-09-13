#!/bin/sh

set -e

build_count=1000

touch 0.in

cat <<'EOF' >build.ninja
rule cp
  command = cp $in $out
  description = run faster

build 0.out: cp 0.in
EOF

for index in $(seq $build_count); do
  echo build $index.out: cp $((index - 1)).out
done >>build.ninja
