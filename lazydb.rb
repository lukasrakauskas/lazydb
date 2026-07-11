class Lazydb < Formula
  desc "Terminal database client that loads instantly, connects securely, and gets out of your way"
  homepage "https://github.com/lukasrakauskas/lazydb"
  license "MIT OR Apache-2.0"
  head "https://github.com/lukasrakauskas/lazydb.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/lazydb --version")
  end
end
