# Contributing to mirk

Thanks for looking. This is a small, opinionated project, and the fastest way to get a change accepted
is to match the conventions below — they are what the repository's own history looks like.

## What mirk is, in one paragraph

A desktop application that tracks reading progress in PDFs. The plan, every locked decision and the
reasoning behind them live in [`odd/tasks/pdf-progress-tracker.md`](odd/tasks/pdf-progress-tracker.md);
that document is the project's memory and is expected to be read before a non-trivial change.

## Before you write code

1. **Read the feature record.** Larger changes live in `odd/tasks/<feature>.md`, with numbered specs
   (`S1`, `S2`…), a task table and a log of decisions in the reader's own words. If your change
   contradicts a spec, say so in the issue instead of silently diverging — the project records
   corrections, and several specs in there carry a `*(Corrected during …)*` note.
2. **Open an issue** for anything larger than a bug fix. Small fixes with a test are welcome as a PR
   without ceremony.

## Building and testing

```bash
bun install
scripts/fetch-pdfium.sh          # once per clone; ~8 MB, gitignored
cargo test --workspace
bun run typecheck && bun run test
```

Page counting loads the native `pdfium` at runtime, so the tests that prove real counting **fail**
without it rather than skipping — that is deliberate. On Linux, Tauri also needs
`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev` and `libxdo-dev`.

The interface has a capture harness that an agent (or you) can use instead of taking screenshots by
hand: `node scripts/capture-ui.mjs`. It seeds a small library, opens Chrome headless at the window size
each layout pins, and exits non-zero on any page exception or `console.error` — a screenshot of a page
that failed to load is not evidence about design.

Bundling a package locally (`bun run tauri build`) signs update artifacts, so it needs the private key:

```bash
export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/mirk.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="…"    # the key's own password
bun run tauri build            # or, with no key at all:
bun run tauri build --config '{"bundle":{"createUpdaterArtifacts":false}}'
```

Without either, the build produces the `.deb` and then exits 1 with *"A public key has been found, but no
private key"*. That is deliberate on the bundler's part and worth knowing before you assume your change
broke something.

## What a good change looks like

- **A test that fails first.** For a behaviour change, write the test, watch it fail for the reason you
  think you are fixing, then fix it. Quote the red in the pull request; a test that never failed proves
  nothing about the defect.
- **Tests that assert observable behaviour**, not internal state: given this input, this is what a
  person sees, and this is what the store holds. A test coupled to private fields breaks on every
  refactor and catches nothing.
- **No invented facts.** "I could not read the file" and "the file is empty" are different answers
  throughout this codebase, and the types say so. Prefer an honest error over a plausible zero.
- **The interface's words live in the catalogue** (`web/src/lib/i18n.ts`), in English and Spanish. No
  string literals in components.

## Gates your PR has to pass

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo test --workspace
bun run typecheck        # tsc + svelte-check, 0 errors and 0 warnings
bun run test
bun run build
```

CI runs the same gates, plus a compile matrix for Windows and macOS (their backends are stubs today, so
only compilation is checked there) and the capture smoke test.

## Commits and pull requests

- [Conventional Commits](https://www.conventionalcommits.org/): `feat(ui): …`, `fix(parser): …`,
  `docs(record): …`. The scope is the area you touched.
- Keep a unit reviewable. Behaviour, its tests and the documentation of it land together; a PR far past
  ~400 authored lines is asked to split, and features bigger than that are chained.
- Say what you verified and what you could not. "Verified on the running application" and "not verified
  on WebKitGTK" are both useful sentences; neither is embarrassing.

## Releases and the update key

A release is a tag plus a human decision. Pushing `v<version>` runs `.github/workflows/release.yml`, which
builds `.deb`, `.rpm` and `.AppImage`, creates a **draft** release with those plus the GPL-3.0 source
tarball, `THIRD-PARTY-NOTICES.md` and `SHA256SUMS`, and stops. Publishing is a click a maintainer makes
after reading the draft: a tag is cheap to push and a published release is not cheap to un-publish.

mirk updates itself, and that needs a signing key pair.

- **The public key** lives in `crates/mirk-desktop/tauri.conf.json` under `plugins.updater.pubkey` — the
  base64 of the whole `.pub` file, comment line included. It ships inside every copy that is installed,
  which is exactly why it cannot be changed later.
- **The private key** never enters this repository, an issue, or a chat. It exists as two repository secrets:
  `TAURI_SIGNING_PRIVATE_KEY` (the file's contents) and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (the password the
  file was generated with — mirk's key has one).

```bash
bun tauri signer generate -w ~/.tauri/mirk.key      # once; it asks for a password
gh secret set TAURI_SIGNING_PRIVATE_KEY < ~/.tauri/mirk.key
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD    # interactive: `gh` reads it without echoing
```

Both secrets are needed when the key has a password — mirk's does. `gh secret set NAME` with no value prompts
for it, which keeps the password out of your shell history; `--body` would put it there. `gh secret list` shows
the names and when each was last updated, never the values.

**Back the private key up, in two places, before the first release — it is part of the product.** The
public key is baked into the copies already installed, so a lost private key means those copies cannot
verify any future release, and no later version can repair that: the only remedy is asking every reader to
download a new build by hand. A leaked private key is a rotation, and a rotation has the same consequence
for installed copies — generate a new pair, replace the public key, and say so in the release notes.

`bundle.createUpdaterArtifacts` is on, so a signed build produces a `.sig` per bundle and the workflow
publishes `latest.json` with it. The release is deliberately **not** marked pre-release: GitHub serves
`/releases/latest/download/latest.json` from the newest release that is neither a draft nor a pre-release,
so a marked one would be invisible to the updater. `0.x` already says pre-1.0, and the release body says
pre-alpha in words.

## Licensing

mirk is **GPL-3.0**. By contributing you agree your work is licensed under the same terms. New
dependencies must be GPL-3.0 compatible: permissive licences are fine (MIT, Apache-2.0, BSD, ISC,
Zlib), MPL-2.0 is fine, and anything copyleft-incompatible is not. Add what you introduce to
[`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md).
