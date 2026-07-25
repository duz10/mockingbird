# Homebrew tap setup (macOS distribution)

This documents how the maintainer serves the Mockingbird Homebrew **cask**
so Mac users can `brew install --cask` the app. Note that Homebrew (6.x)
**adds** `com.apple.quarantine` on cask install -- it does **not** strip
it, and a cask cannot opt out (there is no `quarantine false` stanza in
current Homebrew). Since the `.app` is ad-hoc signed (not notarized),
a plain install still hits a one-time Gatekeeper prompt; set
`HOMEBREW_CASK_OPTS="--no-quarantine"` for the install to skip it (current
Homebrew no longer accepts a bare `--no-quarantine` flag on `brew
install`). The cask still beats a raw `.dmg` download: it wires the
download URL + sha256 verification, the macOS-version guard, and clean
uninstall/zap.

> **Prerequisite:** a published `.dmg` release. The cask
> (`Casks/mockingbird.rb`) is a scaffold with **placeholder `version` +
> `sha256`** until the CI macOS lane cuts the first `.dmg` on a `v*.*.*`
> tag (see `.github/workflows/release.yml`). You cannot finalize the cask
> before that artifact exists.

## Option A -- serve the tap from THIS repo (simplest)

Homebrew treats any repo with a top-level `Casks/` directory as a tap.
The cask already lives at `Casks/mockingbird.rb`, so no second repo is
needed. Users tap + install with:

```bash
brew tap duz10/mockingbird https://github.com/duz10/mockingbird
brew install --cask duz10/mockingbird/mockingbird
```

Or, once tapped, simply `brew install --cask mockingbird`.

**Pros:** one repo, cask versioned next to the release workflow.
**Cons:** every `brew tap` clones the whole app repo (heavier than a
dedicated tap repo).

## Option B -- dedicated `homebrew-mockingbird` tap repo

Create a repo named exactly `homebrew-mockingbird` (the `homebrew-`
prefix is what makes `brew tap duz10/mockingbird` resolve). Put the cask
at `Casks/mockingbird.rb` there. Users:

```bash
brew tap duz10/mockingbird          # resolves to github.com/duz10/homebrew-mockingbird
brew install --cask mockingbird
```

**Pros:** lightweight clone, canonical Homebrew layout.
**Cons:** a second repo to keep in sync with each release.

Keep `Casks/mockingbird.rb` in THIS repo as the source of truth either
way; for Option B, copy it into the tap repo at release time.

## Updating the cask after each release

The cask cannot be finalized until the first `.dmg` exists. After every
release:

1. **`version`** -> the release tag without the leading `v`, matching the
   `version` in `src-tauri/tauri.conf.json` (tauri stamps that config
   version -- not the git tag -- into the `.dmg` filename). Keep tag and
   config version in sync at release time.
2. **`sha256`** -> the real hash of the published asset:
   ```bash
   shasum -a 256 Mockingbird_<version>_aarch64.dmg
   # or, after tapping:
   brew fetch --cask mockingbird   # then read the cached download's hash
   ```
3. Commit + push the cask (this repo for Option A, or the tap repo for
   Option B).

The `url` in the cask,
`.../releases/download/v#{version}/Mockingbird_#{version}_aarch64.dmg`,
already matches tauri v2's dmg naming
(`<productName>_<version>_<arch>.dmg`), so only `version` + `sha256`
change per release.

## Validating locally before publishing

```bash
brew style Casks/mockingbird.rb     # lint
brew audit --cask --new Casks/mockingbird.rb
brew install --cask ./Casks/mockingbird.rb   # only works once url+sha256 are real
```
