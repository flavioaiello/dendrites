class Dendrites < Formula
  desc "Domain Model Context Protocol Server — architectural meta-layer for GitHub Copilot"
  homepage "https://github.com/flavioaiello/dendrites"
  license "MIT"
  url "https://github.com/flavioaiello/dendrites/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "5c8d3c4078d66fbf157bcb6d5e6f70ad8d6cd005962f84d3796a9ac911e3ab5b"
  version "0.1.0"

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
