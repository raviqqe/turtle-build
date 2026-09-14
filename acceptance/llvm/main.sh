#!/bin/sh

set -e

git clone --depth 1 --branch llvmorg-23.1.1 --filter blob:none --sparse https://github.com/llvm/llvm-project source
git -C source sparse-checkout set cmake libc llvm third-party

cmake \
  -G Ninja \
  -S source/llvm \
  -B build \
  -D CMAKE_BUILD_TYPE=Release \
  -D LLVM_TARGETS_TO_BUILD=host
