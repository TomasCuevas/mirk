#!/usr/bin/env bash
#
# The version has one source: `crates/mirk-desktop/tauri.conf.json` (S4 of
# `odd/tasks/ci-and-release.md`).
#
# Why it needs one at all: a release used to be able to update two of the three places the version
# lives and publish a binary whose `--version`, whose `.deb` metadata and whose tag disagreed. The
# user-visible consequence is an installed app that lies about what it is — and, once the updater
# exists, an update check that compares the wrong number against `latest.json`.
#
# The source of truth is the Tauri config because it is the file the packagers read: `deb`, `rpm` and
# `AppImage` metadata all come from it, so it is the one that cannot be wrong without the artifacts
# being wrong too. `Cargo.toml` (the workspace `[workspace.package] version`, which every crate
# inherits with `version.workspace = true`) and `package.json` follow it.
#
# Usage:
#   scripts/version-sync.sh                 # write tauri.conf.json's version into the other two
#   scripts/version-sync.sh --check         # exit 1 when the three disagree; write nothing
#   scripts/version-sync.sh --print         # print the canonical version and exit
#   scripts/version-sync.sh --tag v0.1.0    # exit 1 unless the tag matches the canonical version
#
# `--check` is what CI runs on every change; `--tag` is what the release runs against the pushed tag.
# Both are read-only. Only the no-argument form writes, and it writes nothing when the files already
# agree, so running it on a synced tree leaves no modified files behind.
#
# Requires only bash, awk, grep and coreutils. Deliberately no `jq`: this script must work on the
# smallest runner and on a laptop that never installed it.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tauri_conf="${root}/crates/mirk-desktop/tauri.conf.json"
cargo_toml="${root}/Cargo.toml"
package_json="${root}/package.json"

die() {
    printf 'version-sync: %s\n' "$*" >&2
    exit 1
}

# --- reading -----------------------------------------------------------------------------------
#
# The extraction is deliberately textual and anchored rather than a JSON/Cargo parse: these are three
# files with one known shape each, and a parser dependency would be a heavier promise than the job
# needs. Each reader matches a leading key in the right section, so a nested `"version"` elsewhere in a
# file cannot be picked up by accident.

