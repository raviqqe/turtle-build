#!/bin/sh

set -e

cd $(dirname $0)/..

export PATH=$PWD/target/release:$PATH

cd acceptance

if [ $# -eq 0 ]; then
  set -- *
fi

for name in "$@"; do
  (
    cd $name
    rm -rf tmp
    mkdir tmp
    cd tmp

    ../main.sh

    turtle
    [ -z "$(turtle 2>&1 | tee /dev/stderr)" ]
  )
done
