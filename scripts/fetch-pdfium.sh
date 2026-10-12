#!/usr/bin/env bash
# Fetches the pdfium native library that `oct-scan` loads at runtime.
#
# Why this exists: page counting needs a real PDF parser (risk R2), and the measured spike
# (spike/parser/FINDINGS.md) showed that `pdfium` counts pages of real documents in <= 0.1 s with a
# flat 6-22 MB footprint, while `lopdf` never finished on 2 of 7 documents and cost up to 26 s and
# 290 MB on the rest. `pdfium` is a native library, so it has to be fetched and, in a packaged
# release, bundled beside the executable.
#
# The library is deliberately NOT committed: it is ~8 MB of platform-specific binary. This script is
# the reproducible way to obtain it, and CI will call it.
#
# Usage:  scripts/fetch-pdfium.sh [target-directory]     (default: vendor/pdfium)
#
# Source: https://github.com/bblanchon/pdfium-binaries — prebuilt pdfium, BSD-3-Clause, which is
# GPL-3.0 compatible. The archive's LICENSE is kept next to the library.

set -euo pipefail

destination="${1:-vendor/pdfium}"

case "$(uname -s)-$(uname -m)" in
    Linux-x86_64)   asset="pdfium-linux-x64.tgz" ;;
    Linux-aarch64)  asset="pdfium-linux-arm64.tgz" ;;
    Darwin-x86_64)  asset="pdfium-mac-x64.tgz" ;;
    Darwin-arm64)   asset="pdfium-mac-arm64.tgz" ;;
    MINGW*|MSYS*|CYGWIN*) asset="pdfium-win-x64.tgz" ;;
    *)
        echo "no prebuilt pdfium asset is known for $(uname -s)-$(uname -m)" >&2
        echo "see https://github.com/bblanchon/pdfium-binaries/releases" >&2
        exit 1
        ;;
esac

url="https://github.com/bblanchon/pdfium-binaries/releases/latest/download/${asset}"
echo "fetching ${asset} -> ${destination}"

mkdir -p "${destination}"
temporary="$(mktemp -d)"
trap 'rm -rf "${temporary}"' EXIT

curl --fail --location --silent --show-error --output "${temporary}/${asset}" "${url}"
tar -xzf "${temporary}/${asset}" -C "${temporary}"
mkdir -p "${destination}/lib"
cp "${temporary}"/lib/* "${destination}/lib/"
cp "${temporary}/LICENSE" "${destination}/LICENSE"

echo "done:"
ls -la "${destination}/lib"
echo
echo "oct-scan looks for the library at \$OCT_PDFIUM_LIB first, then in ${destination}/lib,"
echo "then in the system library path. A packaged release bundles it beside the executable."
