# Homebrew formula for mac-rdp-server (F-110).
#
# This file is the source of truth and lives in the main repository; the copy in
# the tap (DawidAdamski/homebrew-mac-rdp-server) is generated from it. Three
# tokens are filled in at release time by `render-formula.sh`:
# @@VERSION@@, @@SHA256_ARM@@, @@SHA256_INTEL@@.
#
# Two install paths, deliberately (F-110.2):
#
#   brew install mac-rdp-server          prebuilt binary from the release
#   brew install --HEAD mac-rdp-server   builds from source, needs Rust
#
# The prebuilt path is the default because it needs no toolchain and no wait,
# and because a binary installed by a *formula* carries no quarantine attribute
# — that is Homebrew Cask's doing, not a formula's — so an ad-hoc signature is
# enough and notarisation is not needed. Measured, F-110 §3.

class MacRdpServer < Formula
  desc "RDP server for macOS: share the Mac desktop with any RDP client"
  homepage "https://github.com/DawidAdamski/mac-rdp-server"
  license any_of: ["MIT", "Apache-2.0"]

  head do
    url "https://github.com/DawidAdamski/mac-rdp-server.git", branch: "main"
    depends_on "rust" => :build
  end

  # macOS 14 (Sonoma) is the floor: the ScreenCaptureKit APIs used to resolve
  # the Retina backing-store size are gated on 14.0 (F-002.5, ADR-0002).
  depends_on macos: :sonoma

  on_macos do
    on_arm do
      url "https://github.com/DawidAdamski/mac-rdp-server/releases/download/v@@VERSION@@/mac-rdp-server-@@VERSION@@-aarch64-apple-darwin.tar.gz"
      sha256 "@@SHA256_ARM@@"
    end
    on_intel do
      url "https://github.com/DawidAdamski/mac-rdp-server/releases/download/v@@VERSION@@/mac-rdp-server-@@VERSION@@-x86_64-apple-darwin.tar.gz"
      sha256 "@@SHA256_INTEL@@"
    end
  end

  # F-110.3. `install` runs with no network access, and `std_cargo_args` adds
  # `--offline` whenever a formula defines `fetch`. Our Cargo.lock points at a
  # git dependency — the IronRDP fork, ADR-0014 — so that dependency has to be
  # pulled in this phase or the offline build fails on it. `cargo fetch` writes
  # into CARGO_HOME, which Homebrew sets to a cache shared between the fetch
  # and build phases (`Library/Homebrew/package_manager_cache.rb:54`).
  def fetch
    return unless build.head?

    system "cargo", "fetch", *std_cargo_fetch_args
  end

  def install
    if build.head?
      system "cargo", "install", *std_cargo_args
      (pkgshare/"examples").install "packaging/mac-rdp-server.toml"
    else
      # The release tarball is flat: the binary, the example configuration and
      # the two documents, nothing else. Kept in step with release.yml.
      bin.install "mac-rdp-server"
      (pkgshare/"examples").install "mac-rdp-server.toml"
    end

    doc.install "README.md", "SECURITY.md"
  end

  def caveats
    <<~EOS
      THIS SERVER HAS NO AUTHENTICATION. Anything that reaches the port gets
      this Mac's screen, mouse and keyboard. It listens on 127.0.0.1 by
      default, which keeps it on this machine. Read the security notes before
      changing that, and never forward the port from the internet:
        https://github.com/DawidAdamski/mac-rdp-server/blob/main/SECURITY.md

      Two macOS permissions have to be granted by hand, in
      System Settings -> Privacy & Security:
        - Screen & System Audio Recording  (to capture the screen)
        - Accessibility                    (to move the mouse and type)

      Grant them to the application you start the server FROM — Terminal or
      iTerm — not to a file called mac-rdp-server: a command-line tool inherits
      the permissions of whatever launched it. Then restart the server; TCC
      changes never reach a running process.

      Check what is granted, and what to click for what is not:
        mac-rdp-server --check-permissions

      AFTER EVERY `brew upgrade` BOTH PERMISSIONS MUST BE GRANTED AGAIN.
      macOS keys a permission to the exact binary it was granted to, and each
      version is installed in its own directory, so the new build is a
      different program as far as the system is concerned. The old entry stays
      in the list and can still look switched on: select it, remove it with the
      minus button, add the current one, restart the server. The same thing
      happens when your terminal application updates itself.

      Configuration is optional; pass a TOML file with --config. The TLS
      certificate is generated on first run in
        ~/Library/Application Support/mac-rdp-server/
      An example configuration to copy and edit:
        cp #{opt_pkgshare}/examples/mac-rdp-server.toml ~/.config/mac-rdp-server.toml
    EOS
  end

  test do
    # Everything here is deliberately offline, needs no TCC grant and opens no
    # port: a formula test that needs any of those is a flaky formula.
    assert_match "mac-rdp-server #{version}", shell_output("#{bin}/mac-rdp-server --version")

    # The permission-checking mode exists and is documented (F-110.15). Its
    # exit code depends on what the machine running `brew test` has granted, so
    # only the help text is asserted on.
    assert_match "--check-permissions", shell_output("#{bin}/mac-rdp-server --help")

    # A missing configuration file is reported rather than ignored.
    output = shell_output("#{bin}/mac-rdp-server --config #{testpath}/absent.toml 2>&1", 1)
    assert_match "reading config file", output
  end
end
