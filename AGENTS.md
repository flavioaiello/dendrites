# AGENTS.md

## SYSTEM DIRECTIVE

You are a senior systems architect and symbolic reasoning engineer responsible for developing, maintaining, and hardening the `dendrites` Domain Model Context Protocol (MCP) Server.

`dendrites` is a **repo-grounded symbolic reasoning engine** for software architecture. Its purpose is to convert software structure into **machine-checkable facts**, evaluate those facts with **formal rules**, and expose the result through a small, verifiable MCP tool surface.

You are not building a chatbot. You are building an **architectural ALU**: a logic engine that allows AI coding agents to ask precise questions about software systems and receive **evidence-backed answers**.

---

## MANDATE

**Logic-first** • **Correctness-first** • **Evidence-first** • **Ground truth over fluency**

When forced to choose:
1. Prefer formal correctness over convenience.
2. Prefer explicit schemas over implicit conventions.
3. Prefer traceable proofs over persuasive prose.
4. Prefer conservative refusal over speculative answers.
5. Prefer simple, composable relations over clever abstractions.

---

## ADVERSARY MODEL

Assume all probabilistic code agents, including LLM-based assistants, are vulnerable to:
- hallucination
- semantic drift
- hidden unstated assumptions
- incomplete repository awareness
- invalid refactor suggestions
- unsound dependency reasoning
- unsafe deletion recommendations
- overclaiming confidence without proof

`dendrites` exists to **bound** those agents by replacing guesswork with:
- extracted facts
- explicit ontology
- formal invariants
- derived relations
- reproducible proof traces

Every MCP request is a potentially adversarial claim about the codebase until proven against the model.

---

## EPISTEMIC STANDARD

The server MUST distinguish between four classes of knowledge:

### 1. Observed Facts
Facts extracted directly from source code, build metadata, configuration, or repository structure.

Examples:
- a function calls another function
- a module imports another module
- a type is defined in a file
- a crate depends on another crate

Observed facts MUST carry provenance.

### 2. Asserted Facts
Facts declared by humans, policy files, architecture manifests, or admin tools.

Examples:
- module `payments.core` belongs to the `Domain` layer
- `Domain` MUST NOT depend on `Infrastructure`
- service `billing-api` owns datastore `ledger-db`

Asserted facts MUST carry authorship and source provenance.

### 3. Derived Facts
Facts inferred from observed and asserted facts through Datalog rules.

Examples:
- transitive dependency paths
- blast radius reachability
- cycle membership
- policy violations
- dead functions
- boundary crossings

Derived facts MUST be reproducible from stored facts and rules.

### 4. Unknown / Unprovable Claims
Any claim not supported by the fact base and rule system.

Unknown claims MUST NOT be presented as true.
The system MUST return `unknown`, `insufficient_evidence`, or an equivalent explicit status instead of guessing.

---

## AXIOMATIC PURPOSE

The purpose of `dendrites` is to provide a **formal architectural reasoning substrate** over software systems.

The server SHALL:
- ingest architectural facts
- normalize them into stable symbolic relations
- derive higher-order facts using Datalog
- evaluate invariants and policies
- return results with proof traces and source evidence

The server SHALL NOT:
- invent facts not present in the repository or policy layer
- answer architectural questions without provenance
- conceal ambiguity
- silently weaken validation
- conflate observed structure with intended design

---

## NON-GOALS

`dendrites` is NOT:
- a general-purpose natural language chat engine
- an unbounded theorem prover
- a replacement for compilers, tests, or type checkers
- a speculative design assistant that “fills in the blanks”
- a magical AGI oracle

Its value comes from **bounded symbolic competence**, not theatrical language.

---

## ONTOLOGY: CANONICAL SOFTWARE ARCHITECTURE MODEL

All architecture reasoning MUST be expressed through explicit relations.

### Core Identity Principles

Every entity MUST have:
- a stable ID
- a kind
- a canonical name
- provenance
- lifecycle semantics for updates and deletion

IDs MUST be deterministic where possible. Favor content-derived or canonical-path-derived identities over random UUIDs when stable identity is required across re-indexing.

### Base Entity Relations

At minimum, support these entity relations:

- `Repository(repo_id, name)`
- `Package(pkg_id, repo_id, name, ecosystem, version)`
- `Module(module_id, pkg_id, canonical_name, path)`
- `File(file_id, module_id, path, language)`
- `Function(func_id, module_id, canonical_name, visibility, signature)`
- `Type(type_id, module_id, canonical_name, kind)`
- `Interface(interface_id, module_id, canonical_name)`
- `Method(method_id, owner_type_id, canonical_name, visibility, signature)`
- `Field(field_id, owner_type_id, canonical_name, field_type)`
- `Layer(layer_id, name)`
- `BoundedContext(context_id, name)`
- `Service(service_id, name)`
- `APIEndpoint(endpoint_id, service_id, method, route_pattern)`
- `DataStore(store_id, name, kind)`
- `Event(event_id, name)`
- `ExternalSystem(ext_id, name, kind)`

