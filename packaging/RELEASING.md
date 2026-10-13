# Releasing mac-rdp-server

F-110.6. Written down so a release is a procedure, not a memory: tag, let CI
build, copy one file into the tap.

## 0. Before the tag

- CI green on `main` (`fmt`, `clippy -D warnings`, `test`, `cargo check --locked`).
- `version` in `Cargo.toml` bumped **in a commit that does nothing else**, and
  `Cargo.lock` regenerated and committed (`cargo check --locked` would fail
  otherwise). The manifest is the only source of truth for the number
  (F-110.16).
- **`CHANGELOG.md` has a section for this version** with today's date. The
  release notes are taken from it, so a missing section stops the release
  (F-110.17). Check it without tagging:

  ```sh
  packaging/release-notes.sh 0.1.0
  ```

- `README.md` → "What does not work yet" still honest for this version.
- The manual acceptance criteria of whatever the release contains have actually
  been run. For wave 5 that is F-110 AC-1, AC-4, AC-7.

## 1. Tag and push

```sh
git tag -a v0.1.0 -m "v0.1.0"
git push origin v0.1.0
```

That starts `.github/workflows/release.yml`, which:

1. refuses the tag if it disagrees with `version` in `Cargo.toml`, or if
   `CHANGELOG.md` has no section for it;
2. builds `aarch64-apple-darwin` natively and `x86_64-apple-darwin`
   cross-compiled on the same runner;
3. ad-hoc signs both binaries (`codesign --sign -`), which is what an arm64
   binary needs in order to run at all;
4. packs each one flat with the example configuration, `README.md` and
   `SECURITY.md`;
5. renders the formula with `packaging/homebrew/render-formula.sh`;
6. creates the GitHub Release with the two tarballs and
   `mac-rdp-server.rb` attached.

## 1.5. First time only: create the tap

The tap is a repository of its own, and it exists once. Everything it needs is
in this repository, so creating it is a copy:

```sh
gh repo create DawidAdamski/homebrew-mac-rdp-server --public \
  --description "Homebrew tap for mac-rdp-server"

cd ..
git clone https://github.com/DawidAdamski/homebrew-mac-rdp-server.git
cd homebrew-mac-rdp-server
mkdir -p Formula
cp ../rdp-server-macos/packaging/homebrew/tap-README.md README.md
gh release download v0.1.0 --repo DawidAdamski/mac-rdp-server \
  --pattern 'mac-rdp-server.rb' --dir Formula
git add -A && git commit -m "mac-rdp-server 0.1.0" && git push
```

The layout matches the owner's other taps (`homebrew-bsi`,
`homebrew-floci-podman`): the formula lives in `Formula/`, and the README
carries the install command and the warning about authentication.

Afterwards, `brew install DawidAdamski/mac-rdp-server/mac-rdp-server` works
for anyone, and every later release only replaces one file — section 2.

## 2. Update the tap

The tap is a separate repository, `DawidAdamski/homebrew-mac-rdp-server`, with
one file in it: `Formula/mac-rdp-server.rb`.

```sh
gh release download v0.1.0 --repo DawidAdamski/mac-rdp-server --pattern 'mac-rdp-server.rb' --dir /tmp
cd ../homebrew-mac-rdp-server
cp /tmp/mac-rdp-server.rb Formula/mac-rdp-server.rb
git commit -am "mac-rdp-server 0.1.0" && git push
```

The rendered file carries a "GENERATED" header: the source of truth is
`packaging/homebrew/mac-rdp-server.rb` in this repository, and an edit made in
the tap is lost at the next release.

## 3. Verify, as a user would

```sh
brew update
brew install DawidAdamski/mac-rdp-server/mac-rdp-server
mac-rdp-server --version          # must match the tag
mac-rdp-server --check-permissions
brew audit --strict --online DawidAdamski/mac-rdp-server/mac-rdp-server
brew test DawidAdamski/mac-rdp-server/mac-rdp-server
```

And once per release, on a machine that has never had Rust on it (F-110 AC-4):

```sh
xattr -l "$(readlink -f "$(which mac-rdp-server)")"   # must NOT list com.apple.quarantine
```

## 4. Where the release notes come from

`CHANGELOG.md`, not this workflow and not a hand-written box (F-110.17). The
section for the version being released is extracted by
`packaging/release-notes.sh`, which also appends the one line that is not
optional, because every upgrading user hits it (F-110.13): both permissions
have to be granted again, and the stale entry removed with the minus button.

So the only thing to write before a release is the changelog section. Preview
exactly what will be published:

```sh
packaging/release-notes.sh 0.1.0
```

A version with no section there fails the release rather than publishing an
empty description, and CI checks both directions on every push.

## Doing it by hand, without CI

```sh
V=0.1.0
for target in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --release --locked --target "$target"
    bin="target/$target/release/mac-rdp-server"
    codesign --force --sign - "$bin"
    staging="$(mktemp -d)"
    cp "$bin" packaging/mac-rdp-server.toml README.md SECURITY.md "$staging/"
    tar -czf "mac-rdp-server-$V-$target.tar.gz" -C "$staging" .
    rm -rf "$staging"
done

packaging/homebrew/render-formula.sh "$V" \
    "mac-rdp-server-$V-aarch64-apple-darwin.tar.gz" \
    "mac-rdp-server-$V-x86_64-apple-darwin.tar.gz" > /tmp/mac-rdp-server.rb

gh release create "v$V" --title "v$V" --generate-notes \
    "mac-rdp-server-$V-aarch64-apple-darwin.tar.gz" \
    "mac-rdp-server-$V-x86_64-apple-darwin.tar.gz" \
    /tmp/mac-rdp-server.rb
```

## Known rough edges

- **The Intel binary is cross-compiled and untested.** GitHub's Intel macOS
  runners are gone, so `x86_64-apple-darwin` is built on an Apple Silicon
  runner and nobody runs it before release. If it turns out broken, drop the
  `on_intel` block from the formula rather than shipping a binary that
  crashes.
- **No notarisation, by decision** (F-110 §3, `waves.md`). A binary installed
  by a formula carries no quarantine attribute, so it runs; the cost is the
  permission re-grant in §4.
- **Building both targets by hand needs rustup, not Homebrew's `rust`.**
  Homebrew's `rust` ships the standard library for the host only, so
  `cargo build --target x86_64-apple-darwin` fails on a machine without
  rustup. The CI runner uses rustup and adds both targets.
- **The source path is `--HEAD` only.** The stable tarball holds a binary, not
  sources, so `brew install --build-from-source` cannot build it. Verifying
  F-110.3 means `brew install --HEAD`.
