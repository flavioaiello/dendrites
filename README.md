# Dendrites — Domain Model Context Protocol Server

A Rust-based MCP server that feeds **domain model abstractions** into GitHub Copilot, ensuring AI-generated code follows your architecture, conventions, and domain-driven design patterns.

## Why Dendrites?

### Copilot has no memory across sessions

Without Dendrites, every new chat starts from zero. Copilot re-discovers your architecture by reading files — slowly, incompletely, and inconsistently. Dendrites gives it the full domain model in **few tokens** (one tool call), which is faster and cheaper than Copilot scanning 50 files to piece it together.

### Copilot doesn't enforce architectural boundaries

Left alone, Copilot will happily create a direct import from your domain layer into infrastructure, or skip aggregate roots entirely. Dendrites's `validate_dependency` and `get_architecture_overview` act as **guardrails that Copilot checks before generating code**. This is the highest-value feature — preventing architectural drift is expensive to fix later.

### Actual vs Desired: explicit refactoring lifecycle

Dendrites maintains two models side by side:

- **Actual model** — reflects the currently implemented architecture
- **Desired model** — the target state, refined iteratively via `update_model`

The difference between actual and desired is the **pending refactoring**. Call `draft_refactoring_plan` to see the diff and get code actions. After implementing, call `accept` to promote desired → actual. Call `reset` to discard changes.

This separation means Copilot can freely evolve the desired model without side effects — acceptance is always explicit.

## How It Works

```
┌─────────────────────────────────────────────────────┐
│  GitHub Copilot (VS Code)                           │
│                                                     │
│  "Create a new billing endpoint"                    │
│       │                                             │
│       ▼                                             │
│  ┌──────────────┐    MCP stdio   ┌───────────────┐  │
│  │ Copilot Chat │◄──────────────►│ Dendrites Server  │  │
│  │ / Agent      │                │               │  │
│  └──────────────┘                │ ▪ Actual model │  │
│       │                          │ ▪ Desired model│  │
│       ▼                          │ ▪ Diff & plan  │  │
│  Code that follows YOUR          │ ▪ Accept/Reset │  │
│  architecture & conventions      └───────────────┘  │
└─────────────────────────────────────────────────────┘
```

## Quick Start

### 1. Install via Homebrew

```bash
brew tap flavioaiello/dendrites git@github.com:flavioaiello/dendrites.git
brew install dendrites
```

Or build from source:

```bash
cargo build --release
cargo install --path .
```

### 2. Import Your Domain Model (optional)

If you have an existing `dendrites.json`, import it into the local store:

```bash
dendrites import dendrites.json --workspace /path/to/your/project
```

The model is stored in `~/.dendrites/dendrites.db` (CozoDB), keyed by workspace path.
If you skip this step, Dendrites starts with an empty model that Copilot can populate via `update_model`.
Imported models are set as both actual and desired — a clean starting point.

### 3. Integrate with VS Code / GitHub Copilot

Add to your project's `.vscode/mcp.json`:

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

After installing, **restart VS Code** or run `> MCP: List Servers` from the command palette to see the Dendrites server listed and active.

### CLI Commands

```bash
# Start MCP server (used by VS Code, not called manually)
dendrites serve --workspace /path/to/project

# Import a dendrites.json file into the local store
dendrites import dendrites.json --workspace /path/to/project

# Export a project's model back to JSON
dendrites export model.json --workspace /path/to/project

# List all stored projects
dendrites list
```

## How It Works with Copilot

Once connected, Copilot gains access to **6 tools** (4 read, 2 write), **1 prompt**, and **dynamic resources**:

### Read Tools (query the domain model)

| Tool | What it does |
|------|-------------|
| `get_architecture_overview` | Both actual and desired models with pending changes status |
| `validate_dependency` | Checks if a cross-context dependency is allowed |
| `suggest_file_path` | Where a new file should be placed per conventions |
| `query_model` | Datalog-based analysis: transitive dependencies, circular dependency detection, layer violations, impact analysis, aggregate quality checks, dependency graphs, and custom Datalog queries |

### Write Tools (update the desired model)

All mutations to the desired model are **auto-saved** to the local store.

| Tool | What it does |
|------|-------------|
| `update_model` | Create, update, or remove any element in the **desired** model (bounded context, entity, service, event) |
| `draft_refactoring_plan` | `plan` (default): diff actual vs desired → code actions. `accept`: promote desired → actual. `reset`: discard desired changes. |

### Resources (Copilot can attach these as context)

| URI | Content |
|-----|---------|
| `dendrites://architecture/overview` | Architecture overview (JSON) |
| `dendrites://architecture/rules` | Architectural rules (JSON) |
| `dendrites://architecture/conventions` | Conventions (JSON) |
| `dendrites://context/{name}` | Per bounded-context detail (JSON) |

### Prompt

| Name | Description |
|------|-------------|
| `dendrites_guidelines` | Architecture guidelines with actual/desired workflow, mandatory tool usage, and project-specific content. Eliminates the need for a per-project `copilot-instructions.md`. |

### Example Copilot Interactions

**You ask:** *"Create a new endpoint to cancel a subscription"*

Copilot will:
1. Call `get_architecture_overview` → sees actual + desired models, status "in_sync"
2. Call `suggest_file_path("Billing", "service", "CancelSubscription")` → `src/billing/application/cancel_subscription.rs`
3. Call `validate_dependency("Billing", "Identity")` → allowed
4. Generate code that:
   - Places the handler in `src/billing/api/`
   - Uses the `Subscription` aggregate's `cancel()` method
   - Emits a domain event
   - Follows error handling conventions (`thiserror`)
   - Respects the repository pattern

**You ask:** *"Add a field to User"*

