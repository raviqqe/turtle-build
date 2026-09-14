#!/bin/sh

set -e

git clone --depth 1 --branch v1.13.2 https://github.com/ninja-build/ninja source
mkdir build
cd build
../source/configure.py
