#!/bin/sh

set -e

build_count=1000

touch 0.in

cat <<'EOF' >build.ninja
rule cp
  command = cp $in $out
  description = run faster

rule dyndep
  command = printf 'ninja_dyndep_version = 1\nbuild $output: dyndep | 0.out\n' > $out
  description = run faster

build 0.out: cp 0.in
EOF

for index in $(seq $build_count); do
  touch $index.in
  echo build $index.dd.out: dyndep $index.in
  echo '' output = $index.out
  echo build $index.out: cp $index.in '||' $index.dd.out
  echo '' dyndep = $index.dd.out
done >>build.ninja
