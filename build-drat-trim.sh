#!/bin/bash
set -e
mkdir -p bin
make -C vendor/drat-trim drat-trim
mv vendor/drat-trim/drat-trim bin/
