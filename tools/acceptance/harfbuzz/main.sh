#!/bin/sh

set -e

git clone --depth 1 --branch 14.4.0 https://github.com/harfbuzz/harfbuzz .

# cspell: ignore nodownload
meson setup --wrap-mode nodownload build
