class Dendrites < Formula
  desc "Domain Model Context Protocol Server — architectural meta-layer for GitHub Copilot"
  homepage "https://github.com/flavioaiello/dendrites"
  license "MIT"
  url "https://github.com/flavioaiello/dendrites/archive/refs/tags/v0.1.1.tar.gz"
  sha256 "8b472a402b558332848a89106419798198697b0e000d10be073cb04dd959d94d"
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

      To export the actual model:

        dendrites export model.json --workspace /path/to/project --state actual

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
