# frozen_string_literal: true

class Slink < Formula
  desc "Unified cross-platform CLI for symlinks, hardlinks, junctions and bind mounts"
  homepage "https://github.com/piyush/smartlink"
  url "https://github.com/piyush/smartlink/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "REPLACE_WITH_SHA256_OF_THE_SOURCE_TARBALL"
  license "MIT"

  depends_on "rust" => :build

  def install
    # The package is `smartlink`; it installs the shorter `slink` binary.
    system "cargo", "install", "--no-track", "--root", prefix, "--path", "."
  end

  test do
    assert_match version.to_s, shell_out("#{bin}/slink --version").stdout
    # Exercise a real hardlink round-trip in a temp directory.
    (testpath/"orig.txt").write("payload")
    system bin/"slink", "make", "orig.txt", "copy.txt"
    assert_equal "payload", (testpath/"copy.txt").read
  end
end
