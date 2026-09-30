#!/bin/sh

set -e

build_count=$1

cat <<'EOF' >build.ninja
rule cp
  command = cp $in $out
  description = run faster

EOF

for index in $(seq $build_count); do
  touch $index.in
  echo build $index.out: cp $index.in
done >>build.ninja

echo build all: phony $(seq -f %g.out $build_count) >>build.ninja
echo default all >>build.ninja
