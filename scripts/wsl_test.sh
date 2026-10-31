#!/usr/bin/env bash
# Linux (WSL) verification for smartlink. Copies the repo onto the native
# ext4 filesystem (symlinks/hardlinks do not work on the /mnt/c drvfs mount),
# then runs fmt / clippy / tests / a release build.
set -uo pipefail

SRC="/mnt/c/Users/Piyush/~ai-home/repos/smartlink"
DST="/root/smartlink"
export CARGO_TERM_COLOR=never

rm -rf "$DST"
mkdir -p "$DST"
cp -a "$SRC/." "$DST/"
rm -rf "$DST/target" "$DST/.git"
cd "$DST" || exit 1

echo "== files =="
ls -1

echo "== fmt =="
if cargo fmt --all -- --check; then echo FMT_OK; else echo FMT_FAIL; fi

echo "== clippy =="
if cargo clippy --all-targets --all-features -- -D warnings >/root/clippy.log 2>&1; then
  echo CLIPPY_OK
else
  echo CLIPPY_FAIL
fi
tail -30 /root/clippy.log

echo "== test =="
if cargo test >/root/test.log 2>&1; then echo TEST_OK; else echo TEST_FAIL; fi
grep -E 'running [0-9]+ test|test result|error\[|panicked|FAILED' /root/test.log

echo "== release build =="
if cargo build --release >/root/rel.log 2>&1; then echo REL_OK; else echo REL_FAIL; fi
tail -8 /root/rel.log
ls -la "$DST/target/release/slink"

echo "== version =="
"$DST/target/release/slink" --version

echo DONE
