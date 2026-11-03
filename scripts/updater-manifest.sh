#!/usr/bin/env bash
#
# Writes the updater manifest (`latest.json`) for a release, to stdout.
#
# Why this exists instead of trusting the manifest the bundler uploads: `tauri-action`'s
# `uploadUpdaterJson` names each asset by its **API** URL
# (`https://api.github.com/repos/<repo>/releases/assets/<id>`), and that URL is not the file. Asking
# it without a token answers `404` while the release is a draft, and after publishing it answers a
# JSON description of the asset rather than the bytes. The application compares what it downloaded
# against the signature in this manifest, so a manifest pointing at a description is an update that
# can never install — measured on the first real release (L12 of `odd/tasks/ci-and-release.md`).
#
# The public download URL has a shape that does not depend on the API:
#
#   https://github.com/<repo>/releases/download/<tag>/<asset>
#
# so this script constructs it, reads the signatures from the release's own `.sig` assets, and refuses
# to write a manifest that promises an artifact it cannot verify.
#
# Usage:
#   scripts/updater-manifest.sh <tag> [owner/repo]
#
# `owner/repo` defaults to `$GITHUB_REPOSITORY`, which is what the runner sets. Requires `gh` (with a
# token that can read the release, drafts included), `python3` and coreutils.
#
# The release does not have to be published for this to run: a draft can be read with a token, which
# is what makes the manifest verifiable before a human promotes anything.

set -euo pipefail

tag="${1:-}"
repo="${2:-${GITHUB_REPOSITORY:-}}"

die() {
    printf 'updater-manifest: %s\n' "$*" >&2
    exit 1
}

[ -n "${tag}" ] || die "usage: updater-manifest.sh <tag> [owner/repo]"
[ -n "${repo}" ] || die "no repository: pass it as the second argument or set GITHUB_REPOSITORY"

command -v gh > /dev/null || die "gh is not installed"
command -v python3 > /dev/null || die "python3 is not installed"

work="$(mktemp -d)"
trap 'rm -rf "${work}"' EXIT

# One call for the metadata, one for the signatures. The signature of a Tauri bundle is the contents
# of the `.sig` file the bundler wrote next to it, and it is the only thing that binds this manifest to
# the bytes the user will download.
gh release view "${tag}" --repo "${repo}" --json tagName,body,publishedAt,assets > "${work}/release.json" \
    || die "the release ${tag} could not be read from ${repo}"

mkdir -p "${work}/signatures"
gh release download "${tag}" --repo "${repo}" --pattern '*.sig' --dir "${work}/signatures" --clobber \
    || die "the release ${tag} has no .sig assets; a manifest without signatures installs nothing"

MANIFEST_TAG="${tag}" MANIFEST_REPO="${repo}" MANIFEST_WORK="${work}" python3 - <<'PYTHON'
import json
import os
import sys

work = os.environ["MANIFEST_WORK"]
tag = os.environ["MANIFEST_TAG"]
repo = os.environ["MANIFEST_REPO"]

with open(os.path.join(work, "release.json"), encoding="utf-8") as handle:
    release = json.load(handle)

# The key each bundle answers to. Tauri's Linux updater asks for the platform key and falls back to the
# format-specific one, which is why `linux-x86_64` and `linux-x86_64-appimage` carry the same artifact:
# that is the shape the bundler itself emits, and an installed copy is already looking for it.
BY_SUFFIX = (
    (".AppImage", "linux-x86_64-appimage"),
    (".deb", "linux-x86_64-deb"),
    (".rpm", "linux-x86_64-rpm"),
)
GENERIC = ("linux-x86_64-appimage", "linux-x86_64")

artifacts = []
for asset in release["assets"]:
    name = asset["name"]
    if name.endswith(".sig"):
        continue
    for suffix, key in BY_SUFFIX:
        if name.endswith(suffix):
            artifacts.append((name, key))
            break

if not artifacts:
    sys.exit("updater-manifest: the release carries no .deb, .rpm or .AppImage to name")

platforms = {}
for name, key in artifacts:
    signature_path = os.path.join(work, "signatures", name + ".sig")
    if not os.path.exists(signature_path):
        # A bundle without its signature is exactly the artifact that cannot be installed safely, so
        # this fails here rather than reaching an installed copy as a manifest it must reject.
        sys.exit(f"updater-manifest: no signature for {name}")
    with open(signature_path, encoding="utf-8") as handle:
        signature = handle.read().strip()
    if not signature:
        sys.exit(f"updater-manifest: the signature of {name} is empty")
    entry = {
        "signature": signature,
        "url": f"https://github.com/{repo}/releases/download/{tag}/{name}",
    }
    platforms[key] = entry
    if key in GENERIC:
        platforms["linux-x86_64"] = entry

manifest = {
    "version": tag[1:] if tag.startswith("v") else tag,
    "notes": release.get("body") or "",
    "pub_date": release.get("publishedAt") or release.get("createdAt") or "",
    "platforms": platforms,
}
json.dump(manifest, sys.stdout, indent=2, sort_keys=True)
sys.stdout.write("\n")
PYTHON
