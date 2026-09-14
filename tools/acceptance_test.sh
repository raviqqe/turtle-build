#!/bin/sh

set -e

cd $(dirname $0)/..

export PATH=$PWD/target/release:$PATH

if [ $# -eq 0 ]; then
  set -- $(ls tools/acceptance)
fi

for name in "$@"; do
  (
    rm -rf tmp/$name
    mkdir -p tmp/$name
    cd tmp/$name

    ../../tools/acceptance/$name/main.sh

    turtle -C build
    [ -z "$(turtle -C build 2>&1 | tee /dev/stderr)" ]
  )
done