# The first top-level-looking `"version": "x"` of a JSON file.
read_json_version() {
    awk '
        /^[[:space:]]*"version"[[:space:]]*:/ {
            line = $0
            sub(/^[^:]*:[[:space:]]*"/, "", line)
            sub(/".*$/, "", line)
            print line
            exit
        }
    ' "$1"
}

# The `version` key of Cargo.toml's `[workspace.package]` section, and only that section: the same
# file has `version = "..."` under `[workspace.dependencies]` for crates.io dependencies, and picking
# one of those up would silently pin the wrong number.
read_cargo_workspace_version() {
    awk '
        /^[[:space:]]*\[/ {
            section = $0
            sub(/^[[:space:]]*\[/, "", section)
            sub(/\][[:space:]]*(#.*)?$/, "", section)
            next
        }
        section == "workspace.package" && /^[[:space:]]*version[[:space:]]*=/ {
            line = $0
            sub(/^[[:space:]]*version[[:space:]]*=[[:space:]]*"/, "", line)
            sub(/".*$/, "", line)
            print line
            exit
        }
    ' "$1"
}

canonical_version() {
    local version
    version="$(read_json_version "${tauri_conf}")"
    [ -n "${version}" ] || die "no \"version\" key found in ${tauri_conf}"
    printf '%s\n' "${version}"
}

# A version this script is willing to write: SemVer `MAJOR.MINOR.PATCH`, optionally with a
# pre-release/build suffix (the reader's decision is a `v0.1.0` tag marked pre-release by the release
# itself, but a `-rc.1` in the tree must not be rejected).
assert_semver() {
    local version="$1"
    case "${version}" in
        [0-9]*.[0-9]*.[0-9]*) ;;
        *) die "'${version}' is not a MAJOR.MINOR.PATCH version; refusing to write it anywhere" ;;
    esac
    printf '%s\n' "${version}" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$' \
        || die "'${version}' is not a valid SemVer version"
}

# --- writing -----------------------------------------------------------------------------------

# Every write goes through a temporary file in the same directory followed by a rename, so a failure
# half-way cannot leave a truncated Cargo.toml behind.
#
# The producer's output is written to the temporary file first and its exit status checked before the
# rename: if the rewrite failed, the original file is untouched rather than half-replaced.
#
# The temporary file is created by `mktemp`, which means it starts at mode 0600. The original mode is
# copied over before the rename (GNU `chmod --reference` where it exists, 0644 otherwise) because a
# source file that quietly becomes unreadable to the rest of the toolchain is a worse outcome than the
# version bump it was trying to make.
replace_file() {
    local file="$1" producer="$2" version="$3" tmp
    tmp="$(mktemp "${file}.XXXXXX")"
    # shellcheck disable=SC2064 # the path is expanded now, on purpose
    trap "rm -f '${tmp}'" EXIT
    "${producer}" "${file}" "${version}" > "${tmp}" \
        || die "could not rewrite ${file}; it was left untouched"
    chmod --reference="${file}" "${tmp}" 2>/dev/null || chmod 0644 "${tmp}"
    mv "${tmp}" "${file}"
    trap - EXIT
}

# A pure filter: file in, rewritten file out. Only the first `"version": "x"` is touched, and
# everything after the closing quote (a trailing comma, a trailing comment) is preserved as it was.
render_json_version() {
    awk -v version="$2" '
        !done && /^[[:space:]]*"version"[[:space:]]*:/ {
            line = $0
            match(line, /^[[:space:]]*"version"[[:space:]]*:[[:space:]]*"/)
            prefix = substr(line, 1, RLENGTH)
            rest = substr(line, RLENGTH + 1)
            sub(/^[^"]*"/, "", rest)
            print prefix version "\"" rest
            done = 1
            next
        }
        { print }
    ' "$1"
}

write_json_version() {
    local file="$1" version="$2"
    assert_semver "${version}"
    replace_file "${file}" render_json_version "${version}"
}

# The same, for the one `version` line under `[workspace.package]`. The section tracking is what keeps
# `[workspace.dependencies]`'s `version = "2"` lines out of reach.
render_cargo_workspace_version() {
    awk -v version="$2" '
        /^[[:space:]]*\[/ {
            section = $0
            sub(/^[[:space:]]*\[/, "", section)
            sub(/\][[:space:]]*(#.*)?$/, "", section)
            print
            next
        }
        !done && section == "workspace.package" && /^[[:space:]]*version[[:space:]]*=/ {
            line = $0
            match(line, /^[[:space:]]*version[[:space:]]*=[[:space:]]*"/)
            prefix = substr(line, 1, RLENGTH)
            rest = substr(line, RLENGTH + 1)
            sub(/^[^"]*"/, "", rest)
            print prefix version "\"" rest
            done = 1
            next
        }
        { print }
    ' "$1"
}

write_cargo_workspace_version() {
    local file="$1" version="$2"
    assert_semver "${version}"
    replace_file "${file}" render_cargo_workspace_version "${version}"
}

# --- modes -------------------------------------------------------------------------------------

# Prints the three versions and the files they were read from — the shape a reader needs when the
# check fails.
report() {
    local tauri cargo package
    tauri="${1}"
    cargo="${2}"
    package="${3}"
    {
        printf '  crates/mirk-desktop/tauri.conf.json  %s   (source of truth)\n' "${tauri:-<missing>}"
        printf '  Cargo.toml [workspace.package]      %s\n' "${cargo:-<missing>}"
        printf '  package.json                        %s\n' "${package:-<missing>}"
    } >&2
}

mode_check() {
    local tauri cargo package
    tauri="$(canonical_version)"
    cargo="$(read_cargo_workspace_version "${cargo_toml}")"
    package="$(read_json_version "${package_json}")"

    if [ "${tauri}" = "${cargo}" ] && [ "${tauri}" = "${package}" ]; then
        printf 'version-sync: %s agrees in all three files\n' "${tauri}"
        return 0
    fi

    {
        echo "version-sync: the version disagrees between files"
        report "${tauri}" "${cargo}" "${package}"
        echo
        echo "Fix it by running scripts/version-sync.sh (it writes tauri.conf.json's version into the other two),"
        echo "or by changing crates/mirk-desktop/tauri.conf.json first if the version itself is wrong."
    } >&2
    return 1
}

mode_tag() {
    local tag="${1:-}" version normalized
    [ -n "${tag}" ] || die "--tag needs a tag name, for example: --tag v0.1.0"
    version="$(canonical_version)"
    normalized="${tag#v}"

    if [ "${normalized}" = "${version}" ]; then
        printf 'version-sync: tag %s matches crates/mirk-desktop/tauri.conf.json (%s)\n' "${tag}" "${version}"
        return 0
    fi

    {
        echo "version-sync: the tag does not match the version in the tree"
        echo "  tag                                  ${tag} (compared as ${normalized})"
        echo "  crates/mirk-desktop/tauri.conf.json  ${version}"
        echo
        echo "A release must not publish a binary whose --version disagrees with the tag. Either move the tag,"
        echo "or bump crates/mirk-desktop/tauri.conf.json and run scripts/version-sync.sh, commit, and re-tag."
    } >&2
    return 1
}

mode_sync() {
    local version cargo package
    version="$(canonical_version)"
    assert_semver "${version}"
    cargo="$(read_cargo_workspace_version "${cargo_toml}")"
    package="$(read_json_version "${package_json}")"

    if [ "${cargo}" = "${version}" ]; then
        printf 'version-sync: Cargo.toml already at %s\n' "${version}"
    else
        write_cargo_workspace_version "${cargo_toml}" "${version}"
        printf 'version-sync: Cargo.toml [workspace.package] %s -> %s\n' "${cargo:-<missing>}" "${version}"
    fi

    if [ "${package}" = "${version}" ]; then
        printf 'version-sync: package.json already at %s\n' "${version}"
    else
        write_json_version "${package_json}" "${version}"
        printf 'version-sync: package.json %s -> %s\n' "${package:-<missing>}" "${version}"
    fi
}

main() {
    case "${1:-}" in
        "") mode_sync ;;
        --check) mode_check ;;
        --print) canonical_version ;;
        --tag) mode_tag "${2:-}" ;;
        -h | --help) usage ;;
        *) die "unknown argument '${1}'; try --help" ;;
    esac
}

usage() {
    cat <<'USAGE'
scripts/version-sync.sh — keep the version in one place

Source of truth: crates/mirk-desktop/tauri.conf.json (the file the Linux packagers read).

  (no argument)          write that version into Cargo.toml [workspace.package] and package.json
  --check                exit 1 when the three disagree; write nothing
  --print                print the canonical version
  --tag <ref>            exit 1 unless <ref> (with or without a leading "v") matches it
  -h, --help             this text

`--check` runs in CI on every change; `--tag "$GITHUB_REF_NAME"` runs in the release workflow.
USAGE
}

main "$@"
