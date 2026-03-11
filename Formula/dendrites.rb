class Dendrites < Formula
  desc "Domain Model Context Protocol Server — architectural meta-layer for GitHub Copilot"
  homepage "https://github.com/flavioaiello/dendrites"
  license "MIT"
  url "https://github.com/flavioaiello/dendrites/archive/refs/tags/v0.1.4.tar.gz"
  sha256 "c37295055209fec0b257d478f4167cabe9040845320ce94a8c4c0b1c135cea83"
  version "0.1.4"

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
      Dendrites stores domain models per-crate in <crate_root>/.dendrites/store.db (SQLite).

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
