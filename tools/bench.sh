#!/bin/sh

set -e

cd $(dirname $0)/../benchmark/$1
rm -rf tmp
mkdir tmp
cd tmp

../main.sh

cargo install hyperfine

clean='rm -rf *.out .ninja* .turtle*'

hyperfine -L tool ninja,turtle -n '{tool} (clean build)' --prepare "$clean" '{tool}'
hyperfine -L tool ninja,turtle -n '{tool} (no-op build)' --setup "$clean" --warmup 1 '{tool}'
