#!/bin/sh

set -e

git clone --depth 1 --branch v4.4.3 https://github.com/Kitware/CMake .

cmake \
  -G Ninja \
  -S . \
  -B build \
  -D CMAKE_BUILD_TYPE=Release \
  -D BUILD_TESTING=OFF
