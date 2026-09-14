#!/bin/sh

set -e

git clone --depth 1 --branch v1.13.2 https://github.com/ninja-build/ninja .
mkdir build
cd build
../configure.py
