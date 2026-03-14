<p align="center">
  <img src="dendrites.svg" alt="dendrites" width="420"/>
</p>

<p align="center">
  <strong>Architectural meta-layer for GitHub Copilot</strong><br/>
  <em>Repo-grounded symbolic reasoning over software architecture via the Model Context Protocol</em>
</p>

<p align="center">
  <a href="https://github.com/flavioaiello/dendrites/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-7c3aed" alt="MIT License"/></a>
  <img src="https://img.shields.io/badge/rust-2024_edition-f97316" alt="Rust 2024"/>
  <img src="https://img.shields.io/badge/MCP-2025--03--26-14b8a6" alt="MCP Spec"/>
</p>

---

## What is dendrites?

**dendrites** is an MCP server that gives AI coding agents — like GitHub Copilot — a formal understanding of your software architecture. Instead of letting agents guess about your codebase structure, dendrites extracts facts from source code, stores them as machine-checkable relations in an embedded Datalog engine ([CozoDB](https://www.cozodb.org/)), and answers architectural questions with **proof-carrying results**.

It maintains two parallel views of your architecture:

| View | Source | Purpose |
|:-----|:-------|:--------|
| **Desired** | Declared by you through the agent | The architecture you intend to build |
| **Actual** | Extracted from AST scanning | What the code actually does |

The diff between these two views drives refactoring, violation detection, and impact analysis — all grounded in facts, never hallucinated.

## Key capabilities

- **Polyglot AST extraction** — Rust (via `syn`), Python, TypeScript/TSX, Go (via tree-sitter)
- **Datalog reasoning** — Transitive dependencies, cycle detection, layer violations, blast radius, dead code analysis
- **Architectural invariants** — Define and enforce layer rules, bounded context boundaries, and dependency constraints
- **Impact analysis** — Compute the blast radius of any change before making it
- **Safe deletion** — Proof-backed answers to "can I delete this?" with witness references
- **Live file watching** — Background watcher keeps the actual model in sync as you code
- **Multi-crate workspaces** — Isolated per-project databases with workspace member support

## Installation

### Homebrew (macOS)

```bash
brew tap flavioaiello/dendrites https://github.com/flavioaiello/dendrites
brew install dendrites
```

### From source

```bash
git clone https://github.com/flavioaiello/dendrites.git
cd dendrites
cargo install --path .
```

## Setup with VS Code / GitHub Copilot

Add to `.vscode/mcp.json` in your project:

```json
{
  "servers": {
    "dendrites": {
      "type": "stdio",
      "command": "dendrites",
      "args": ["serve", "--workspace", "${workspaceFolder}"]
    }
  }
}
```

Once configured, Copilot gains access to all dendrites tools, resources, and prompts automatically.

## MCP tools

### Read tools

| Tool | Description |
|:-----|:------------|
| `get_model` | Returns both desired and actual models with sync status and pending changes |
| `model_health` | Structured health report via Datalog — score (0–100), cycles, violations, complexity |
| `query_blast_radius` | Downstream impact analysis: transitive deps, cycles, layer violations, field usage |
| `can_delete_symbol` | Proof-backed safe-deletion check with inbound reference witnesses |
| `check_architectural_invariant` | Evaluate invariants: layer violations, cycles, aggregate quality, orphans |
| `query_dependency_path` | Return proof paths between any two architectural entities |
| `explain_violation` | Evidence-backed explanation with witness paths for any violation |
| `diff_models` | Compare desired vs actual — added/removed entities and pending changes |

### Write tools

| Tool | Description |
|:-----|:------------|
| `set_model` | Create, update, or remove model elements (contexts, entities, services, events, etc.) |
| `scan_model` | AST-scan workspace source code and populate the actual model |
| `refactor_model` | Refactoring lifecycle: `plan` (diff), `accept` (promote), `reset` (discard) |
| `assert_model` | Declare constraints: layer assignments, allowed/forbidden dependencies |

### Resources

| URI | Content |
|:----|:--------|
| `dendrites://architecture/overview` | All bounded contexts, entities, and rules |
| `dendrites://architecture/rules` | Architectural constraints |
| `dendrites://architecture/conventions` | Naming, structure, and testing conventions |
| `dendrites://context/{name}` | Per-bounded-context details |

## CLI

```
dendrites [command] [options]
```

| Command | Description |
|:--------|:------------|
| `serve` | Start the MCP stdio server with background file watcher (default) |
| `export <file>` | Export domain model to JSON (`--state desired\|actual\|both`) |
| `list` | Show all crates and their model status |
| `check` | Verify workspace semantics (layer violations, cycles) |
| `scan` | AST-scan a workspace and populate the actual model |

All commands accept `--workspace <path>` (defaults to current directory).

## How it works

```
┌─────────────────┐     stdio/JSON-RPC     ┌──────────────────┐
│  GitHub Copilot  │◄─────────────────────►│    dendrites     │
│   (or any MCP    │                        │    MCP Server    │
│     client)      │                        └────────┬─────────┘
└─────────────────┘                                  │
                                          ┌──────────┼──────────┐
                                          │          │          │
                                     ┌────▼───┐ ┌───▼────┐ ┌───▼───┐
                                     │ Domain │ │ Store  │ │Server │
                                     │ Module │ │(CozoDB)│ │Module │
                                     └────┬───┘ └───┬────┘ └───┬───┘
                                          │         │          │
                                     AST Scan   Datalog    File
                                     (syn +     Rules &    Watcher
                                     tree-      Relations  (notify)
                                     sitter)
```

1. **Ingest** — AST scanners extract structural facts (functions, types, imports, calls) from source code
2. **Store** — Facts are normalized into ~30 CozoDB relations with full provenance
3. **Reason** — Datalog rules derive transitive dependencies, cycles, violations, and blast radius
4. **Expose** — MCP tools return proof-carrying results with witness paths and source locations
5. **Watch** — Background file watcher keeps the actual model in sync (2-second debounce)

## Architecture concepts

### Bounded contexts

dendrites organizes code into **bounded contexts** — the core building block of domain-driven design. Each context contains:

- **Entities** — Domain objects with identity, fields, methods, and invariants
- **Value objects** — Immutable objects with validation rules
- **Services** — Application, domain, and infrastructure services
- **Domain events** — Events published by entities
- **Repositories** — Aggregate persistence
- **Policies** — Process managers and domain policies

### First-class relations

Sub-structures (fields, methods, parameters, invariants) are stored as **independent CozoDB relations**, not nested JSON. This enables cross-cutting Datalog queries that would be impossible with flat document storage.

### Proof-carrying results

Every reasoning tool returns structured results with:

- **status** — `true`, `false`, or `unknown`
- **proof** — Witness paths and supporting edges
- **evidence** — Source files and line spans
- **limitations** — Explicit uncertainty (dynamic dispatch, reflection, partial ingestion)

The system never guesses. If it can't prove a claim, it returns `unknown`.

## Example tool outputs

### `model_health`

```json
{
  "score": 85,
  "circular_deps": [],
  "layer_violations": [],
  "missing_invariants": [["Catalog", "Category"]],
  "orphan_contexts": ["Notifications"],
  "god_contexts": [],
  "unsourced_events": [],
  "complexity": [
    { "context": "Catalog", "entity_count": 3, "service_count": 2, "event_count": 2, "dep_count": 0 },
    { "context": "Ordering", "entity_count": 2, "service_count": 1, "event_count": 1, "dep_count": 1 }
  ]
}
```

### `can_delete_symbol`

```json
{
  "can_delete": false,
  "aggregates_referencing": [],
  "events_sourced": ["OrderPlaced", "OrderCancelled"],
  "repositories_managing": ["OrderRepository"],
  "import_references": [],
  "ast_references": [],
  "call_references": [
    { "caller": "process_payment", "file": "src/billing/service.rs", "line": 42 }
  ]
}
```

### `diff_models`

```json
{
  "status": "diverged",
  "pending_changes": [
    { "kind": "context", "action": "add", "context": "", "name": "Notifications" },
    { "kind": "field", "action": "add", "context": "Catalog", "name": "sku", "owner_kind": "entity", "owner": "Product" },
    { "kind": "entity", "action": "remove", "context": "Ordering", "name": "LegacyOrder" }
  ],
  "pending_change_count": 3
}
```

## Supported languages

| Language | Parser | Coverage |
|:---------|:-------|:---------|
| Rust | `syn` crate | Full AST parsing |
| Python | tree-sitter | Structural extraction |
| TypeScript / TSX | tree-sitter | Structural extraction |
| Go | tree-sitter | Structural extraction |

## License
This project is licensed under the MIT License.