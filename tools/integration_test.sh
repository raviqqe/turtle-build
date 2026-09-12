#!/bin/sh

set -e

cd $(dirname $0)/..

export PATH=$PWD/target/release:$PATH

go tool agoa "$@"
