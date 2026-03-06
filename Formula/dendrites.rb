class Dendrites < Formula
  desc "Domain Model Context Protocol Server — architectural meta-layer for GitHub Copilot"
  homepage "https://github.com/flavioaiello/dendrites"
  license "MIT"
  url "https://github.com/flavioaiello/dendrites/archive/refs/tags/v0.1.1.tar.gz"
  sha256 "157ab2aa6593486b998a97c4a852a3a71c636575ef4c8469dc377f0fe7f313a2"
  version "0.1.1"

  head "https://github.com/flavioaiello/dendrites.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
    # Binary is named dendrites
  end

  def post_install
    # Ensure the data directory exists
    (var/"dendrites").mkpath
  end

  def caveats
    <<~EOS
      Dendrites stores domain models in ~/.dendrites/dendrites.db (SQLite).

      To use with VS Code / GitHub Copilot, add to .vscode/mcp.json:

        {
          "servers": {
            "dendrites": {
              "type": "stdio",
              "command": "dendrites",
              "args": ["serve", "--workspace", "${workspaceFolder}"]
            }
          }
        }

      To import an existing dendrites.json:

        dendrites import dendrites.json --workspace /path/to/your/project

      To list all stored projects:

        dendrites list
    EOS
  end

  test do
    # Verify the binary starts and prints usage
    output = shell_output("#{bin}/dendrites 2>&1", 1)
    assert_match "dendrites", output
  end
end