Copilot will:
1. Call `get_architecture_overview` → sees existing entities, rules, conventions
2. Call `update_model` to add the field to the desired model
3. Call `draft_refactoring_plan` → gets code diff and migration notes
4. Generate code, then call `draft_refactoring_plan` with `action: "accept"`

### Bidirectional: Codebase → Model → Refactoring

**You ask:** *"Analyze this codebase and build a domain model from it"*

Copilot will:
1. Scan the module structure → call `update_model` for each discovered bounded context
2. Read entity files → call `update_model` with fields, methods, invariants
3. Read service files → call `update_model` with dependencies and layer
4. Call `draft_refactoring_plan` with `action: "accept"` to set this as the actual model

**You then ask:** *"Rename the Identity context to Auth and add a `last_login` field to User"*

Copilot will:
1. Call `update_model` to update the desired model (auto-saved)
2. Call `draft_refactoring_plan` → gets a prioritized list of code changes:
   - `modify_file: src/identity/domain/user.rs` (high)
   - `move_file: src/identity → src/auth` (critical)
   - Migration note: *"New field 'last_login' on 'User' — needs ALTER TABLE migration"*
3. Execute code actions in priority order
4. Call `draft_refactoring_plan` with `action: "accept"` → actual = desired

## Domain Model Schema

The `dendrites.json` file describes your entire system architecture:

```
DomainModel
├── name, description
├── tech_stack (language, framework, database, ...)
├── bounded_contexts[]
│   ├── name, module_path
│   ├── entities[] (fields, methods, invariants, aggregate_root)
│   ├── value_objects[] (fields, validation_rules)
│   ├── services[] (kind: domain|application|infrastructure, methods, dependencies)
│   ├── repositories[] (aggregate, methods)
│   ├── events[] (fields, source entity)
│   └── dependencies[] (allowed cross-context deps)
├── rules[] (id, description, severity, scope)
└── conventions
    ├── naming (entities, services, events, ...)
    ├── file_structure (pattern, layers)
    ├── error_handling
    └── testing
```

## Storage & Inference

Dendrites stores domain models in a local CozoDB database at `~/.dendrites/dendrites.db`, keyed by workspace path. CozoDB is a Datalog-based relational database that enables **logical inference** over the domain model.

Each workspace has two models:

- **Desired** (`model_json`) — the target architecture being refined
- **Actual** (`baseline_json`) — the implemented architecture, updated via explicit `accept`

### Relational Decomposition

When a model is saved, Dendrites decomposes it into 16 CozoDB relations (context, entity, entity_field, entity_method, service, service_dep, event, invariant, etc.) that enable Datalog queries.

### Built-in Analyses (via `query_model` tool)

| Analysis | What it finds |
|----------|--------------|
| `transitive_deps` | All transitive dependencies from a bounded context using recursive Datalog |
| `circular_deps` | Circular dependency cycles in context dependency graph |
| `layer_violations` | Domain services depending on infrastructure — DDD layer violations |
| `impact_analysis` | Affected events, services, and dependent contexts when changing an entity |
| `aggregate_quality` | Aggregate roots without invariants (quality gap) |
| `dependency_graph` | Full graph JSON with nodes, edges, and cycles |
| `datalog` | Arbitrary Datalog queries against the decomposed model |

### Custom Datalog Queries

Run arbitrary queries against the knowledge graph. Available relations:
`context`, `context_dep`, `entity`, `entity_field`, `entity_method`, `method_param`, `invariant`, `service`, `service_dep`, `service_method`, `event`, `event_field`, `value_object`, `repository`, `arch_rule`

Example: find all aggregate root entities:
```
?[context, name] := *entity{workspace: $ws, context, name, aggregate_root: true}
```

This means:

- **Multi-project support**: Each workspace gets its own isolated model pair
- **Explicit acceptance**: The actual model only changes when you say so
- **No per-project config files needed**: The model lives centrally on the dev machine
- **Portable import/export**: Use `dendrites import` / `export` to share models via `dendrites.json` files
- **Version control friendly**: Export to `dendrites.json` when you want to commit the model to git

## Architectural Enforcement

Dendrites doesn't just inform — it **constrains**. The `validate_dependency` tool lets Copilot check whether cross-context imports are allowed before generating them. The architectural rules describe invariants that Copilot will respect.

Example rules from the included config:
- **LAYER-001**: Domain layer must not depend on infrastructure
- **DDD-001**: State mutations must go through aggregate root methods
- **DDD-002**: Cross-aggregate communication via domain events only
- **ERR-001**: Use typed domain errors, never panic

## Advanced: Custom `instructions.md`

Dendrites ships a built-in `dendrites_guidelines` prompt that serves architecture instructions automatically. For additional project-specific instructions, create `.github/copilot-instructions.md`:

```markdown
## Architecture

This project uses Domain-Driven Design with a hexagonal architecture.
Before writing any code, ALWAYS call `get_architecture_overview` from the Dendrites
server to understand actual and desired model state.

When creating new files, call `suggest_file_path` to determine the correct location.
When adding cross-context dependencies, call `validate_dependency` to verify it's allowed.
After implementing refactorings, call `draft_refactoring_plan` with `action: "accept"`.
```

This ensures Copilot **proactively** queries the domain model rather than waiting for tool hints.

## Installation

### Homebrew (recommended)

```bash
brew tap flavioaiello/dendrites git@github.com:flavioaiello/dendrites.git
brew install dendrites
```

### From source

```bash
cargo install --path .
```

## Development

```bash
# Build debug
cargo build

# Run tests
cargo test

# Run with debug logging
RUST_LOG=debug cargo run -- serve --workspace .

# Import the example model
cargo run -- import dendrites.json --workspace /path/to/project

# List stored projects
cargo run -- list
```

## License

MIT