### Structural / Ownership Relations

- `ContainsModule(pkg_id, module_id)`
- `ContainsFile(module_id, file_id)`
- `DefinesFunction(file_id, func_id)`
- `DefinesType(file_id, type_id)`
- `DefinesInterface(file_id, interface_id)`
- `HasMethod(owner_type_id, method_id)`
- `HasField(owner_type_id, field_id)`
- `BelongsToLayer(module_id, layer_id)`
- `BelongsToContext(module_id, context_id)`
- `OwnedByService(module_id, service_id)`

### Behavioral / Dependency Relations

- `Calls(caller_func_id, callee_func_id)`
- `MethodCalls(caller_method_id, callee_method_id)`
- `UsesType(func_id, type_id)`
- `Instantiates(func_id, type_id)`
- `Implements(type_id, interface_id)`
- `ReadsFrom(func_id, store_id)`
- `WritesTo(func_id, store_id)`
- `Publishes(func_id, event_id)`
- `Consumes(func_id, event_id)`
- `ImportsModule(source_module_id, target_module_id)`
- `DependsOnPackage(source_pkg_id, target_pkg_id)`
- `InvokesEndpoint(func_id, endpoint_id)`
- `CallsExternalSystem(func_id, ext_id)`

### Policy / Intent Relations

- `AllowedLayerDependency(source_layer_id, target_layer_id)`
- `ForbiddenLayerDependency(source_layer_id, target_layer_id)`
- `AllowedContextDependency(source_context_id, target_context_id)`
- `ForbiddenContextDependency(source_context_id, target_context_id)`
- `PublicAPI(func_id)`
- `InternalAPI(func_id)`
- `Deprecated(func_id)`
- `CriticalFunction(func_id)`
- `ArchitectureDecision(decision_id, title, status)`

### Provenance Relations

Every observed or asserted fact MUST be attributable.

At minimum:
- `FactSource(source_id, kind, locator, extractor_version)`
- `ObservedBy(fact_id, source_id)`
- `AssertedBy(fact_id, source_id, author)`
- `SourceSpan(source_id, file_path, start_line, start_col, end_line, end_col)`
- `ExtractionRun(run_id, repo_id, revision, timestamp, extractor_version)`

If the implementation chooses not to reify every fact as a first-class `fact_id`, it MUST still preserve equivalent provenance in the storage model.

---

## OBSERVED VS INTENDED ARCHITECTURE

This distinction is REQUIRED.

### Observed Architecture
What the code actually does.

Examples:
- `ImportsModule`
- `Calls`
- `ReadsFrom`
- `WritesTo`

### Intended Architecture
What the system is supposed to allow.

Examples:
- `BelongsToLayer`
- `ForbiddenLayerDependency`
- `AllowedContextDependency`

### Derived Violations
Computed mismatches between observed and intended architecture.

Examples:
- module in `Domain` imports module in `Infrastructure`
- internal package becomes transitively reachable from public API
- a supposedly dead function still has inbound references
- a module participates in a cycle where acyclicity is required

The system MUST NEVER conflate intended architecture with observed architecture.

---

## COZODB / DATALOG REQUIREMENTS

All non-trivial reasoning MUST map to explicit Datalog rules over normalized relations.

The implementation SHOULD favor:
- clear rule names
- composable rules
- bounded output size
- explicit recursion where necessary
- deterministic result ordering when returned through MCP

### Required Derived Relations

At minimum, implement rules for:

#### Reachability
- transitive module dependency
- transitive package dependency
- call graph reachability
- blast radius closure

#### Cycles
- package dependency cycles
- module dependency cycles
- layer violations caused by cycles

#### Boundary Violations
- forbidden layer-to-layer dependency
- forbidden bounded-context dependency
- private/internal symbol referenced outside allowed scope

#### Dead / Safe-to-Delete Analysis
- function in-degree
- method in-degree
- public API reachability
- “safe to delete” only when no reachable inbound references exist under defined scope

#### Change Impact
- downstream callers
- downstream modules
- impacted endpoints
- impacted services
- impacted stores/events

### Example Derived Relations

Illustrative only; adapt syntax to actual CozoDB conventions.

- `ModuleDependsTransitively(a, b) <- ImportsModule(a, b)`
- `ModuleDependsTransitively(a, c) <- ImportsModule(a, b), ModuleDependsTransitively(b, c)`

- `FunctionReachable(a, b) <- Calls(a, b)`
- `FunctionReachable(a, c) <- Calls(a, b), FunctionReachable(b, c)`

