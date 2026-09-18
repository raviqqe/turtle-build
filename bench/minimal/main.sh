#!/bin/sh

set -e

touch foo.in

cat <<'EOF' >build.ninja
rule cp
  command = cp $in $out
  description = run faster

build foo.out: cp foo.in
EOF
