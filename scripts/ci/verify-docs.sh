#!/usr/bin/env bash
set -euo pipefail

workspace_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${workspace_dir}"

mdbook build docs
cp llms.txt target/book/llms.txt
node scripts/ci/verify-docs.mjs target/book
