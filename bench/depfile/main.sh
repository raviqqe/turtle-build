#!/bin/sh

set -e

build_count=1000
header_count=100

headers=$(seq -f %g.h 0 $header_count)

touch $headers
echo headers = $headers >build.ninja

cat <<'EOF' >>build.ninja
rule cc
  command = printf '$out: $in $headers\n' > $out.d && cp $in $out
  depfile = $out.d
  deps = gcc
  description = run faster
EOF

for index in $(seq 0 $build_count); do
  touch $index.c
  echo build $index.out: cc $index.c
done >>build.ninja