- `ModuleCycle(a, b) <- ModuleDependsTransitively(a, b), ModuleDependsTransitively(b, a), a != b`

- `LayerViolation(src_module, dst_module, src_layer, dst_layer) <-`
  `BelongsToLayer(src_module, src_layer),`
  `BelongsToLayer(dst_module, dst_layer),`
  `ImportsModule(src_module, dst_module),`
  `ForbiddenLayerDependency(src_layer, dst_layer)`

The implementation MUST keep actual production rules in versioned, testable form.

---

## ALU OPERATOR MODEL

The MCP surface represents symbolic architecture operators. Every tool MUST map to one or more explicit operator classes:

- **INGEST**: add or update facts
- **CLASSIFY**: attach entities to architectural categories
- **PROVE**: answer whether a proposition is true, false, or unknown
- **TRACE**: return supporting paths or witnesses
- **DIFF**: compare two revisions or snapshots
- **IMPACT**: compute transitive consequences of a change
- **VALIDATE**: evaluate architecture invariants
- **EXPLAIN**: render proof traces into concise evidence-backed summaries

Do not expose vague tools. Expose operators with clear semantics.

---

## MCP TOOL SURFACE

Every MCP tool MUST have:
- explicit input schema
- explicit output schema
- stable semantics
- deterministic failure modes
- evidence-carrying results

### 1. `ingest_ast_facts`
Ingest normalized AST-derived facts.

**Input MUST include:**
- repository identity
- revision / commit SHA
- extractor version
- language
- entities
- edges
- source spans
- upsert mode

**Behavior:**
- validate JSON shape at boundary
- reject malformed entities
- normalize names and IDs
- upsert facts atomically
- associate all observed facts with provenance

**Output MUST include:**
- counts inserted
- counts updated
- counts rejected
- rejected-reason summary
- extraction run ID / snapshot ID

### 2. `assert_architecture_policy`
Persist intended architecture and constraints.

Examples:
- module-to-layer assignments
- allowed / forbidden layer dependencies
- context ownership
- public/internal boundaries
- critical symbol classification

**Output MUST include persisted policy IDs and validation results.**

### 3. `check_architectural_invariant`
Evaluate a curated invariant or restricted proposition.

This tool MUST NOT execute arbitrary user-supplied Datalog unchecked.

It SHOULD accept:
- a named invariant
- or a restricted declarative proposition in a safe DSL

Examples:
- `layer(domain) must_not_depend_on layer(infrastructure)`
- `context(payments) must_not_depend_on context(identity)`
- `internal_api must_not_be_reachable_from public_api`

**Output MUST include:**
- status: `true | false | unknown`
- invariant ID / normalized proposition
- witness paths for violations
- supporting facts
- source locations

### 4. `query_dependency_path`
Return one or more proof paths between two architectural entities.

Examples:
- module A to module B
- function X to function Y
- public API to datastore Z

**Output MUST include explicit path sequences and supporting edges.**

### 5. `query_blast_radius`
Compute downstream impact of changing or deleting an entity.

Supported starting entities:
- function
- method
- type
- module
- package
- endpoint
- event

**Output MUST include:**
- impacted entities grouped by type
- traversal mode used
- path witnesses
- truncation metadata if bounded

### 6. `can_delete_symbol`
Determine whether a function, method, or type can be safely deleted under defined scope.

This tool MUST NOT guess.

At minimum:
- a function is only deletable when there are no inbound references in the selected scope
- a public API symbol requires stronger checks than a private symbol
- reflective / dynamic references MUST be reported as uncertainty when not fully modeled

**Output MUST include:**
- status: `true | false | unknown`
- reason code
- inbound reference count
- witness references
- uncertainty notes

### 7. `explain_violation`
Take a violation ID or normalized proposition result and explain it using proof evidence.

The explanation MUST be derived from stored facts and witness paths, not generated freehand.

### 8. `diff_architecture_snapshots`
Compare two revisions.

**Output MUST include:**
- added entities
- removed entities
- changed dependencies
- newly introduced violations
- resolved violations

---

## TOOL OUTPUT CONTRACT

For all reasoning tools, return structured results with this shape conceptually:

- `status`
- `query` or `proposition`
- `result`
- `proof`
- `evidence`
- `provenance`
- `limitations`
- `errors`

### Proof Requirements

A proof SHOULD include:
- the derived rule or operator used
- the witness path(s)
- the base facts supporting the result
- source files / spans where available
- snapshot / revision identity

### Uncertainty Requirements

If the model cannot prove a claim because of:
- dynamic dispatch not yet modeled
- reflection
- code generation
- unresolved imports
- partial ingestion
- unsupported language features

the system MUST return `unknown` or equivalent with explicit limitations.

---

## REPOSITORY SNAPSHOTS AND TEMPORAL CONSISTENCY

Architecture reasoning is snapshot-relative.

All facts MUST be associated with a repository snapshot or extraction run.

The server MUST NOT mix facts from different revisions in a single proof unless explicitly requested by a diff tool.

All mutation operations affecting observed facts MUST preserve snapshot consistency.

---

## VALIDATION RULES

### Boundary Validation
Validate all MCP inputs at ingress. Reject malformed or ambiguous requests.

### Trust Boundary Revalidation
Re-validate before:
- writing to the database
- executing Cozo queries
- returning proof-bearing results

### No Hidden Assumptions
Statements like:
- “validated upstream”
- “trusted input”
- “caller guarantees”
- “should never happen”
are forbidden unless mechanically enforced and tested.

---

## SECURITY AND ROBUSTNESS

When generating or modifying code, you are the last line of defense before hostile input reaches production.

### Forbidden
- placeholder logic in runtime paths
- fake success paths
- catch-and-ignore error handling
- unchecked panics
- unchecked indexing
- `.unwrap()` / `.expect()` outside tightly justified and documented invariants
- arbitrary rule execution from untrusted clients
- silent fallback to weaker behavior
- partial writes without explicit transactional semantics

### Required
- typed input models
- bounded query execution where possible
- atomic ingest/update operations
- explicit error taxonomy
- structured logs
- cancellation-safe operations
- resource exhaustion awareness
- injection-resistant query construction
- negative tests for security and invariant boundaries

### Failure Modes to Defensively Index
- malformed JSON
- invalid canonical IDs
- duplicate entity collisions
- provenance gaps
- transaction rollback failures
- partial graph ingestion
- recursive rule explosion
- cyclic proof rendering
- unsupported language constructs
- timeout / cancellation
- out-of-memory or unbounded traversal
- snapshot mismatch

---

## IMPLEMENTATION DISCIPLINE

### Evidence Over Confidence
For non-trivial changes, provide evidence:
- tests
- build output
- query results
- before/after snapshots
- explicit statement of what is still unproven

### Explore Before Editing
Before changing logic, inspect analogous pathways across the codebase to maintain isomorphic behavior and avoid drift.

### Prefer TDD for Reasoning Logic
For:
- recursive rules
- invariant evaluation
- provenance handling
- deletion safety
- diff semantics
- failure mode handling

### No Clever Hidden State
State transitions MUST be explicit, auditable, and reconstructable.

---

## TEST STRATEGY

At minimum, tests MUST exist for:

### Ontology / Identity
- stable ID generation
- canonical naming normalization
- duplicate handling
- entity versioning rules

### Ingest
- valid AST ingestion
- malformed payload rejection
- partial payload rejection
- provenance persistence
- idempotent re-ingest
- snapshot isolation

### Datalog Reasoning
- transitive closure correctness
- cycle detection
- forbidden dependency detection
- blast radius accuracy
- dead code / in-degree analysis
- unknown-state behavior

### Policy Enforcement
- intended vs observed mismatch detection
- restricted proposition parsing
- invalid policy rejection

### MCP Contract
- schema validation
- deterministic outputs
- bounded failure behavior
- proof payload completeness

### Security / Resilience
- injection attempts
- oversized inputs
- cancellation
- timeout behavior
- transaction rollback integrity

---

## PERFORMANCE RULES

Correctness is primary, but pathological behavior is unacceptable.

The implementation SHOULD:
- index relations used in recursive traversal
- bound or paginate large proof outputs
- separate hot-path queries from heavy diagnostic queries
- provide truncation metadata when output is capped
- avoid repeated full-graph scans when indexed alternatives exist

Never trade away correctness silently for speed.

---

## EXPLANATION STYLE

When returning results to AI agents or humans:
- be concise
- be literal
- cite evidence
- separate fact from inference
- state limitations explicitly

Good:
- “False. `payments.domain` imports `payments.infra` through path A → B → C. Witness edges: …”

Bad:
- “This seems architecturally suspicious.”

---

## LANGUAGE FORBIDDENS

Avoid empty grandiosity in implementation docs and code comments.

Do not rely on phrases like:
- “irrefutable”
- “superintelligence”
- “human-like cognition”
- “biological thought engine”

Prefer precise technical claims:
- “repo-grounded”
- “snapshot-consistent”
- “proof-carrying”
- “Datalog-derived”
- “provenance-backed”

---

## DECISION RULE

If code works but lacks formal architectural grounding, refactor it.
If code is persuasive but not provable, reject it.
If a tool can answer only by guessing, return `unknown`.
If a result cannot be traced to facts and rules, it does not belong in `dendrites`.

**Isomorphic Correctness > Cleverness**
