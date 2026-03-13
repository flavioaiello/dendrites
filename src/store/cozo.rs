use anyhow::{Context, Result};
use cozo::{DbInstance, ScriptMutability};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;

use crate::domain::model::*;

/// CozoDB-backed cerebral store for domain models.
///
/// Architecture:
/// - Every domain element is stored as a **first-class relational tuple**.
/// - Sub-structures (fields, methods, parameters, invariants, validation rules)
///   are their own relations — not JSON blobs. Datalog can reason about them directly.
/// - All domain relations carry `state: 'desired' | 'actual'` for set-theoretic diffing.
/// - Diff, accept, and reset are **pure Datalog set operations**.
/// - `DomainModel` structs are reconstructed on-demand from relations.
pub struct Store {
    db: DbInstance,
}

impl Store {
    /// Open (or create) the store at a specific path.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }

        let db = DbInstance::new("sqlite", path.to_str().unwrap_or(""), Default::default())
            .map_err(|e| anyhow::anyhow!("Failed to open CozoDB: {:?}", e))?;

        Self::init_schema(&db)?;
        Ok(Self { db })
    }

    // ── Schema ─────────────────────────────────────────────────────────────

    fn init_schema(db: &DbInstance) -> Result<()> {
        // Migration v0: old schema used 'workspace_path' key on project
        let has_v0 = db
            .run_script(
                "?[x] := *project{workspace_path: x}",
                Default::default(),
                ScriptMutability::Immutable,
            )
            .is_ok();

        if has_v0 {
            let old_tables = [
                "project", "context", "context_dep", "entity", "entity_field",
                "entity_method", "method_param", "invariant", "service", "service_dep",
                "service_method", "event", "event_field", "value_object", "repository",
                "arch_rule", "live_import",
            ];
            for t in old_tables {
                let _ = db.run_script(
                    &format!("::remove {t}"),
                    Default::default(),
                    ScriptMutability::Mutable,
                );
            }
        }

        // Migration v1: schema had *_json blob columns on entity/service/event/etc.
        let has_v1 = db
            .run_script(
                "?[x] := *entity{fields_json: x}",
                Default::default(),
                ScriptMutability::Immutable,
            )
            .is_ok();

        if has_v1 {
            for t in [
                "entity", "service", "event", "value_object", "repository",
            ] {
                let _ = db.run_script(
                    &format!("::remove {t}"),
                    Default::default(),
                    ScriptMutability::Mutable,
                );
            }
        }

        // Migration v2: tables lacked file_path/start_line/end_line columns
        let needs_v2 = db
            .run_script(
                "?[x] := *service{file_path: x}",
                Default::default(),
                ScriptMutability::Immutable,
            )
            .is_err()
            && db
                .run_script(
                    "?[x] := *service{name: x}",
                    Default::default(),
                    ScriptMutability::Immutable,
                )
                .is_ok();

        if needs_v2 {
            for t in [
                "entity", "service", "event", "value_object", "repository", "module",
            ] {
                let _ = db.run_script(
                    &format!("::remove {t}"),
                    Default::default(),
                    ScriptMutability::Mutable,
                );
            }
        }

        let schemas = vec![
            // Project metadata (rules/tech/conventions as JSON — config, not domain topology)
            ":create project { workspace: String => name: String, description: String default '', updated_at: String, rules_json: String default '[]', tech_stack_json: String default '{}', conventions_json: String default '{}' }",
            // ── Domain element headers ──
            ":create context { workspace: String, name: String, state: String => description: String default '', module_path: String default '' }",
            ":create context_dep { workspace: String, from_ctx: String, to_ctx: String, state: String }",
            ":create owner_meta { workspace: String, context: String, owner_kind: String, owner: String, state: String => team: String default '', owners_json: String default '[]', rationale: String default '' }",
            ":create aggregate { workspace: String, context: String, name: String, state: String => description: String default '', root_entity: String default '' }",
            ":create aggregate_member { workspace: String, context: String, aggregate: String, member_kind: String, member: String, state: String }",
            ":create entity { workspace: String, context: String, name: String, state: String => description: String default '', aggregate_root: Bool default false, file_path: String default '', start_line: Int default 0, end_line: Int default 0 }",
            ":create policy { workspace: String, context: String, name: String, state: String => description: String default '', kind: String default 'domain' }",
            ":create policy_link { workspace: String, context: String, policy: String, link_kind: String, link: String, idx: Int, state: String }",
            ":create read_model { workspace: String, context: String, name: String, state: String => description: String default '', source: String default '' }",
            ":create service { workspace: String, context: String, name: String, state: String => description: String default '', kind: String default 'domain', file_path: String default '', start_line: Int default 0, end_line: Int default 0 }",
            ":create service_dep { workspace: String, context: String, service: String, dep: String, state: String }",
            ":create event { workspace: String, context: String, name: String, state: String => description: String default '', source: String default '', file_path: String default '', start_line: Int default 0, end_line: Int default 0 }",
            ":create value_object { workspace: String, context: String, name: String, state: String => description: String default '', file_path: String default '', start_line: Int default 0, end_line: Int default 0 }",
            ":create repository { workspace: String, context: String, name: String, state: String => aggregate: String default '', file_path: String default '', start_line: Int default 0, end_line: Int default 0 }",
            ":create module { workspace: String, context: String, name: String, state: String => path: String default '', public: Bool default false, file_path: String default '', description: String default '' }",
            ":create external_system { workspace: String, name: String, state: String => description: String default '', kind: String default '', rationale: String default '' }",
            ":create external_system_context { workspace: String, system: String, context: String, idx: Int, state: String }",
            ":create api_endpoint { workspace: String, context: String, id: String, state: String => service_id: String default '', method: String default '', route_pattern: String default '', description: String default '' }",
            ":create invokes_endpoint { workspace: String, caller_context: String, caller_method: String, endpoint_id: String, state: String }",
            ":create calls_external_system { workspace: String, caller_context: String, caller_method: String, ext_id: String, state: String }",
            ":create architectural_decision { workspace: String, id: String, state: String => title: String default '', status: String default 'proposed', scope: String default '', date: String default '', rationale: String default '' }",
            ":create decision_context { workspace: String, decision_id: String, context: String, idx: Int, state: String }",
            ":create decision_consequence { workspace: String, decision_id: String, idx: Int, state: String => text: String default '' }",
            // ── First-class sub-structures ──
            ":create invariant { workspace: String, context: String, entity: String, idx: Int, state: String => text: String }",
            ":create field { workspace: String, context: String, owner_kind: String, owner: String, name: String, state: String => field_type: String default '', required: Bool default false, description: String default '', idx: Int default 0 }",
            ":create method { workspace: String, context: String, owner_kind: String, owner: String, name: String, state: String => description: String default '', return_type: String default '', idx: Int default 0 }",
            ":create method_param { workspace: String, context: String, owner_kind: String, owner: String, method: String, name: String, state: String => param_type: String default '', required: Bool default false, description: String default '', idx: Int default 0 }",
            ":create vo_rule { workspace: String, context: String, value_object: String, idx: Int, state: String => text: String }",
            // ── Architecture policy relations ──
            ":create layer_assignment { workspace: String, context: String => layer: String }",
            ":create dependency_constraint { workspace: String, constraint_kind: String, source: String, target: String => rule: String default 'forbidden' }",
            // Ephemeral — no state column
            ":create live_import { workspace: String, from_file: String, to_module: String }",
            // AST structural edges (extends, implements, decorators)
            ":create ast_edge { workspace: String, state: String, from_node: String, to_node: String, edge_type: String }",
        ];

        for schema in schemas {
            let _ = db.run_script(schema, Default::default(), ScriptMutability::Mutable);
        }
        Ok(())
    }

    // ── Core State Operations ──────────────────────────────────────────────

    /// Save the desired domain model: decomposes into relational rows with `state='desired'`.
    pub fn save_desired(&self, workspace_path: &str, model: &DomainModel) -> Result<()> {
        let ws = canonicalize_path(workspace_path);
        let now = chrono_now();

        // Upsert project metadata
        let rules_json = serde_json::to_string(&model.rules).unwrap_or_else(|_| "[]".into());
        let tech_json =
            serde_json::to_string(&model.tech_stack).unwrap_or_else(|_| "{}".into());
        let conv_json =
            serde_json::to_string(&model.conventions).unwrap_or_else(|_| "{}".into());
        let params = params_map(&[
            ("ws", &ws),
            ("name", &model.name),
            ("desc", &model.description),
            ("now", &now),
            ("rules", &rules_json),
            ("tech", &tech_json),
            ("conv", &conv_json),
        ]);
        self.db
            .run_script(
                "?[workspace, name, description, updated_at, rules_json, tech_stack_json, conventions_json] <- \
                    [[$ws, $name, $desc, $now, $rules, $tech, $conv]] \
                 :put project { workspace => name, description, updated_at, rules_json, tech_stack_json, conventions_json }",
                params,
                ScriptMutability::Mutable,
            )
            .map_err(|e| anyhow::anyhow!("Failed to save project metadata: {:?}", e))?;

        self.save_state(&ws, model, "desired")
    }

    /// Load the desired domain model (reconstructed from relations).
    pub fn load_desired(&self, workspace_path: &str) -> Result<Option<DomainModel>> {
        self.reconstruct_model(workspace_path, "desired")
    }

    /// Load the actual domain model (reconstructed from relations).
    pub fn load_actual(&self, workspace_path: &str) -> Result<Option<DomainModel>> {
        self.reconstruct_model(workspace_path, "actual")
    }

    /// Save a scanned model as the actual state (from AST extraction).
    pub fn save_actual(&self, workspace_path: &str, model: &DomainModel) -> Result<()> {
        let ws = canonicalize_path(workspace_path);
        self.save_state(&ws, model, "actual")
    }

    /// Accept: promote desired → actual via Datalog state copy.
    pub fn accept(&self, workspace_path: &str) -> Result<()> {
        let ws = canonicalize_path(workspace_path);
        self.copy_state(&ws, "desired", "actual")
    }

    /// Reset: revert desired → actual via Datalog state copy, return the restored model.
    pub fn reset(&self, workspace_path: &str) -> Result<Option<DomainModel>> {
        let ws = canonicalize_path(workspace_path);
        let has_actual = self.load_actual(workspace_path)?.is_some();
        if !has_actual {
            return Ok(None);
        }
        self.copy_state(&ws, "actual", "desired")?;
        self.load_desired(workspace_path)
    }

    // ── Private: Sub-structure Helpers ──────────────────────────────────────

    /// Save a slice of fields into the `field` relation.
    fn save_fields(
        &self,
        ws: &str,
        ctx: &str,
        owner_kind: &str,
        owner: &str,
        fields: &[Field],
        state: &str,
    ) -> Result<()> {
        for (i, f) in fields.iter().enumerate() {
            let mut params = params_map(&[
                ("ws", ws),
                ("ctx", ctx),
                ("ok", owner_kind),
                ("ow", owner),
                ("name", &f.name),
                ("st", state),
                ("ft", &f.field_type),
                ("desc", &f.description),
            ]);
            params.insert("req".into(), cozo::DataValue::Bool(f.required));
            params.insert("idx".into(), int_dv(i as i64));
            self.db
                .run_script(
                    "?[workspace, context, owner_kind, owner, name, state, field_type, required, description, idx] <- \
                        [[$ws, $ctx, $ok, $ow, $name, $st, $ft, $req, $desc, $idx]] \
                     :put field { workspace, context, owner_kind, owner, name, state => field_type, required, description, idx }",
                    params,
                    ScriptMutability::Mutable,
                )
                .map_err(|e| anyhow::anyhow!("save field '{}'.{}: {:?}", owner, f.name, e))?;
        }
        Ok(())
    }

    /// Save a slice of methods (+ their params) into the `method` and `method_param` relations.
    fn save_methods(
        &self,
        ws: &str,
        ctx: &str,
        owner_kind: &str,
        owner: &str,
        methods: &[Method],
        state: &str,
    ) -> Result<()> {
        for (i, m) in methods.iter().enumerate() {
            let mut params = params_map(&[
                ("ws", ws),
                ("ctx", ctx),
                ("ok", owner_kind),
                ("ow", owner),
                ("name", &m.name),
                ("st", state),
                ("desc", &m.description),
                ("rt", &m.return_type),
            ]);
            params.insert("idx".into(), int_dv(i as i64));
            self.db
                .run_script(
                    "?[workspace, context, owner_kind, owner, name, state, description, return_type, idx] <- \
                        [[$ws, $ctx, $ok, $ow, $name, $st, $desc, $rt, $idx]] \
                     :put method { workspace, context, owner_kind, owner, name, state => description, return_type, idx }",
                    params,
                    ScriptMutability::Mutable,
                )
                .map_err(|e| anyhow::anyhow!("save method '{}'.{}: {:?}", owner, m.name, e))?;

            // Method parameters
            for (j, p) in m.parameters.iter().enumerate() {
                let mut pp = params_map(&[
                    ("ws", ws),
                    ("ctx", ctx),
                    ("ok", owner_kind),
                    ("ow", owner),
                    ("method", &m.name),
                    ("name", &p.name),
                    ("st", state),
                    ("pt", &p.field_type),
                    ("desc", &p.description),
                ]);
                pp.insert("req".into(), cozo::DataValue::Bool(p.required));
                pp.insert("idx".into(), int_dv(j as i64));
                self.db
                    .run_script(
                        "?[workspace, context, owner_kind, owner, method, name, state, param_type, required, description, idx] <- \
                            [[$ws, $ctx, $ok, $ow, $method, $name, $st, $pt, $req, $desc, $idx]] \
                         :put method_param { workspace, context, owner_kind, owner, method, name, state => param_type, required, description, idx }",
                        pp,
                        ScriptMutability::Mutable,
                    )
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "save method_param '{}'.{}.{}: {:?}",
                            owner,
                            m.name,
                            p.name,
                            e
                        )
                    })?;
            }
        }
        Ok(())
    }

    fn save_owner_meta(
        &self,
        ws: &str,
        ctx: &str,
        owner_kind: &str,
        owner: &str,
        ownership: &Ownership,
        state: &str,
    ) -> Result<()> {
        let owners_json = serde_json::to_string(&ownership.owners).unwrap_or_else(|_| "[]".into());
        self.db
            .run_script(
                "?[workspace, context, owner_kind, owner, state, team, owners_json, rationale] <- [[$ws, $ctx, $ok, $owner, $st, $team, $owners, $rationale]] :put owner_meta { workspace, context, owner_kind, owner, state => team, owners_json, rationale }",
                params_map(&[
                    ("ws", ws),
                    ("ctx", ctx),
                    ("ok", owner_kind),
                    ("owner", owner),
                    ("st", state),
                    ("team", &ownership.team),
                    ("owners", &owners_json),
                    ("rationale", &ownership.rationale),
                ]),
                ScriptMutability::Mutable,
            )
            .map_err(|e| anyhow::anyhow!("save owner_meta '{}':'{}': {:?}", owner_kind, owner, e))?;
        Ok(())
    }

    fn remove_owner_meta(&self, ws: &str, ctx: &str, owner_kind: &str, owner: &str) {
        let _ = self.db.run_script(
            "?[workspace, context, owner_kind, owner, state] := *owner_meta{workspace, context, owner_kind, owner, state}, workspace = $ws, context = $ctx, owner_kind = $ok, owner = $owner :rm owner_meta { workspace, context, owner_kind, owner, state }",
            params_map(&[("ws", ws), ("ctx", ctx), ("ok", owner_kind), ("owner", owner)]),
            ScriptMutability::Mutable,
        );
    }

    fn replace_owner_fields(
        &self,
        ws: &str,
        ctx: &str,
        owner_kind: &str,
        owner: &str,
        fields: &[Field],
    ) -> Result<()> {
        let _ = self.db.run_script(
            "?[workspace, context, owner_kind, owner, name, state] := *field{workspace, context, owner_kind, owner, name, state}, workspace = $ws, context = $ctx, owner_kind = $ok, owner = $owner, state = 'desired' :rm field { workspace, context, owner_kind, owner, name, state }",
            params_map(&[("ws", ws), ("ctx", ctx), ("ok", owner_kind), ("owner", owner)]),
            ScriptMutability::Mutable,
        );
        self.save_fields(ws, ctx, owner_kind, owner, fields, "desired")
    }

    fn replace_owner_methods(
        &self,
        ws: &str,
        ctx: &str,
        owner_kind: &str,
        owner: &str,
        methods: &[Method],
    ) -> Result<()> {
        let _ = self.db.run_script(
            "?[workspace, context, owner_kind, owner, name, state] := *method{workspace, context, owner_kind, owner, name, state}, workspace = $ws, context = $ctx, owner_kind = $ok, owner = $owner, state = 'desired' :rm method { workspace, context, owner_kind, owner, name, state }",
            params_map(&[("ws", ws), ("ctx", ctx), ("ok", owner_kind), ("owner", owner)]),
            ScriptMutability::Mutable,
        );
        let _ = self.db.run_script(
            "?[workspace, context, owner_kind, owner, method, name, state] := *method_param{workspace, context, owner_kind, owner, method, name, state}, workspace = $ws, context = $ctx, owner_kind = $ok, owner = $owner, state = 'desired' :rm method_param { workspace, context, owner_kind, owner, method, name, state }",
            params_map(&[("ws", ws), ("ctx", ctx), ("ok", owner_kind), ("owner", owner)]),
            ScriptMutability::Mutable,
        );
        self.save_methods(ws, ctx, owner_kind, owner, methods, "desired")
    }

    fn replace_invariants(&self, ws: &str, ctx: &str, entity: &str, invariants: &[String]) -> Result<()> {
        let _ = self.db.run_script(
            "?[workspace, context, entity, idx, state] := *invariant{workspace, context, entity, idx, state}, workspace = $ws, context = $ctx, entity = $entity, state = 'desired' :rm invariant { workspace, context, entity, idx, state }",
            params_map(&[("ws", ws), ("ctx", ctx), ("entity", entity)]),
            ScriptMutability::Mutable,
        );
        for (idx, invariant) in invariants.iter().enumerate() {
            let mut params = params_map(&[("ws", ws), ("ctx", ctx), ("entity", entity), ("text", invariant)]);
            params.insert("idx".into(), int_dv(idx as i64));
            self.db.run_script(
                "?[workspace, context, entity, idx, state, text] <- [[$ws, $ctx, $entity, $idx, 'desired', $text]] :put invariant { workspace, context, entity, idx, state => text }",
                params,
                ScriptMutability::Mutable,
            ).map_err(|e| anyhow::anyhow!("replace_invariants '{}': {:?}", entity, e))?;
        }
        Ok(())
    }

    fn replace_vo_rules(&self, ws: &str, ctx: &str, value_object: &str, rules: &[String]) -> Result<()> {
        let _ = self.db.run_script(
            "?[workspace, context, value_object, idx, state] := *vo_rule{workspace, context, value_object, idx, state}, workspace = $ws, context = $ctx, value_object = $vo, state = 'desired' :rm vo_rule { workspace, context, value_object, idx, state }",
            params_map(&[("ws", ws), ("ctx", ctx), ("vo", value_object)]),
            ScriptMutability::Mutable,
        );
        for (idx, rule) in rules.iter().enumerate() {
            let mut params = params_map(&[("ws", ws), ("ctx", ctx), ("vo", value_object), ("text", rule)]);
            params.insert("idx".into(), int_dv(idx as i64));
            self.db.run_script(
                "?[workspace, context, value_object, idx, state, text] <- [[$ws, $ctx, $vo, $idx, 'desired', $text]] :put vo_rule { workspace, context, value_object, idx, state => text }",
                params,
                ScriptMutability::Mutable,
            ).map_err(|e| anyhow::anyhow!("replace_vo_rules '{}': {:?}", value_object, e))?;
        }
        Ok(())
    }

    fn replace_service_deps(&self, ws: &str, ctx: &str, service: &str, dependencies: &[String]) -> Result<()> {
        let _ = self.db.run_script(
            "?[workspace, context, service, dep, state] := *service_dep{workspace, context, service, dep, state}, workspace = $ws, context = $ctx, service = $service, state = 'desired' :rm service_dep { workspace, context, service, dep, state }",
            params_map(&[("ws", ws), ("ctx", ctx), ("service", service)]),
            ScriptMutability::Mutable,
        );
        for dep in dependencies {
            self.db.run_script(
                "?[workspace, context, service, dep, state] <- [[$ws, $ctx, $service, $dep, 'desired']] :put service_dep { workspace, context, service, dep, state }",
                params_map(&[("ws", ws), ("ctx", ctx), ("service", service), ("dep", dep)]),
                ScriptMutability::Mutable,
            ).map_err(|e| anyhow::anyhow!("replace_service_deps '{}': {:?}", service, e))?;
        }
        Ok(())
    }

    fn ensure_project(&self, workspace_path: &str) -> Result<()> {
        let ws = canonicalize_path(workspace_path);
        let has_project = self.db.run_script(
            "?[name] := *project{workspace: $ws, name}",
            params_map(&[("ws", &ws)]),
            ScriptMutability::Immutable,
        ).map(|r| !r.rows.is_empty()).unwrap_or(false);
        if has_project {
            return Ok(());
        }

        let empty = DomainModel::empty(workspace_path);
        let now = chrono_now();
        let rules_json = serde_json::to_string(&empty.rules).unwrap_or_else(|_| "[]".into());
        let tech_json = serde_json::to_string(&empty.tech_stack).unwrap_or_else(|_| "{}".into());
        let conv_json = serde_json::to_string(&empty.conventions).unwrap_or_else(|_| "{}".into());
        self.db.run_script(
            "?[workspace, name, description, updated_at, rules_json, tech_stack_json, conventions_json] <- [[$ws, $name, $desc, $now, $rules, $tech, $conv]] :put project { workspace => name, description, updated_at, rules_json, tech_stack_json, conventions_json }",
            params_map(&[("ws", &ws), ("name", &empty.name), ("desc", &empty.description), ("now", &now), ("rules", &rules_json), ("tech", &tech_json), ("conv", &conv_json)]),
            ScriptMutability::Mutable,
        ).map_err(|e| anyhow::anyhow!("ensure_project: {:?}", e))?;
        self.save_owner_meta(&ws, "", "project", &empty.name, &empty.ownership, "desired")?;
        Ok(())
    }

    /// Query fields for a specific owner from the `field` relation, ordered by idx.
    fn query_fields(
        &self,
        ws: &str,
        ctx: &str,
        owner_kind: &str,
        owner: &str,
        state: &str,
    ) -> Vec<Field> {
        let params = params_map(&[
            ("ws", ws),
            ("ctx", ctx),
            ("ok", owner_kind),
            ("ow", owner),
            ("st", state),
        ]);
        let rows = self
            .db
            .run_script(
                "?[name, field_type, required, description, idx] := \
                    *field{workspace: $ws, context: $ctx, owner_kind: $ok, owner: $ow, \
                           name, state: $st, field_type, required, description, idx}",
                params,
                ScriptMutability::Immutable,
            )
            .map(|r| r.rows)
            .unwrap_or_default();

        let mut indexed: Vec<(i64, Field)> = rows
            .iter()
            .map(|r| {
                (
                    dv_i64(&r[4]),
                    Field {
                        name: dv_str(&r[0]),
                        field_type: dv_str(&r[1]),
                        required: matches!(&r[2], cozo::DataValue::Bool(true)),
                        description: dv_str(&r[3]),
                    },
                )
            })
            .collect();
        indexed.sort_by_key(|(i, _)| *i);
        indexed.into_iter().map(|(_, f)| f).collect()
    }

    /// Query methods (+ their params) for a specific owner, ordered by idx.
    fn query_methods(
        &self,
        ws: &str,
        ctx: &str,
        owner_kind: &str,
        owner: &str,
        state: &str,
    ) -> Vec<Method> {
        let params = params_map(&[
            ("ws", ws),
            ("ctx", ctx),
            ("ok", owner_kind),
            ("ow", owner),
            ("st", state),
        ]);
        let rows = self
            .db
            .run_script(
                "?[name, description, return_type, idx] := \
                    *method{workspace: $ws, context: $ctx, owner_kind: $ok, owner: $ow, \
                            name, state: $st, description, return_type, idx}",
                params,
                ScriptMutability::Immutable,
            )
            .map(|r| r.rows)
            .unwrap_or_default();

        let mut indexed: Vec<(i64, Method)> = rows
            .iter()
            .map(|r| {
                let mname = dv_str(&r[0]);
                let mp = params_map(&[
                    ("ws", ws),
                    ("ctx", ctx),
                    ("ok", owner_kind),
                    ("ow", owner),
                    ("method", &mname),
                    ("st", state),
                ]);
                let param_rows = self
                    .db
                    .run_script(
                        "?[name, param_type, required, description, idx] := \
                            *method_param{workspace: $ws, context: $ctx, owner_kind: $ok, \
                                          owner: $ow, method: $method, name, state: $st, \
                                          param_type, required, description, idx}",
                        mp,
                        ScriptMutability::Immutable,
                    )
                    .map(|r| r.rows)
                    .unwrap_or_default();

                let mut parms: Vec<(i64, Field)> = param_rows
                    .iter()
                    .map(|p| {
                        (
                            dv_i64(&p[4]),
                            Field {
                                name: dv_str(&p[0]),
                                field_type: dv_str(&p[1]),
                                required: matches!(&p[2], cozo::DataValue::Bool(true)),
                                description: dv_str(&p[3]),
                            },
                        )
                    })
                    .collect();
                parms.sort_by_key(|(i, _)| *i);

                (
                    dv_i64(&r[3]),
                    Method {
                        name: mname,
                        description: dv_str(&r[1]),
                        parameters: parms.into_iter().map(|(_, p)| p).collect(),
                        return_type: dv_str(&r[2]),
                        file_path: None,
                        start_line: None,
                        end_line: None,
                    },
                )
            })
            .collect();
        indexed.sort_by_key(|(i, _)| *i);
        indexed.into_iter().map(|(_, m)| m).collect()
    }

    fn query_ownership(
        &self,
        ws: &str,
        ctx: &str,
        owner_kind: &str,
        owner: &str,
        state: &str,
    ) -> Ownership {
        let rows = self
            .db
            .run_script(
                "?[team, owners_json, rationale] := *owner_meta{workspace: $ws, context: $ctx, owner_kind: $ok, owner: $owner, state: $st, team, owners_json, rationale}",
                params_map(&[("ws", ws), ("ctx", ctx), ("ok", owner_kind), ("owner", owner), ("st", state)]),
                ScriptMutability::Immutable,
            )
            .map(|r| r.rows)
            .unwrap_or_default();

        if let Some(row) = rows.first() {
            let owners = serde_json::from_str::<Vec<String>>(&dv_str(&row[1])).unwrap_or_default();
            Ownership {
                team: dv_str(&row[0]),
                owners,
                rationale: dv_str(&row[2]),
            }
        } else {
            Ownership::default()
        }
    }

    fn query_indexed_strings(&self, query: &str, params: BTreeMap<String, cozo::DataValue>) -> Vec<String> {
        let rows = self
            .db
            .run_script(query, params, ScriptMutability::Immutable)
            .map(|r| r.rows)
            .unwrap_or_default();

        let mut indexed: Vec<(i64, String)> = rows
            .iter()
            .map(|row| (dv_i64(&row[0]), dv_str(&row[1])))
            .collect();
        indexed.sort_by_key(|(idx, _)| *idx);
        indexed.into_iter().map(|(_, value)| value).collect()
    }

    fn policy_kind_key(kind: &PolicyKind) -> &'static str {
        match kind {
            PolicyKind::Domain => "domain",
            PolicyKind::ProcessManager => "process_manager",
            PolicyKind::Integration => "integration",
        }
    }

    /// Query invariants for an entity, ordered by idx.
    fn query_invariants(
        &self,
        ws: &str,
        ctx: &str,
        entity: &str,
        state: &str,
    ) -> Vec<String> {
        let params = params_map(&[
            ("ws", ws),
            ("ctx", ctx),
            ("ent", entity),
            ("st", state),
        ]);
        let rows = self
            .db
            .run_script(
                "?[idx, text] := \
                    *invariant{workspace: $ws, context: $ctx, entity: $ent, \
                               idx, state: $st, text}",
                params,
                ScriptMutability::Immutable,
            )
            .map(|r| r.rows)
            .unwrap_or_default();

        let mut indexed: Vec<(i64, String)> =
            rows.iter().map(|r| (dv_i64(&r[0]), dv_str(&r[1]))).collect();
        indexed.sort_by_key(|(i, _)| *i);
        indexed.into_iter().map(|(_, t)| t).collect()
    }

    /// Query validation rules for a value object, ordered by idx.
    fn query_vo_rules(
        &self,
        ws: &str,
        ctx: &str,
        vo: &str,
        state: &str,
    ) -> Vec<String> {
        let params = params_map(&[
            ("ws", ws),
            ("ctx", ctx),
            ("vo", vo),
            ("st", state),
        ]);
        let rows = self
            .db
            .run_script(
                "?[idx, text] := \
                    *vo_rule{workspace: $ws, context: $ctx, value_object: $vo, \
                             idx, state: $st, text}",
                params,
                ScriptMutability::Immutable,
            )
            .map(|r| r.rows)
            .unwrap_or_default();

        let mut indexed: Vec<(i64, String)> =
            rows.iter().map(|r| (dv_i64(&r[0]), dv_str(&r[1]))).collect();
        indexed.sort_by_key(|(i, _)| *i);
        indexed.into_iter().map(|(_, t)| t).collect()
    }

    // ── Private: State Decomposition ───────────────────────────────────────

    /// Decompose a DomainModel into relational rows tagged with `state`.
    fn save_state(&self, workspace: &str, model: &DomainModel, state: &str) -> Result<()> {
        self.clear_state(workspace, state)?;
        self.save_owner_meta(workspace, "", "project", &model.name, &model.ownership, state)?;

        for bc in &model.bounded_contexts {
            let params = params_map(&[
                ("ws", workspace),
                ("name", &bc.name),
                ("st", state),
                ("desc", &bc.description),
                ("mp", &bc.module_path),
            ]);
            self.db.run_script(
                "?[workspace, name, state, description, module_path] <- [[$ws, $name, $st, $desc, $mp]] :put context { workspace, name, state => description, module_path }",
                params,
                ScriptMutability::Mutable,
            ).map_err(|e| anyhow::anyhow!("save context '{}': {:?}", bc.name, e))?;

            self.save_owner_meta(workspace, &bc.name, "context", &bc.name, &bc.ownership, state)?;

            for dep in &bc.dependencies {
                self.db.run_script(
                    "?[workspace, from_ctx, to_ctx, state] <- [[$ws, $from, $to, $st]] :put context_dep { workspace, from_ctx, to_ctx, state }",
                    params_map(&[("ws", workspace), ("from", &bc.name), ("to", dep), ("st", state)]),
                    ScriptMutability::Mutable,
                ).map_err(|e| anyhow::anyhow!("save context_dep: {:?}", e))?;
            }

            for aggregate in &bc.aggregates {
                self.db.run_script(
                    "?[workspace, context, name, state, description, root_entity] <- [[$ws, $ctx, $name, $st, $desc, $root]] :put aggregate { workspace, context, name, state => description, root_entity }",
                    params_map(&[("ws", workspace), ("ctx", &bc.name), ("name", &aggregate.name), ("st", state), ("desc", &aggregate.description), ("root", &aggregate.root_entity)]),
                    ScriptMutability::Mutable,
                ).map_err(|e| anyhow::anyhow!("save aggregate '{}': {:?}", aggregate.name, e))?;
                self.save_owner_meta(workspace, &bc.name, "aggregate", &aggregate.name, &aggregate.ownership, state)?;
                for entity in &aggregate.entities {
                    self.db.run_script(
                        "?[workspace, context, aggregate, member_kind, member, state] <- [[$ws, $ctx, $agg, 'entity', $member, $st]] :put aggregate_member { workspace, context, aggregate, member_kind, member, state }",
                        params_map(&[("ws", workspace), ("ctx", &bc.name), ("agg", &aggregate.name), ("member", entity), ("st", state)]),
                        ScriptMutability::Mutable,
                    ).map_err(|e| anyhow::anyhow!("save aggregate entity member: {:?}", e))?;
                }
                for value_object in &aggregate.value_objects {
                    self.db.run_script(
                        "?[workspace, context, aggregate, member_kind, member, state] <- [[$ws, $ctx, $agg, 'value_object', $member, $st]] :put aggregate_member { workspace, context, aggregate, member_kind, member, state }",
                        params_map(&[("ws", workspace), ("ctx", &bc.name), ("agg", &aggregate.name), ("member", value_object), ("st", state)]),
                        ScriptMutability::Mutable,
                    ).map_err(|e| anyhow::anyhow!("save aggregate value_object member: {:?}", e))?;
                }
            }

            for entity in &bc.entities {
                let mut params = params_map(&[("ws", workspace), ("ctx", &bc.name), ("name", &entity.name), ("st", state), ("desc", &entity.description)]);
                params.insert("agg".into(), cozo::DataValue::Bool(entity.aggregate_root));
                params.insert("file".into(), cozo::DataValue::Str(entity.file_path.as_deref().unwrap_or("").into()));
                params.insert("sl".into(), int_dv(entity.start_line.unwrap_or(0) as i64));
                params.insert("el".into(), int_dv(entity.end_line.unwrap_or(0) as i64));
                self.db.run_script(
                    "?[workspace, context, name, state, description, aggregate_root, file_path, start_line, end_line] <- [[$ws, $ctx, $name, $st, $desc, $agg, $file, $sl, $el]] :put entity { workspace, context, name, state => description, aggregate_root, file_path, start_line, end_line }",
                    params,
                    ScriptMutability::Mutable,
                ).map_err(|e| anyhow::anyhow!("save entity '{}': {:?}", entity.name, e))?;
                self.save_fields(workspace, &bc.name, "entity", &entity.name, &entity.fields, state)?;
                self.save_methods(workspace, &bc.name, "entity", &entity.name, &entity.methods, state)?;
                for (idx, inv) in entity.invariants.iter().enumerate() {
                    let mut params = params_map(&[("ws", workspace), ("ctx", &bc.name), ("ent", &entity.name), ("st", state), ("text", inv)]);
                    params.insert("idx".into(), int_dv(idx as i64));
                    self.db.run_script(
                        "?[workspace, context, entity, idx, state, text] <- [[$ws, $ctx, $ent, $idx, $st, $text]] :put invariant { workspace, context, entity, idx, state => text }",
                        params,
                        ScriptMutability::Mutable,
                    ).map_err(|e| anyhow::anyhow!("save invariant: {:?}", e))?;
                }
            }

            for policy in &bc.policies {
                let kind_str = Self::policy_kind_key(&policy.kind).to_string();
                self.db.run_script(
                    "?[workspace, context, name, state, description, kind] <- [[$ws, $ctx, $name, $st, $desc, $kind]] :put policy { workspace, context, name, state => description, kind }",
                    params_map(&[("ws", workspace), ("ctx", &bc.name), ("name", &policy.name), ("st", state), ("desc", &policy.description), ("kind", &kind_str)]),
                    ScriptMutability::Mutable,
                ).map_err(|e| anyhow::anyhow!("save policy '{}': {:?}", policy.name, e))?;
                self.save_owner_meta(workspace, &bc.name, "policy", &policy.name, &policy.ownership, state)?;
                for (idx, trigger) in policy.triggers.iter().enumerate() {
                    let mut params = params_map(&[("ws", workspace), ("ctx", &bc.name), ("policy", &policy.name), ("link", trigger), ("st", state)]);
                    params.insert("idx".into(), int_dv(idx as i64));
                    self.db.run_script(
                        "?[workspace, context, policy, link_kind, link, idx, state] <- [[$ws, $ctx, $policy, 'trigger', $link, $idx, $st]] :put policy_link { workspace, context, policy, link_kind, link, idx, state }",
                        params,
                        ScriptMutability::Mutable,
                    ).map_err(|e| anyhow::anyhow!("save policy trigger: {:?}", e))?;
                }
                for (idx, command) in policy.commands.iter().enumerate() {
                    let mut params = params_map(&[("ws", workspace), ("ctx", &bc.name), ("policy", &policy.name), ("link", command), ("st", state)]);
                    params.insert("idx".into(), int_dv(idx as i64));
                    self.db.run_script(
                        "?[workspace, context, policy, link_kind, link, idx, state] <- [[$ws, $ctx, $policy, 'command', $link, $idx, $st]] :put policy_link { workspace, context, policy, link_kind, link, idx, state }",
                        params,
                        ScriptMutability::Mutable,
                    ).map_err(|e| anyhow::anyhow!("save policy command: {:?}", e))?;
                }
            }

            for read_model in &bc.read_models {
                self.db.run_script(
                    "?[workspace, context, name, state, description, source] <- [[$ws, $ctx, $name, $st, $desc, $src]] :put read_model { workspace, context, name, state => description, source }",
                    params_map(&[("ws", workspace), ("ctx", &bc.name), ("name", &read_model.name), ("st", state), ("desc", &read_model.description), ("src", &read_model.source)]),
                    ScriptMutability::Mutable,
                ).map_err(|e| anyhow::anyhow!("save read_model '{}': {:?}", read_model.name, e))?;
                self.save_owner_meta(workspace, &bc.name, "read_model", &read_model.name, &read_model.ownership, state)?;
                self.save_fields(workspace, &bc.name, "read_model", &read_model.name, &read_model.fields, state)?;
            }

            
            for ep in &bc.api_endpoints {
                let params = params_map(&[
                    ("ws", workspace),
                    ("ctx", &bc.name),
                    ("id", &ep.id),
                    ("st", state),
                    ("svc", &ep.service_id),
                    ("met", &ep.method),
                    ("path", &ep.route_pattern),
                    ("desc", &ep.description),
                ]);
                self.db.run_script(
                    "?[workspace, context, id, state, service_id, method, route_pattern, description] <- \
                     [[$ws, $ctx, $id, $st, $svc, $met, $path, $desc]] \
                     :put api_endpoint { workspace, context, id, state => service_id, method, route_pattern, description }",
                    params,
                    ScriptMutability::Mutable,
                ).map_err(|e| anyhow::anyhow!("save api_endpoint: {:?}", e))?;
            }
for svc in &bc.services {
                let kind_str = format!("{:?}", svc.kind).to_lowercase();
                let mut params = params_map(&[("ws", workspace), ("ctx", &bc.name), ("name", &svc.name), ("st", state), ("desc", &svc.description), ("kind", &kind_str)]);
                params.insert("file".into(), cozo::DataValue::Str(svc.file_path.as_deref().unwrap_or("").into()));
                params.insert("sl".into(), int_dv(svc.start_line.unwrap_or(0) as i64));
                params.insert("el".into(), int_dv(svc.end_line.unwrap_or(0) as i64));
                self.db.run_script(
                    "?[workspace, context, name, state, description, kind, file_path, start_line, end_line] <- [[$ws, $ctx, $name, $st, $desc, $kind, $file, $sl, $el]] :put service { workspace, context, name, state => description, kind, file_path, start_line, end_line }",
                    params,
                    ScriptMutability::Mutable,
                ).map_err(|e| anyhow::anyhow!("save service '{}': {:?}", svc.name, e))?;
                self.save_methods(workspace, &bc.name, "service", &svc.name, &svc.methods, state)?;
                for dep in &svc.dependencies {
                    self.db.run_script(
                        "?[workspace, context, service, dep, state] <- [[$ws, $ctx, $svc, $dep, $st]] :put service_dep { workspace, context, service, dep, state }",
                        params_map(&[("ws", workspace), ("ctx", &bc.name), ("svc", &svc.name), ("dep", dep), ("st", state)]),
                        ScriptMutability::Mutable,
                    ).map_err(|e| anyhow::anyhow!("save service_dep: {:?}", e))?;
                }
            }

            for evt in &bc.events {
                let mut params = params_map(&[("ws", workspace), ("ctx", &bc.name), ("name", &evt.name), ("st", state), ("desc", &evt.description), ("src", &evt.source)]);
                params.insert("file".into(), cozo::DataValue::Str(evt.file_path.as_deref().unwrap_or("").into()));
                params.insert("sl".into(), int_dv(evt.start_line.unwrap_or(0) as i64));
                params.insert("el".into(), int_dv(evt.end_line.unwrap_or(0) as i64));
                self.db.run_script(
                    "?[workspace, context, name, state, description, source, file_path, start_line, end_line] <- [[$ws, $ctx, $name, $st, $desc, $src, $file, $sl, $el]] :put event { workspace, context, name, state => description, source, file_path, start_line, end_line }",
                    params,
                    ScriptMutability::Mutable,
                ).map_err(|e| anyhow::anyhow!("save event '{}': {:?}", evt.name, e))?;
                self.save_fields(workspace, &bc.name, "event", &evt.name, &evt.fields, state)?;
            }

            for vo in &bc.value_objects {
                let mut params = params_map(&[("ws", workspace), ("ctx", &bc.name), ("name", &vo.name), ("st", state), ("desc", &vo.description)]);
                params.insert("file".into(), cozo::DataValue::Str(vo.file_path.as_deref().unwrap_or("").into()));
                params.insert("sl".into(), int_dv(vo.start_line.unwrap_or(0) as i64));
                params.insert("el".into(), int_dv(vo.end_line.unwrap_or(0) as i64));
                self.db.run_script(
                    "?[workspace, context, name, state, description, file_path, start_line, end_line] <- [[$ws, $ctx, $name, $st, $desc, $file, $sl, $el]] :put value_object { workspace, context, name, state => description, file_path, start_line, end_line }",
                    params,
                    ScriptMutability::Mutable,
                ).map_err(|e| anyhow::anyhow!("save value_object '{}': {:?}", vo.name, e))?;
                self.save_fields(workspace, &bc.name, "value_object", &vo.name, &vo.fields, state)?;
                for (idx, rule) in vo.validation_rules.iter().enumerate() {
                    let mut p = params_map(&[("ws", workspace), ("ctx", &bc.name), ("vo", &vo.name), ("st", state), ("text", rule)]);
                    p.insert("idx".into(), int_dv(idx as i64));
                    self.db.run_script(
                        "?[workspace, context, value_object, idx, state, text] <- [[$ws, $ctx, $vo, $idx, $st, $text]] :put vo_rule { workspace, context, value_object, idx, state => text }",
                        p,
                        ScriptMutability::Mutable,
                    ).map_err(|e| anyhow::anyhow!("save vo_rule: {:?}", e))?;
                }
            }

            for repo in &bc.repositories {
                let mut params = params_map(&[("ws", workspace), ("ctx", &bc.name), ("name", &repo.name), ("st", state), ("agg", &repo.aggregate)]);
                params.insert("file".into(), cozo::DataValue::Str(repo.file_path.as_deref().unwrap_or("").into()));
                params.insert("sl".into(), int_dv(repo.start_line.unwrap_or(0) as i64));
                params.insert("el".into(), int_dv(repo.end_line.unwrap_or(0) as i64));
                self.db.run_script(
                    "?[workspace, context, name, state, aggregate, file_path, start_line, end_line] <- [[$ws, $ctx, $name, $st, $agg, $file, $sl, $el]] :put repository { workspace, context, name, state => aggregate, file_path, start_line, end_line }",
                    params,
                    ScriptMutability::Mutable,
                ).map_err(|e| anyhow::anyhow!("save repository '{}': {:?}", repo.name, e))?;
                self.save_methods(workspace, &bc.name, "repository", &repo.name, &repo.methods, state)?;
            }

            for module in &bc.modules {
                let mut params = params_map(&[
                    ("ws", workspace),
                    ("ctx", &bc.name),
                    ("name", &module.name),
                    ("st", state),
                    ("path", &module.path),
                    ("fp", &module.file_path),
                    ("desc", &module.description),
                ]);
                params.insert("public".into(), cozo::DataValue::Bool(module.public));
                self.db.run_script(
                    "?[workspace, context, name, state, path, public, file_path, description] <- [[$ws, $ctx, $name, $st, $path, $public, $fp, $desc]] :put module { workspace, context, name, state => path, public, file_path, description }",
                    params,
                    ScriptMutability::Mutable,
                ).map_err(|e| anyhow::anyhow!("save module '{}': {:?}", module.name, e))?;
            }
        }

        for system in &model.external_systems {
            self.db.run_script(
                "?[workspace, name, state, description, kind, rationale] <- [[$ws, $name, $st, $desc, $kind, $rationale]] :put external_system { workspace, name, state => description, kind, rationale }",
                params_map(&[("ws", workspace), ("name", &system.name), ("st", state), ("desc", &system.description), ("kind", &system.kind), ("rationale", &system.rationale)]),
                ScriptMutability::Mutable,
            ).map_err(|e| anyhow::anyhow!("save external_system '{}': {:?}", system.name, e))?;
            self.save_owner_meta(workspace, "", "external_system", &system.name, &system.ownership, state)?;
            for (idx, ctx) in system.consumed_by_contexts.iter().enumerate() {
                let mut params = params_map(&[("ws", workspace), ("name", &system.name), ("ctx", ctx), ("st", state)]);
                params.insert("idx".into(), int_dv(idx as i64));
                self.db.run_script(
                    "?[workspace, system, context, idx, state] <- [[$ws, $name, $ctx, $idx, $st]] :put external_system_context { workspace, system, context, idx, state }",
                    params,
                    ScriptMutability::Mutable,
                ).map_err(|e| anyhow::anyhow!("save external_system_context: {:?}", e))?;
            }
        }

        for decision in &model.architectural_decisions {
            let status = format!("{:?}", decision.status).to_lowercase();
            self.db.run_script(
                "?[workspace, id, state, title, status, scope, date, rationale] <- [[$ws, $id, $st, $title, $status, $scope, $date, $rationale]] :put architectural_decision { workspace, id, state => title, status, scope, date, rationale }",
                params_map(&[("ws", workspace), ("id", &decision.id), ("st", state), ("title", &decision.title), ("status", &status), ("scope", &decision.scope), ("date", &decision.date), ("rationale", &decision.rationale)]),
                ScriptMutability::Mutable,
            ).map_err(|e| anyhow::anyhow!("save architectural_decision '{}': {:?}", decision.id, e))?;
            self.save_owner_meta(workspace, "", "architectural_decision", &decision.id, &decision.ownership, state)?;
            for (idx, ctx) in decision.contexts.iter().enumerate() {
                let mut params = params_map(&[("ws", workspace), ("id", &decision.id), ("ctx", ctx), ("st", state)]);
                params.insert("idx".into(), int_dv(idx as i64));
                self.db.run_script(
                    "?[workspace, decision_id, context, idx, state] <- [[$ws, $id, $ctx, $idx, $st]] :put decision_context { workspace, decision_id, context, idx, state }",
                    params,
                    ScriptMutability::Mutable,
                ).map_err(|e| anyhow::anyhow!("save decision_context: {:?}", e))?;
            }
            for (idx, consequence) in decision.consequences.iter().enumerate() {
                let mut params = params_map(&[("ws", workspace), ("id", &decision.id), ("text", consequence), ("st", state)]);
                params.insert("idx".into(), int_dv(idx as i64));
                self.db.run_script(
                    "?[workspace, decision_id, idx, state, text] <- [[$ws, $id, $idx, $st, $text]] :put decision_consequence { workspace, decision_id, idx, state => text }",
                    params,
                    ScriptMutability::Mutable,
                ).map_err(|e| anyhow::anyhow!("save decision_consequence: {:?}", e))?;
            }
        }

        // Save AST edges
        for edge in &model.ast_edges {
            self.db.run_script(
                "?[workspace, state, from_node, to_node, edge_type] <- [[$ws, $st, $from, $to, $kind]] :put ast_edge { workspace, state, from_node, to_node, edge_type }",
                params_map(&[
                    ("ws", workspace),
                    ("st", state),
                    ("from", &edge.from_node),
                    ("to", &edge.to_node),
                    ("kind", &edge.edge_type),
                ]),
                ScriptMutability::Mutable,
            ).map_err(|e| anyhow::anyhow!("save ast_edge: {:?}", e))?;
        }

        Ok(())
    }

    /// Remove all rows for a workspace+state across all domain tables.
    fn clear_state(&self, workspace: &str, state: &str) -> Result<()> {
        let params = params_map(&[("ws", workspace), ("st", state)]);
        let tables = [
            ("owner_meta", "workspace, context, owner_kind, owner, state"),
            ("context", "workspace, name, state"),
            ("context_dep", "workspace, from_ctx, to_ctx, state"),
            ("aggregate", "workspace, context, name, state"),
            ("aggregate_member", "workspace, context, aggregate, member_kind, member, state"),
            ("entity", "workspace, context, name, state"),
            ("policy", "workspace, context, name, state"),
            ("policy_link", "workspace, context, policy, link_kind, link, idx, state"),
            ("read_model", "workspace, context, name, state"),
            ("service", "workspace, context, name, state"),
            ("service_dep", "workspace, context, service, dep, state"),
            ("event", "workspace, context, name, state"),
            ("value_object", "workspace, context, name, state"),
            ("repository", "workspace, context, name, state"),
            ("module", "workspace, context, name, state"),
            ("external_system", "workspace, name, state"),
            ("external_system_context", "workspace, system, context, idx, state"),
            ("architectural_decision", "workspace, id, state"),
            ("decision_context", "workspace, decision_id, context, idx, state"),
            ("decision_consequence", "workspace, decision_id, idx, state"),
            ("invariant", "workspace, context, entity, idx, state"),
            ("field", "workspace, context, owner_kind, owner, name, state"),
            ("method", "workspace, context, owner_kind, owner, name, state"),
            ("method_param", "workspace, context, owner_kind, owner, method, name, state"),
            ("vo_rule", "workspace, context, value_object, idx, state"),
            ("ast_edge", "workspace, state, from_node, to_node, edge_type"),
        ];
        for (rel, keys) in tables {
            let script = format!(
                "?[{keys}] := *{rel}{{{keys}}}, workspace = $ws, state = $st :rm {rel} {{{keys}}}"
            );
            let _ = self
                .db
                .run_script(&script, params.clone(), ScriptMutability::Mutable);
        }
        Ok(())
    }

    /// Copy all rows from one state to another via Datalog.
    fn copy_state(&self, workspace: &str, from: &str, to: &str) -> Result<()> {
        self.clear_state(workspace, to)?;
        let params = params_map(&[("ws", workspace), ("from", from), ("to", to)]);

        let scripts = vec![
            // owner_meta
            "src[ws, c, ok, ow, team, owners, rationale] := *owner_meta{workspace: ws, context: c, owner_kind: ok, owner: ow, state: $from, team, owners_json: owners, rationale}, ws = $ws \
             ?[workspace, context, owner_kind, owner, state, team, owners_json, rationale] := src[workspace, context, owner_kind, owner, team, owners_json, rationale], state = $to \
             :put owner_meta {workspace, context, owner_kind, owner, state => team, owners_json, rationale}",
            // context
            "src[ws, n, d, m] := *context{workspace: ws, name: n, state: $from, description: d, module_path: m}, ws = $ws \
             ?[workspace, name, state, description, module_path] := src[workspace, name, description, module_path], state = $to \
             :put context {workspace, name, state => description, module_path}",
            // context_dep
            "src[ws, f, t] := *context_dep{workspace: ws, from_ctx: f, to_ctx: t, state: $from}, ws = $ws \
             ?[workspace, from_ctx, to_ctx, state] := src[workspace, from_ctx, to_ctx], state = $to \
             :put context_dep {workspace, from_ctx, to_ctx, state}",
            // aggregate
            "src[ws, c, n, d, root] := *aggregate{workspace: ws, context: c, name: n, state: $from, description: d, root_entity: root}, ws = $ws \
             ?[workspace, context, name, state, description, root_entity] := src[workspace, context, name, description, root_entity], state = $to \
             :put aggregate {workspace, context, name, state => description, root_entity}",
            // aggregate_member
            "src[ws, c, a, mk, m] := *aggregate_member{workspace: ws, context: c, aggregate: a, member_kind: mk, member: m, state: $from}, ws = $ws \
             ?[workspace, context, aggregate, member_kind, member, state] := src[workspace, context, aggregate, member_kind, member], state = $to \
             :put aggregate_member {workspace, context, aggregate, member_kind, member, state}",
            // entity (no JSON columns)
            "src[ws, c, n, d, a] := *entity{workspace: ws, context: c, name: n, state: $from, description: d, aggregate_root: a}, ws = $ws \
             ?[workspace, context, name, state, description, aggregate_root] := src[workspace, context, name, description, aggregate_root], state = $to \
             :put entity {workspace, context, name, state => description, aggregate_root}",
            // policy
            "src[ws, c, n, d, k] := *policy{workspace: ws, context: c, name: n, state: $from, description: d, kind: k}, ws = $ws \
             ?[workspace, context, name, state, description, kind] := src[workspace, context, name, description, kind], state = $to \
             :put policy {workspace, context, name, state => description, kind}",
            // policy_link
            "src[ws, c, p, lk, l, i] := *policy_link{workspace: ws, context: c, policy: p, link_kind: lk, link: l, idx: i, state: $from}, ws = $ws \
             ?[workspace, context, policy, link_kind, link, idx, state] := src[workspace, context, policy, link_kind, link, idx], state = $to \
             :put policy_link {workspace, context, policy, link_kind, link, idx, state}",
            // read_model
            "src[ws, c, n, d, s] := *read_model{workspace: ws, context: c, name: n, state: $from, description: d, source: s}, ws = $ws \
             ?[workspace, context, name, state, description, source] := src[workspace, context, name, description, source], state = $to \
             :put read_model {workspace, context, name, state => description, source}",
            // service (no JSON columns)
            "src[ws, c, n, d, k] := *service{workspace: ws, context: c, name: n, state: $from, description: d, kind: k}, ws = $ws \
             ?[workspace, context, name, state, description, kind] := src[workspace, context, name, description, kind], state = $to \
             :put service {workspace, context, name, state => description, kind}",
            // service_dep
            "src[ws, c, s, d] := *service_dep{workspace: ws, context: c, service: s, dep: d, state: $from}, ws = $ws \
             ?[workspace, context, service, dep, state] := src[workspace, context, service, dep], state = $to \
             :put service_dep {workspace, context, service, dep, state}",
            // event (no JSON columns)
            "src[ws, c, n, d, s] := *event{workspace: ws, context: c, name: n, state: $from, description: d, source: s}, ws = $ws \
             ?[workspace, context, name, state, description, source] := src[workspace, context, name, description, source], state = $to \
             :put event {workspace, context, name, state => description, source}",
            // value_object (no JSON columns)
            "src[ws, c, n, d] := *value_object{workspace: ws, context: c, name: n, state: $from, description: d}, ws = $ws \
             ?[workspace, context, name, state, description] := src[workspace, context, name, description], state = $to \
             :put value_object {workspace, context, name, state => description}",
            // repository (no JSON columns)
            "src[ws, c, n, a] := *repository{workspace: ws, context: c, name: n, state: $from, aggregate: a}, ws = $ws \
             ?[workspace, context, name, state, aggregate] := src[workspace, context, name, aggregate], state = $to \
             :put repository {workspace, context, name, state => aggregate}",
            // external_system
            "src[ws, n, d, k, r] := *external_system{workspace: ws, name: n, state: $from, description: d, kind: k, rationale: r}, ws = $ws \
             ?[workspace, name, state, description, kind, rationale] := src[workspace, name, description, kind, rationale], state = $to \
             :put external_system {workspace, name, state => description, kind, rationale}",
            // external_system_context
            "src[ws, s, c, i] := *external_system_context{workspace: ws, system: s, context: c, idx: i, state: $from}, ws = $ws \
             ?[workspace, system, context, idx, state] := src[workspace, system, context, idx], state = $to \
             :put external_system_context {workspace, system, context, idx, state}",
            // architectural_decision
            "src[ws, id, title, status, scope, date, rationale] := *architectural_decision{workspace: ws, id, state: $from, title, status, scope, date, rationale}, ws = $ws \
             ?[workspace, id, state, title, status, scope, date, rationale] := src[workspace, id, title, status, scope, date, rationale], state = $to \
             :put architectural_decision {workspace, id, state => title, status, scope, date, rationale}",
            // decision_context
            "src[ws, id, c, i] := *decision_context{workspace: ws, decision_id: id, context: c, idx: i, state: $from}, ws = $ws \
             ?[workspace, decision_id, context, idx, state] := src[workspace, decision_id, context, idx], state = $to \
             :put decision_context {workspace, decision_id, context, idx, state}",
            // decision_consequence
            "src[ws, id, i, text] := *decision_consequence{workspace: ws, decision_id: id, idx: i, state: $from, text}, ws = $ws \
             ?[workspace, decision_id, idx, state, text] := src[workspace, decision_id, idx, text], state = $to \
             :put decision_consequence {workspace, decision_id, idx, state => text}",
            // invariant
            "src[ws, c, e, i, t] := *invariant{workspace: ws, context: c, entity: e, idx: i, state: $from, text: t}, ws = $ws \
             ?[workspace, context, entity, idx, state, text] := src[workspace, context, entity, idx, text], state = $to \
             :put invariant {workspace, context, entity, idx, state => text}",
            // field
            "src[ws, c, ok, ow, n, ft, req, desc, idx] := *field{workspace: ws, context: c, owner_kind: ok, owner: ow, name: n, state: $from, field_type: ft, required: req, description: desc, idx}, ws = $ws \
             ?[workspace, context, owner_kind, owner, name, state, field_type, required, description, idx] := src[workspace, context, owner_kind, owner, name, field_type, required, description, idx], state = $to \
             :put field {workspace, context, owner_kind, owner, name, state => field_type, required, description, idx}",
            // method
            "src[ws, c, ok, ow, n, desc, rt, idx] := *method{workspace: ws, context: c, owner_kind: ok, owner: ow, name: n, state: $from, description: desc, return_type: rt, idx}, ws = $ws \
             ?[workspace, context, owner_kind, owner, name, state, description, return_type, idx] := src[workspace, context, owner_kind, owner, name, description, return_type, idx], state = $to \
             :put method {workspace, context, owner_kind, owner, name, state => description, return_type, idx}",
            // method_param
            "src[ws, c, ok, ow, m, n, pt, req, desc, idx] := *method_param{workspace: ws, context: c, owner_kind: ok, owner: ow, method: m, name: n, state: $from, param_type: pt, required: req, description: desc, idx}, ws = $ws \
             ?[workspace, context, owner_kind, owner, method, name, state, param_type, required, description, idx] := src[workspace, context, owner_kind, owner, method, name, param_type, required, description, idx], state = $to \
             :put method_param {workspace, context, owner_kind, owner, method, name, state => param_type, required, description, idx}",
            // vo_rule
            "src[ws, c, vo, i, t] := *vo_rule{workspace: ws, context: c, value_object: vo, idx: i, state: $from, text: t}, ws = $ws \
             ?[workspace, context, value_object, idx, state, text] := src[workspace, context, value_object, idx, text], state = $to \
             :put vo_rule {workspace, context, value_object, idx, state => text}",
        ];

        for script in scripts {
            let _ = self
                .db
                .run_script(script, params.clone(), ScriptMutability::Mutable);
        }
        Ok(())
    }

    /// Reconstruct a DomainModel from relational rows for a given state.
    fn reconstruct_model(
        &self,
        workspace_path: &str,
        state: &str,
    ) -> Result<Option<DomainModel>> {
        let ws = canonicalize_path(workspace_path);
        let p = params_map(&[("ws", &ws), ("st", state)]);

        // Project metadata
        let proj = self
            .db
            .run_script(
                "?[name, description, rules_json, tech_stack_json, conventions_json] := \
                    *project{workspace: $ws, name, description, rules_json, tech_stack_json, conventions_json}",
                params_map(&[("ws", &ws)]),
                ScriptMutability::Immutable,
            )
            .ok();

        // Contexts for this state
        let ctxs = self
            .db
            .run_script(
                "?[name, description, module_path] := \
                    *context{workspace: $ws, name, state: $st, description, module_path}",
                p.clone(),
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("reconstruct contexts: {:?}", e))?;

        let has_project = proj
            .as_ref()
            .map(|r| !r.rows.is_empty())
            .unwrap_or(false);

        if ctxs.rows.is_empty()
            && (state == "actual" || !has_project)
        {
            return Ok(None);
        }

        // Extract project-level metadata
        let (project_name, description, rules, tech_stack, conventions) = if has_project {
            let r = &proj.unwrap().rows[0];
            (
                dv_str(&r[0]),
                dv_str(&r[1]),
                serde_json::from_str::<Vec<ArchitecturalRule>>(&dv_str(&r[2]))
                    .unwrap_or_default(),
                serde_json::from_str::<TechStack>(&dv_str(&r[3])).unwrap_or_default(),
                serde_json::from_str::<Conventions>(&dv_str(&r[4])).unwrap_or_default(),
            )
        } else {
            let name = std::path::Path::new(workspace_path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "Unnamed".into());
            (
                name,
                String::new(),
                vec![],
                TechStack::default(),
                Conventions::default(),
            )
        };

        let project_ownership = self.query_ownership(&ws, "", "project", &project_name, state);

        // Reconstruct each bounded context
        let mut bounded_contexts = Vec::new();
        for row in &ctxs.rows {
            let ctx_name = dv_str(&row[0]);

            // Dependencies
            let deps = self
                .db
                .run_script(
                    "?[to_ctx] := *context_dep{workspace: $ws, from_ctx: $ctx, to_ctx, state: $st}",
                    params_map(&[("ws", &ws), ("ctx", &ctx_name), ("st", state)]),
                    ScriptMutability::Immutable,
                )
                .map(|r| r.rows)
                .unwrap_or_default();
            let dependencies: Vec<String> = deps.iter().map(|r| dv_str(&r[0])).collect();

            let ownership = self.query_ownership(&ws, &ctx_name, "context", &ctx_name, state);

            let aggs = self
                .db
                .run_script(
                    "?[name, description, root_entity] := *aggregate{workspace: $ws, context: $ctx, name, state: $st, description, root_entity}",
                    params_map(&[("ws", &ws), ("ctx", &ctx_name), ("st", state)]),
                    ScriptMutability::Immutable,
                )
                .map(|r| r.rows)
                .unwrap_or_default();
            let aggregates: Vec<Aggregate> = aggs
                .iter()
                .map(|r| {
                    let aggregate_name = dv_str(&r[0]);
                    let members = self
                        .db
                        .run_script(
                            "?[member_kind, member] := *aggregate_member{workspace: $ws, context: $ctx, aggregate: $agg, member_kind, member, state: $st}",
                            params_map(&[("ws", &ws), ("ctx", &ctx_name), ("agg", &aggregate_name), ("st", state)]),
                            ScriptMutability::Immutable,
                        )
                        .map(|r| r.rows)
                        .unwrap_or_default();
                    Aggregate {
                        name: aggregate_name.clone(),
                        description: dv_str(&r[1]),
                        root_entity: dv_str(&r[2]),
                        entities: members.iter().filter(|m| dv_str(&m[0]) == "entity").map(|m| dv_str(&m[1])).collect(),
                        value_objects: members.iter().filter(|m| dv_str(&m[0]) == "value_object").map(|m| dv_str(&m[1])).collect(),
                        ownership: self.query_ownership(&ws, &ctx_name, "aggregate", &aggregate_name, state),
                    }
                })
                .collect();

            // Entities
            let ents = self
                .db
                .run_script(
                    "?[name, description, aggregate_root] := \
                        *entity{workspace: $ws, context: $ctx, name, state: $st, \
                                description, aggregate_root}",
                    params_map(&[("ws", &ws), ("ctx", &ctx_name), ("st", state)]),
                    ScriptMutability::Immutable,
                )
                .map(|r| r.rows)
                .unwrap_or_default();
            let entities: Vec<Entity> = ents
                .iter()
                .map(|r| {
                    let ename = dv_str(&r[0]);
                    Entity {
                        name: ename.clone(),
                        description: dv_str(&r[1]),
                        aggregate_root: matches!(&r[2], cozo::DataValue::Bool(true)),
                        fields: self.query_fields(&ws, &ctx_name, "entity", &ename, state),
                        methods: self.query_methods(&ws, &ctx_name, "entity", &ename, state),
                        invariants: self.query_invariants(&ws, &ctx_name, &ename, state),
                        file_path: None,
                        start_line: None,
                        end_line: None,
                    }
                })
                .collect();

            let policy_rows = self
                .db
                .run_script(
                    "?[name, description, kind] := *policy{workspace: $ws, context: $ctx, name, state: $st, description, kind}",
                    params_map(&[("ws", &ws), ("ctx", &ctx_name), ("st", state)]),
                    ScriptMutability::Immutable,
                )
                .map(|r| r.rows)
                .unwrap_or_default();
            let policies: Vec<Policy> = policy_rows
                .iter()
                .map(|r| {
                    let policy_name = dv_str(&r[0]);
                    let links = self
                        .db
                        .run_script(
                            "?[idx, link_kind, link] := *policy_link{workspace: $ws, context: $ctx, policy: $policy, idx, state: $st, link_kind, link}",
                            params_map(&[("ws", &ws), ("ctx", &ctx_name), ("policy", &policy_name), ("st", state)]),
                            ScriptMutability::Immutable,
                        )
                        .map(|r| r.rows)
                        .unwrap_or_default();
                    let mut indexed = links.iter().map(|row| (dv_i64(&row[0]), dv_str(&row[1]), dv_str(&row[2]))).collect::<Vec<_>>();
                    indexed.sort_by_key(|(idx, _, _)| *idx);
                    Policy {
                        name: policy_name.clone(),
                        description: dv_str(&r[1]),
                        kind: match dv_str(&r[2]).as_str() {
                            "process_manager" => PolicyKind::ProcessManager,
                            "integration" => PolicyKind::Integration,
                            _ => PolicyKind::Domain,
                        },
                        triggers: indexed.iter().filter(|(_, kind, _)| kind == "trigger").map(|(_, _, link)| link.clone()).collect(),
                        commands: indexed.iter().filter(|(_, kind, _)| kind == "command").map(|(_, _, link)| link.clone()).collect(),
                        ownership: self.query_ownership(&ws, &ctx_name, "policy", &policy_name, state),
                    }
                })
                .collect();

            let read_model_rows = self
                .db
                .run_script(
                    "?[name, description, source] := *read_model{workspace: $ws, context: $ctx, name, state: $st, description, source}",
                    params_map(&[("ws", &ws), ("ctx", &ctx_name), ("st", state)]),
                    ScriptMutability::Immutable,
                )
                .map(|r| r.rows)
                .unwrap_or_default();
            let read_models: Vec<ReadModel> = read_model_rows
                .iter()
                .map(|r| {
                    let read_name = dv_str(&r[0]);
                    ReadModel {
                        name: read_name.clone(),
                        description: dv_str(&r[1]),
                        source: dv_str(&r[2]),
                        fields: self.query_fields(&ws, &ctx_name, "read_model", &read_name, state),
                        ownership: self.query_ownership(&ws, &ctx_name, "read_model", &read_name, state),
                    }
                })
                .collect();

            // Services
            let svcs = self
                .db
                .run_script(
                    "?[name, description, kind] := \
                        *service{workspace: $ws, context: $ctx, name, state: $st, \
                                 description, kind}",
                    params_map(&[("ws", &ws), ("ctx", &ctx_name), ("st", state)]),
                    ScriptMutability::Immutable,
                )
                .map(|r| r.rows)
                .unwrap_or_default();
            let services: Vec<Service> = svcs
                .iter()
                .map(|r| {
                    let svc_name = dv_str(&r[0]);
                    let svc_deps = self
                        .db
                        .run_script(
                            "?[dep] := *service_dep{workspace: $ws, context: $ctx, service: $svc, dep, state: $st}",
                            params_map(&[
                                ("ws", &ws),
                                ("ctx", &ctx_name),
                                ("svc", &svc_name),
                                ("st", state),
                            ]),
                            ScriptMutability::Immutable,
                        )
                        .map(|r| r.rows)
                        .unwrap_or_default();
                    Service {
                        name: svc_name.clone(),
                        description: dv_str(&r[1]),
                        kind: match dv_str(&r[2]).as_str() {
                            "application" => ServiceKind::Application,
                            "infrastructure" => ServiceKind::Infrastructure,
                            _ => ServiceKind::Domain,
                        },
                        methods: self.query_methods(&ws, &ctx_name, "service", &svc_name, state),
                        dependencies: svc_deps.iter().map(|r| dv_str(&r[0])).collect(),
                        file_path: None,
                        start_line: None,
                        end_line: None,
                    }
                })
                .collect();

            // Events
            let evts = self
                .db
                .run_script(
                    "?[name, description, source] := \
                        *event{workspace: $ws, context: $ctx, name, state: $st, \
                               description, source}",
                    params_map(&[("ws", &ws), ("ctx", &ctx_name), ("st", state)]),
                    ScriptMutability::Immutable,
                )
                .map(|r| r.rows)
                .unwrap_or_default();
            let api_endpoints_rows = self.db.run_script(
                "?[id, service_id, method, route_pattern, description] := *api_endpoint{workspace: $ws, context: $ctx, id, state: $st, service_id, method, route_pattern, description}",
                params_map(&[("ws", &ws), ("ctx", &ctx_name), ("st", state)]),
                ScriptMutability::Immutable,
            ).map(|r| r.rows).unwrap_or_default();
            let api_endpoints: Vec<APIEndpoint> = api_endpoints_rows.iter().map(|r| {
                APIEndpoint {
                    id: dv_str(&r[0]),
                    service_id: dv_str(&r[1]),
                    method: dv_str(&r[2]),
                    route_pattern: dv_str(&r[3]),
                    description: dv_str(&r[4]),
                }
            }).collect();

            let events: Vec<DomainEvent> = evts
                .iter()
                .map(|r| {
                    let ename = dv_str(&r[0]);
                    DomainEvent {
                        name: ename.clone(),
                        description: dv_str(&r[1]),
                        source: dv_str(&r[2]),
                        fields: self.query_fields(&ws, &ctx_name, "event", &ename, state),
                        file_path: None,
                        start_line: None,
                        end_line: None,
                    }
                })
                .collect();

            // Value objects
            let vos = self
                .db
                .run_script(
                    "?[name, description] := \
                        *value_object{workspace: $ws, context: $ctx, name, state: $st, description}",
                    params_map(&[("ws", &ws), ("ctx", &ctx_name), ("st", state)]),
                    ScriptMutability::Immutable,
                )
                .map(|r| r.rows)
                .unwrap_or_default();
            let value_objects: Vec<ValueObject> = vos
                .iter()
                .map(|r| {
                    let voname = dv_str(&r[0]);
                    ValueObject {
                        name: voname.clone(),
                        description: dv_str(&r[1]),
                        fields: self.query_fields(&ws, &ctx_name, "value_object", &voname, state),
                        validation_rules: self.query_vo_rules(&ws, &ctx_name, &voname, state),
                        file_path: None,
                        start_line: None,
                        end_line: None,
                    }
                })
                .collect();

            // Repositories
            let repos = self
                .db
                .run_script(
                    "?[name, aggregate] := \
                        *repository{workspace: $ws, context: $ctx, name, state: $st, aggregate}",
                    params_map(&[("ws", &ws), ("ctx", &ctx_name), ("st", state)]),
                    ScriptMutability::Immutable,
                )
                .map(|r| r.rows)
                .unwrap_or_default();
            let repositories: Vec<Repository> = repos
                .iter()
                .map(|r| {
                    let rname = dv_str(&r[0]);
                    Repository {
                        name: rname.clone(),
                        aggregate: dv_str(&r[1]),
                        methods: self.query_methods(&ws, &ctx_name, "repository", &rname, state),
                        file_path: None,
                        start_line: None,
                        end_line: None,
                    }
                })
                .collect();

            // Modules
            let mods = self
                .db
                .run_script(
                    "?[name, path, public, file_path, description] := \
                        *module{workspace: $ws, context: $ctx, name, state: $st, path, public, file_path, description}",
                    params_map(&[("ws", &ws), ("ctx", &ctx_name), ("st", state)]),
                    ScriptMutability::Immutable,
                )
                .map(|r| r.rows)
                .unwrap_or_default();
            let modules: Vec<Module> = mods
                .iter()
                .map(|r| Module {
                    name: dv_str(&r[0]),
                    path: dv_str(&r[1]),
                    public: r[2].get_bool().unwrap_or(false),
                    file_path: dv_str(&r[3]),
                    description: dv_str(&r[4]),
                })
                .collect();

            bounded_contexts.push(BoundedContext {
                name: ctx_name,
                description: dv_str(&row[1]),
                module_path: dv_str(&row[2]),
                ownership,
                aggregates,
                policies,
                read_models,
                entities,
                value_objects,
                services,
                api_endpoints,
                repositories,
                events,
                modules,
                dependencies,
            });
        }

        let external_system_rows = self
            .db
            .run_script(
                "?[name, description, kind, rationale] := *external_system{workspace: $ws, name, state: $st, description, kind, rationale}",
                params_map(&[("ws", &ws), ("st", state)]),
                ScriptMutability::Immutable,
            )
            .map(|r| r.rows)
            .unwrap_or_default();
        let external_systems: Vec<ExternalSystem> = external_system_rows
            .iter()
            .map(|r| {
                let system_name = dv_str(&r[0]);
                ExternalSystem {
                    name: system_name.clone(),
                    description: dv_str(&r[1]),
                    kind: dv_str(&r[2]),
                    consumed_by_contexts: self.query_indexed_strings(
                        "?[idx, context] := *external_system_context{workspace: $ws, system: $name, idx, state: $st, context}",
                        params_map(&[("ws", &ws), ("name", &system_name), ("st", state)]),
                    ),
                    rationale: dv_str(&r[3]),
                    ownership: self.query_ownership(&ws, "", "external_system", &system_name, state),
                }
            })
            .collect();

        let decision_rows = self
            .db
            .run_script(
                "?[id, title, status, scope, date, rationale] := *architectural_decision{workspace: $ws, id, state: $st, title, status, scope, date, rationale}",
                params_map(&[("ws", &ws), ("st", state)]),
                ScriptMutability::Immutable,
            )
            .map(|r| r.rows)
            .unwrap_or_default();
        let architectural_decisions: Vec<ArchitecturalDecision> = decision_rows
            .iter()
            .map(|r| {
                let decision_id = dv_str(&r[0]);
                ArchitecturalDecision {
                    id: decision_id.clone(),
                    title: dv_str(&r[1]),
                    status: match dv_str(&r[2]).as_str() {
                        "accepted" => DecisionStatus::Accepted,
                        "superseded" => DecisionStatus::Superseded,
                        "deprecated" => DecisionStatus::Deprecated,
                        _ => DecisionStatus::Proposed,
                    },
                    scope: dv_str(&r[3]),
                    date: dv_str(&r[4]),
                    rationale: dv_str(&r[5]),
                    consequences: self.query_indexed_strings(
                        "?[idx, text] := *decision_consequence{workspace: $ws, decision_id: $id, idx, state: $st, text}",
                        params_map(&[("ws", &ws), ("id", &decision_id), ("st", state)]),
                    ),
                    contexts: self.query_indexed_strings(
                        "?[idx, context] := *decision_context{workspace: $ws, decision_id: $id, idx, state: $st, context}",
                        params_map(&[("ws", &ws), ("id", &decision_id), ("st", state)]),
                    ),
                    ownership: self.query_ownership(&ws, "", "architectural_decision", &decision_id, state),
                }
            })
            .collect();

        Ok(Some(DomainModel {
            name: project_name,
            description,
            bounded_contexts,
            external_systems,
            architectural_decisions,
            ownership: project_ownership,
            rules,
            tech_stack,
            conventions,
            ast_edges: {
                let rows = self.db.run_script(
                    "?[from_node, to_node, edge_type] := *ast_edge{workspace: $ws, state: $st, from_node, to_node, edge_type}",
                    params_map(&[("ws", &ws), ("st", state)]),
                    ScriptMutability::Immutable,
                ).map(|r| r.rows).unwrap_or_default();
                rows.iter().map(|r| crate::domain::model::ASTEdge {
                    from_node: dv_str(&r[0]),
                    to_node: dv_str(&r[1]),
                    edge_type: dv_str(&r[2]),
                }).collect()
            },
        }))
    }

    // ── Graph-native Query & Mutation Helpers ─────────────────────────────

    pub fn query_entity(&self, ws: &str, ctx: &str, name: &str) -> Option<Entity> {
        let ws = canonicalize_path(ws);
        let rows = self.db.run_script(
            "?[description, aggregate_root] := *entity{workspace: $ws, context: $ctx, name: $name, state: 'desired', description, aggregate_root}",
            params_map(&[("ws", &ws), ("ctx", ctx), ("name", name)]),
            ScriptMutability::Immutable,
        ).ok()?.rows;
        let row = rows.first()?;
        Some(Entity {
            name: name.to_string(),
            description: dv_str(&row[0]),
            aggregate_root: matches!(&row[1], cozo::DataValue::Bool(true)),
            fields: self.query_fields(&ws, ctx, "entity", name, "desired"),
            methods: self.query_methods(&ws, ctx, "entity", name, "desired"),
            invariants: self.query_invariants(&ws, ctx, name, "desired"),
            file_path: None,
            start_line: None,
            end_line: None,
        })
    }

    pub fn query_service(&self, ws: &str, ctx: &str, name: &str) -> Option<Service> {
        let ws = canonicalize_path(ws);
        let rows = self.db.run_script(
            "?[description, kind] := *service{workspace: $ws, context: $ctx, name: $name, state: 'desired', description, kind}",
            params_map(&[("ws", &ws), ("ctx", ctx), ("name", name)]),
            ScriptMutability::Immutable,
        ).ok()?.rows;
        let row = rows.first()?;
        let dep_rows = self.db.run_script(
            "?[dep] := *service_dep{workspace: $ws, context: $ctx, service: $name, dep, state: 'desired'}",
            params_map(&[("ws", &ws), ("ctx", ctx), ("name", name)]),
            ScriptMutability::Immutable,
        ).map(|r| r.rows).unwrap_or_default();
        Some(Service {
            name: name.to_string(),
            description: dv_str(&row[0]),
            kind: match dv_str(&row[1]).as_str() {
                "application" => ServiceKind::Application,
                "infrastructure" => ServiceKind::Infrastructure,
                _ => ServiceKind::Domain,
            },
            methods: self.query_methods(&ws, ctx, "service", name, "desired"),
            dependencies: dep_rows.iter().map(|r| dv_str(&r[0])).collect(),
            file_path: None,
            start_line: None,
            end_line: None,
        })
    }

    pub fn query_event(&self, ws: &str, ctx: &str, name: &str) -> Option<DomainEvent> {
        let ws = canonicalize_path(ws);
        let rows = self.db.run_script(
            "?[description, source] := *event{workspace: $ws, context: $ctx, name: $name, state: 'desired', description, source}",
            params_map(&[("ws", &ws), ("ctx", ctx), ("name", name)]),
            ScriptMutability::Immutable,
        ).ok()?.rows;
        let row = rows.first()?;
        Some(DomainEvent {
            name: name.to_string(),
            description: dv_str(&row[0]),
            fields: self.query_fields(&ws, ctx, "event", name, "desired"),
            source: dv_str(&row[1]),
            file_path: None,
            start_line: None,
            end_line: None,
        })
    }

    pub fn query_value_object(&self, ws: &str, ctx: &str, name: &str) -> Option<ValueObject> {
        let ws = canonicalize_path(ws);
        let rows = self.db.run_script(
            "?[description] := *value_object{workspace: $ws, context: $ctx, name: $name, state: 'desired', description}",
            params_map(&[("ws", &ws), ("ctx", ctx), ("name", name)]),
            ScriptMutability::Immutable,
        ).ok()?.rows;
        let row = rows.first()?;
        Some(ValueObject {
            name: name.to_string(),
            description: dv_str(&row[0]),
            fields: self.query_fields(&ws, ctx, "value_object", name, "desired"),
            validation_rules: self.query_vo_rules(&ws, ctx, name, "desired"),
            file_path: None,
            start_line: None,
            end_line: None,
        })
    }

    pub fn query_repository(&self, ws: &str, ctx: &str, name: &str) -> Option<Repository> {
        let ws = canonicalize_path(ws);
        let rows = self.db.run_script(
            "?[aggregate] := *repository{workspace: $ws, context: $ctx, name: $name, state: 'desired', aggregate}",
            params_map(&[("ws", &ws), ("ctx", ctx), ("name", name)]),
            ScriptMutability::Immutable,
        ).ok()?.rows;
        let row = rows.first()?;
        Some(Repository {
            name: name.to_string(),
            aggregate: dv_str(&row[0]),
            methods: self.query_methods(&ws, ctx, "repository", name, "desired"),
            file_path: None,
            start_line: None,
            end_line: None,
        })
    }

    pub fn query_aggregate(&self, ws: &str, ctx: &str, name: &str) -> Option<Aggregate> {
        let ws = canonicalize_path(ws);
        let rows = self.db.run_script(
            "?[description, root_entity] := *aggregate{workspace: $ws, context: $ctx, name: $name, state: 'desired', description, root_entity}",
            params_map(&[("ws", &ws), ("ctx", ctx), ("name", name)]),
            ScriptMutability::Immutable,
        ).ok()?.rows;
        let row = rows.first()?;
        let members = self.db.run_script(
            "?[member_kind, member] := *aggregate_member{workspace: $ws, context: $ctx, aggregate: $name, member_kind, member, state: 'desired'}",
            params_map(&[("ws", &ws), ("ctx", ctx), ("name", name)]),
            ScriptMutability::Immutable,
        ).map(|r| r.rows).unwrap_or_default();
        Some(Aggregate {
            name: name.to_string(),
            description: dv_str(&row[0]),
            root_entity: dv_str(&row[1]),
            entities: members.iter().filter(|r| dv_str(&r[0]) == "entity").map(|r| dv_str(&r[1])).collect(),
            value_objects: members.iter().filter(|r| dv_str(&r[0]) == "value_object").map(|r| dv_str(&r[1])).collect(),
            ownership: self.query_ownership(&ws, ctx, "aggregate", name, "desired"),
        })
    }

    pub fn query_policy(&self, ws: &str, ctx: &str, name: &str) -> Option<Policy> {
        let ws = canonicalize_path(ws);
        let rows = self.db.run_script(
            "?[description, kind] := *policy{workspace: $ws, context: $ctx, name: $name, state: 'desired', description, kind}",
            params_map(&[("ws", &ws), ("ctx", ctx), ("name", name)]),
            ScriptMutability::Immutable,
        ).ok()?.rows;
        let row = rows.first()?;
        let links = self.db.run_script(
            "?[idx, link_kind, link] := *policy_link{workspace: $ws, context: $ctx, policy: $name, idx, state: 'desired', link_kind, link}",
            params_map(&[("ws", &ws), ("ctx", ctx), ("name", name)]),
            ScriptMutability::Immutable,
        ).map(|r| r.rows).unwrap_or_default();
        let mut indexed = links.iter().map(|r| (dv_i64(&r[0]), dv_str(&r[1]), dv_str(&r[2]))).collect::<Vec<_>>();
        indexed.sort_by_key(|(idx, _, _)| *idx);
        Some(Policy {
            name: name.to_string(),
            description: dv_str(&row[0]),
            kind: match dv_str(&row[1]).as_str() {
                "process_manager" => PolicyKind::ProcessManager,
                "integration" => PolicyKind::Integration,
                _ => PolicyKind::Domain,
            },
            triggers: indexed.iter().filter(|(_, kind, _)| kind == "trigger").map(|(_, _, link)| link.clone()).collect(),
            commands: indexed.iter().filter(|(_, kind, _)| kind == "command").map(|(_, _, link)| link.clone()).collect(),
            ownership: self.query_ownership(&ws, ctx, "policy", name, "desired"),
        })
    }

    pub fn query_read_model(&self, ws: &str, ctx: &str, name: &str) -> Option<ReadModel> {
        let ws = canonicalize_path(ws);
        let rows = self.db.run_script(
            "?[description, source] := *read_model{workspace: $ws, context: $ctx, name: $name, state: 'desired', description, source}",
            params_map(&[("ws", &ws), ("ctx", ctx), ("name", name)]),
            ScriptMutability::Immutable,
        ).ok()?.rows;
        let row = rows.first()?;
        Some(ReadModel {
            name: name.to_string(),
            description: dv_str(&row[0]),
            source: dv_str(&row[1]),
            fields: self.query_fields(&ws, ctx, "read_model", name, "desired"),
            ownership: self.query_ownership(&ws, ctx, "read_model", name, "desired"),
        })
    }

    pub fn query_external_system(&self, ws: &str, name: &str) -> Option<ExternalSystem> {
        let ws = canonicalize_path(ws);
        let rows = self.db.run_script(
            "?[description, kind, rationale] := *external_system{workspace: $ws, name: $name, state: 'desired', description, kind, rationale}",
            params_map(&[("ws", &ws), ("name", name)]),
            ScriptMutability::Immutable,
        ).ok()?.rows;
        let row = rows.first()?;
        Some(ExternalSystem {
            name: name.to_string(),
            description: dv_str(&row[0]),
            kind: dv_str(&row[1]),
            consumed_by_contexts: self.query_indexed_strings(
                "?[idx, context] := *external_system_context{workspace: $ws, system: $name, idx, state: 'desired', context}",
                params_map(&[("ws", &ws), ("name", name)]),
            ),
            rationale: dv_str(&row[2]),
            ownership: self.query_ownership(&ws, "", "external_system", name, "desired"),
        })
    }

    pub fn query_architectural_decision(&self, ws: &str, id: &str) -> Option<ArchitecturalDecision> {
        let ws = canonicalize_path(ws);
        let rows = self.db.run_script(
            "?[title, status, scope, date, rationale] := *architectural_decision{workspace: $ws, id: $id, state: 'desired', title, status, scope, date, rationale}",
            params_map(&[("ws", &ws), ("id", id)]),
            ScriptMutability::Immutable,
        ).ok()?.rows;
        let row = rows.first()?;
        Some(ArchitecturalDecision {
            id: id.to_string(),
            title: dv_str(&row[0]),
            status: match dv_str(&row[1]).as_str() {
                "accepted" => DecisionStatus::Accepted,
                "superseded" => DecisionStatus::Superseded,
                "deprecated" => DecisionStatus::Deprecated,
                _ => DecisionStatus::Proposed,
            },
            scope: dv_str(&row[2]),
            date: dv_str(&row[3]),
            rationale: dv_str(&row[4]),
            consequences: self.query_indexed_strings(
                "?[idx, text] := *decision_consequence{workspace: $ws, decision_id: $id, idx, state: 'desired', text}",
                params_map(&[("ws", &ws), ("id", id)]),
            ),
            contexts: self.query_indexed_strings(
                "?[idx, context] := *decision_context{workspace: $ws, decision_id: $id, idx, state: 'desired', context}",
                params_map(&[("ws", &ws), ("id", id)]),
            ),
            ownership: self.query_ownership(&ws, "", "architectural_decision", id, "desired"),
        })
    }

    pub fn upsert_context(&self, workspace_path: &str, name: &str, description: &str, module_path: &str, dependencies: &[String], ownership: &Ownership) -> Result<()> {
        let ws = canonicalize_path(workspace_path);
        self.ensure_project(workspace_path)?;
        self.db.run_script(
            "?[workspace, name, state, description, module_path] <- [[$ws, $name, 'desired', $desc, $mp]] :put context { workspace, name, state => description, module_path }",
            params_map(&[("ws", &ws), ("name", name), ("desc", description), ("mp", module_path)]),
            ScriptMutability::Mutable,
        ).map_err(|e| anyhow::anyhow!("upsert_context: {:?}", e))?;
        let _ = self.db.run_script(
            "?[workspace, from_ctx, to_ctx, state] := *context_dep{workspace, from_ctx, to_ctx, state}, workspace = $ws, from_ctx = $name, state = 'desired' :rm context_dep { workspace, from_ctx, to_ctx, state }",
            params_map(&[("ws", &ws), ("name", name)]),
            ScriptMutability::Mutable,
        );
        for dep in dependencies {
            self.db.run_script(
                "?[workspace, from_ctx, to_ctx, state] <- [[$ws, $from, $to, 'desired']] :put context_dep { workspace, from_ctx, to_ctx, state }",
                params_map(&[("ws", &ws), ("from", name), ("to", dep)]),
                ScriptMutability::Mutable,
            ).map_err(|e| anyhow::anyhow!("upsert_context dep: {:?}", e))?;
        }
        self.save_owner_meta(&ws, name, "context", name, ownership, "desired")?;
        Ok(())
    }

    pub fn remove_context(&self, workspace_path: &str, name: &str) -> Result<bool> {
        let ws = canonicalize_path(workspace_path);
        let p = params_map(&[("ws", &ws), ("name", name)]);
        let exists = self.db.run_script(
            "?[n] := *context{workspace: $ws, name: $name, state: 'desired'}, n = $name",
            p.clone(),
            ScriptMutability::Immutable,
        ).map(|r| !r.rows.is_empty()).unwrap_or(false);
        if !exists {
            return Ok(false);
        }
        let _ = self.db.run_script(
            "?[workspace, from_ctx, to_ctx, state] := *context_dep{workspace, from_ctx, to_ctx, state}, workspace = $ws, from_ctx = $name, state = 'desired' :rm context_dep { workspace, from_ctx, to_ctx, state }",
            p.clone(),
            ScriptMutability::Mutable,
        );
        let _ = self.db.run_script(
            "?[workspace, from_ctx, to_ctx, state] := *context_dep{workspace, from_ctx, to_ctx, state}, workspace = $ws, to_ctx = $name, state = 'desired' :rm context_dep { workspace, from_ctx, to_ctx, state }",
            p.clone(),
            ScriptMutability::Mutable,
        );
        self.remove_owner_meta(&ws, name, "context", name);
        self.db.run_script(
            "?[workspace, name, state] <- [[$ws, $name, 'desired']] :rm context { workspace, name, state }",
            p,
            ScriptMutability::Mutable,
        ).map_err(|e| anyhow::anyhow!("remove_context: {:?}", e))?;
        Ok(true)
    }

    pub fn upsert_entity(&self, workspace_path: &str, ctx: &str, entity: &Entity) -> Result<()> {
        let ws = canonicalize_path(workspace_path);
        self.ensure_project(workspace_path)?;
        let mut params = params_map(&[("ws", &ws), ("ctx", ctx), ("name", &entity.name), ("desc", &entity.description)]);
        params.insert("aggregate_root".into(), cozo::DataValue::Bool(entity.aggregate_root));
        self.db.run_script(
            "?[workspace, context, name, state, description, aggregate_root] <- [[$ws, $ctx, $name, 'desired', $desc, $aggregate_root]] :put entity { workspace, context, name, state => description, aggregate_root }",
            params,
            ScriptMutability::Mutable,
        ).map_err(|e| anyhow::anyhow!("upsert_entity: {:?}", e))?;
        self.replace_owner_fields(&ws, ctx, "entity", &entity.name, &entity.fields)?;
        self.replace_owner_methods(&ws, ctx, "entity", &entity.name, &entity.methods)?;
        self.replace_invariants(&ws, ctx, &entity.name, &entity.invariants)?;
        Ok(())
    }

    pub fn remove_entity(&self, workspace_path: &str, ctx: &str, name: &str) -> Result<bool> {
        let ws = canonicalize_path(workspace_path);
        let p = params_map(&[("ws", &ws), ("ctx", ctx), ("name", name)]);
        let exists = self.db.run_script(
            "?[n] := *entity{workspace: $ws, context: $ctx, name: $name, state: 'desired'}, n = $name",
            p.clone(),
            ScriptMutability::Immutable,
        ).map(|r| !r.rows.is_empty()).unwrap_or(false);
        if !exists { return Ok(false); }
        let _ = self.db.run_script("?[workspace, context, owner_kind, owner, name, state] := *field{workspace, context, owner_kind, owner, name, state}, workspace = $ws, context = $ctx, owner_kind = 'entity', owner = $name, state = 'desired' :rm field { workspace, context, owner_kind, owner, name, state }", p.clone(), ScriptMutability::Mutable);
        let _ = self.db.run_script("?[workspace, context, owner_kind, owner, name, state] := *method{workspace, context, owner_kind, owner, name, state}, workspace = $ws, context = $ctx, owner_kind = 'entity', owner = $name, state = 'desired' :rm method { workspace, context, owner_kind, owner, name, state }", p.clone(), ScriptMutability::Mutable);
        let _ = self.db.run_script("?[workspace, context, owner_kind, owner, method, name, state] := *method_param{workspace, context, owner_kind, owner, method, name, state}, workspace = $ws, context = $ctx, owner_kind = 'entity', owner = $name, state = 'desired' :rm method_param { workspace, context, owner_kind, owner, method, name, state }", p.clone(), ScriptMutability::Mutable);
        let _ = self.db.run_script("?[workspace, context, entity, idx, state] := *invariant{workspace, context, entity, idx, state}, workspace = $ws, context = $ctx, entity = $name, state = 'desired' :rm invariant { workspace, context, entity, idx, state }", p.clone(), ScriptMutability::Mutable);
        self.db.run_script("?[workspace, context, name, state] <- [[$ws, $ctx, $name, 'desired']] :rm entity { workspace, context, name, state }", p, ScriptMutability::Mutable).map_err(|e| anyhow::anyhow!("remove_entity: {:?}", e))?;
        Ok(true)
    }

    
    pub fn upsert_api_endpoint(&self, workspace_path: &str, ctx: &str, ep: &APIEndpoint) -> Result<()> {
        let ws = canonicalize_path(workspace_path);
        self.ensure_project(workspace_path)?;
        let params = params_map(&[
            ("ws", &ws), ("ctx", ctx), ("id", &ep.id), ("svc", &ep.service_id),
            ("met", &ep.method), ("path", &ep.route_pattern), ("desc", &ep.description)
        ]);
        self.db.run_script(
            "?[workspace, context, id, state, service_id, method, route_pattern, description] <- \
             [[$ws, $ctx, $id, 'desired', $svc, $met, $path, $desc]] :put api_endpoint { workspace, context, id, state => service_id, method, route_pattern, description }",
            params, ScriptMutability::Mutable
        ).map_err(|e| anyhow::anyhow!("upsert_api_endpoint: {:?}", e))?;
        Ok(())
    }

    pub fn remove_api_endpoint(&self, workspace_path: &str, ctx: &str, id: &str) -> Result<bool> {
        let ws = canonicalize_path(workspace_path);
        let params = params_map(&[("ws", &ws), ("ctx", ctx), ("id", id)]);
        let _ = self.db.run_script(
            "?[workspace, context, id, state] := *api_endpoint{workspace, context, id, state}, workspace = $ws, context = $ctx, id = $id, state = 'desired' :rm api_endpoint { workspace, context, id, state }",
            params, ScriptMutability::Mutable
        ).map_err(|e| anyhow::anyhow!("remove_api_endpoint: {:?}", e))?;
        Ok(true)
    }

    pub fn query_api_endpoint(&self, ws: &str, ctx: &str, id: &str) -> Option<APIEndpoint> {
        let ws = canonicalize_path(ws);
        let rows = self.db.run_script(
            "?[service_id, method, route_pattern, description] := *api_endpoint{workspace: $ws, context: $ctx, id: $id, state: 'desired', service_id, method, route_pattern, description}",
            params_map(&[("ws", &ws), ("ctx", ctx), ("id", id)]),
            ScriptMutability::Immutable
        ).ok()?.rows;
        let row = rows.first()?;
        Some(APIEndpoint {
            id: id.to_string(),
            service_id: dv_str(&row[0]),
            method: dv_str(&row[1]),
            route_pattern: dv_str(&row[2]),
            description: dv_str(&row[3]),
        })
    }

    pub fn upsert_service(&self, workspace_path: &str, ctx: &str, service: &Service) -> Result<()> {
        let ws = canonicalize_path(workspace_path);
        self.ensure_project(workspace_path)?;
        let kind = match service.kind {
            ServiceKind::Application => "application",
            ServiceKind::Infrastructure => "infrastructure",
            ServiceKind::Domain => "domain",
        };
        self.db.run_script(
            "?[workspace, context, name, state, description, kind] <- [[$ws, $ctx, $name, 'desired', $desc, $kind]] :put service { workspace, context, name, state => description, kind }",
            params_map(&[("ws", &ws), ("ctx", ctx), ("name", &service.name), ("desc", &service.description), ("kind", kind)]),
            ScriptMutability::Mutable,
        ).map_err(|e| anyhow::anyhow!("upsert_service: {:?}", e))?;
        self.replace_owner_methods(&ws, ctx, "service", &service.name, &service.methods)?;
        self.replace_service_deps(&ws, ctx, &service.name, &service.dependencies)?;
        Ok(())
    }

    pub fn remove_service(&self, workspace_path: &str, ctx: &str, name: &str) -> Result<bool> {
        let ws = canonicalize_path(workspace_path);
        let p = params_map(&[("ws", &ws), ("ctx", ctx), ("name", name)]);
        let exists = self.db.run_script("?[n] := *service{workspace: $ws, context: $ctx, name: $name, state: 'desired'}, n = $name", p.clone(), ScriptMutability::Immutable).map(|r| !r.rows.is_empty()).unwrap_or(false);
        if !exists { return Ok(false); }
        let _ = self.db.run_script("?[workspace, context, owner_kind, owner, name, state] := *method{workspace, context, owner_kind, owner, name, state}, workspace = $ws, context = $ctx, owner_kind = 'service', owner = $name, state = 'desired' :rm method { workspace, context, owner_kind, owner, name, state }", p.clone(), ScriptMutability::Mutable);
        let _ = self.db.run_script("?[workspace, context, owner_kind, owner, method, name, state] := *method_param{workspace, context, owner_kind, owner, method, name, state}, workspace = $ws, context = $ctx, owner_kind = 'service', owner = $name, state = 'desired' :rm method_param { workspace, context, owner_kind, owner, method, name, state }", p.clone(), ScriptMutability::Mutable);
        let _ = self.db.run_script("?[workspace, context, service, dep, state] := *service_dep{workspace, context, service, dep, state}, workspace = $ws, context = $ctx, service = $name, state = 'desired' :rm service_dep { workspace, context, service, dep, state }", p.clone(), ScriptMutability::Mutable);
        self.db.run_script("?[workspace, context, name, state] <- [[$ws, $ctx, $name, 'desired']] :rm service { workspace, context, name, state }", p, ScriptMutability::Mutable).map_err(|e| anyhow::anyhow!("remove_service: {:?}", e))?;
        Ok(true)
    }

    pub fn upsert_event(&self, workspace_path: &str, ctx: &str, event: &DomainEvent) -> Result<()> {
        let ws = canonicalize_path(workspace_path);
        self.ensure_project(workspace_path)?;
        self.db.run_script(
            "?[workspace, context, name, state, description, source] <- [[$ws, $ctx, $name, 'desired', $desc, $source]] :put event { workspace, context, name, state => description, source }",
            params_map(&[("ws", &ws), ("ctx", ctx), ("name", &event.name), ("desc", &event.description), ("source", &event.source)]),
            ScriptMutability::Mutable,
        ).map_err(|e| anyhow::anyhow!("upsert_event: {:?}", e))?;
        self.replace_owner_fields(&ws, ctx, "event", &event.name, &event.fields)?;
        Ok(())
    }

    pub fn remove_event(&self, workspace_path: &str, ctx: &str, name: &str) -> Result<bool> {
        let ws = canonicalize_path(workspace_path);
        let p = params_map(&[("ws", &ws), ("ctx", ctx), ("name", name)]);
        let exists = self.db.run_script("?[n] := *event{workspace: $ws, context: $ctx, name: $name, state: 'desired'}, n = $name", p.clone(), ScriptMutability::Immutable).map(|r| !r.rows.is_empty()).unwrap_or(false);
        if !exists { return Ok(false); }
        let _ = self.db.run_script("?[workspace, context, owner_kind, owner, name, state] := *field{workspace, context, owner_kind, owner, name, state}, workspace = $ws, context = $ctx, owner_kind = 'event', owner = $name, state = 'desired' :rm field { workspace, context, owner_kind, owner, name, state }", p.clone(), ScriptMutability::Mutable);
        self.db.run_script("?[workspace, context, name, state] <- [[$ws, $ctx, $name, 'desired']] :rm event { workspace, context, name, state }", p, ScriptMutability::Mutable).map_err(|e| anyhow::anyhow!("remove_event: {:?}", e))?;
        Ok(true)
    }

    pub fn upsert_value_object(&self, workspace_path: &str, ctx: &str, value_object: &ValueObject) -> Result<()> {
        let ws = canonicalize_path(workspace_path);
        self.ensure_project(workspace_path)?;
        self.db.run_script(
            "?[workspace, context, name, state, description] <- [[$ws, $ctx, $name, 'desired', $desc]] :put value_object { workspace, context, name, state => description }",
            params_map(&[("ws", &ws), ("ctx", ctx), ("name", &value_object.name), ("desc", &value_object.description)]),
            ScriptMutability::Mutable,
        ).map_err(|e| anyhow::anyhow!("upsert_value_object: {:?}", e))?;
        self.replace_owner_fields(&ws, ctx, "value_object", &value_object.name, &value_object.fields)?;
        self.replace_vo_rules(&ws, ctx, &value_object.name, &value_object.validation_rules)?;
        Ok(())
    }

    pub fn remove_value_object(&self, workspace_path: &str, ctx: &str, name: &str) -> Result<bool> {
        let ws = canonicalize_path(workspace_path);
        let p = params_map(&[("ws", &ws), ("ctx", ctx), ("name", name)]);
        let exists = self.db.run_script("?[n] := *value_object{workspace: $ws, context: $ctx, name: $name, state: 'desired'}, n = $name", p.clone(), ScriptMutability::Immutable).map(|r| !r.rows.is_empty()).unwrap_or(false);
        if !exists { return Ok(false); }
        let _ = self.db.run_script("?[workspace, context, owner_kind, owner, name, state] := *field{workspace, context, owner_kind, owner, name, state}, workspace = $ws, context = $ctx, owner_kind = 'value_object', owner = $name, state = 'desired' :rm field { workspace, context, owner_kind, owner, name, state }", p.clone(), ScriptMutability::Mutable);
        let _ = self.db.run_script("?[workspace, context, value_object, idx, state] := *vo_rule{workspace, context, value_object, idx, state}, workspace = $ws, context = $ctx, value_object = $name, state = 'desired' :rm vo_rule { workspace, context, value_object, idx, state }", p.clone(), ScriptMutability::Mutable);
        self.db.run_script("?[workspace, context, name, state] <- [[$ws, $ctx, $name, 'desired']] :rm value_object { workspace, context, name, state }", p, ScriptMutability::Mutable).map_err(|e| anyhow::anyhow!("remove_value_object: {:?}", e))?;
        Ok(true)
    }

    pub fn upsert_repository(&self, workspace_path: &str, ctx: &str, repository: &Repository) -> Result<()> {
        let ws = canonicalize_path(workspace_path);
        self.ensure_project(workspace_path)?;
        self.db.run_script(
            "?[workspace, context, name, state, aggregate] <- [[$ws, $ctx, $name, 'desired', $aggregate]] :put repository { workspace, context, name, state => aggregate }",
            params_map(&[("ws", &ws), ("ctx", ctx), ("name", &repository.name), ("aggregate", &repository.aggregate)]),
            ScriptMutability::Mutable,
        ).map_err(|e| anyhow::anyhow!("upsert_repository: {:?}", e))?;
        self.replace_owner_methods(&ws, ctx, "repository", &repository.name, &repository.methods)?;
        Ok(())
    }

    pub fn remove_repository(&self, workspace_path: &str, ctx: &str, name: &str) -> Result<bool> {
        let ws = canonicalize_path(workspace_path);
        let p = params_map(&[("ws", &ws), ("ctx", ctx), ("name", name)]);
        let exists = self.db.run_script("?[n] := *repository{workspace: $ws, context: $ctx, name: $name, state: 'desired'}, n = $name", p.clone(), ScriptMutability::Immutable).map(|r| !r.rows.is_empty()).unwrap_or(false);
        if !exists { return Ok(false); }
        let _ = self.db.run_script("?[workspace, context, owner_kind, owner, name, state] := *method{workspace, context, owner_kind, owner, name, state}, workspace = $ws, context = $ctx, owner_kind = 'repository', owner = $name, state = 'desired' :rm method { workspace, context, owner_kind, owner, name, state }", p.clone(), ScriptMutability::Mutable);
        let _ = self.db.run_script("?[workspace, context, owner_kind, owner, method, name, state] := *method_param{workspace, context, owner_kind, owner, method, name, state}, workspace = $ws, context = $ctx, owner_kind = 'repository', owner = $name, state = 'desired' :rm method_param { workspace, context, owner_kind, owner, method, name, state }", p.clone(), ScriptMutability::Mutable);
        self.db.run_script("?[workspace, context, name, state] <- [[$ws, $ctx, $name, 'desired']] :rm repository { workspace, context, name, state }", p, ScriptMutability::Mutable).map_err(|e| anyhow::anyhow!("remove_repository: {:?}", e))?;
        Ok(true)
    }

    pub fn query_module(&self, ws: &str, ctx: &str, name: &str) -> Option<Module> {
        let ws = canonicalize_path(ws);
        let rows = self.db.run_script(
            "?[path, public, file_path, description] := *module{workspace: $ws, context: $ctx, name: $name, state: 'desired', path, public, file_path, description}",
            params_map(&[("ws", &ws), ("ctx", ctx), ("name", name)]),
            ScriptMutability::Immutable,
        ).ok()?.rows;
        let row = rows.first()?;
        Some(Module {
            name: name.to_string(),
            path: dv_str(&row[0]),
            public: matches!(&row[1], cozo::DataValue::Bool(true)),
            file_path: dv_str(&row[2]),
            description: dv_str(&row[3]),
        })
    }

    pub fn upsert_module(&self, workspace_path: &str, ctx: &str, module: &Module) -> Result<()> {
        let ws = canonicalize_path(workspace_path);
        self.ensure_project(workspace_path)?;
        let mut params = params_map(&[("ws", &ws), ("ctx", ctx), ("name", &module.name), ("path", &module.path), ("fp", &module.file_path), ("desc", &module.description)]);
        params.insert("public".into(), cozo::DataValue::Bool(module.public));
        self.db.run_script(
            "?[workspace, context, name, state, path, public, file_path, description] <- [[$ws, $ctx, $name, 'desired', $path, $public, $fp, $desc]] :put module { workspace, context, name, state => path, public, file_path, description }",
            params,
            ScriptMutability::Mutable,
        ).map_err(|e| anyhow::anyhow!("upsert_module: {:?}", e))?;
        Ok(())
    }

    pub fn remove_module(&self, workspace_path: &str, ctx: &str, name: &str) -> Result<bool> {
        let ws = canonicalize_path(workspace_path);
        let p = params_map(&[("ws", &ws), ("ctx", ctx), ("name", name)]);
        let exists = self.db.run_script(
            "?[n] := *module{workspace: $ws, context: $ctx, name: $name, state: 'desired'}, n = $name",
            p.clone(),
            ScriptMutability::Immutable,
        ).map(|r| !r.rows.is_empty()).unwrap_or(false);
        if !exists { return Ok(false); }
        self.db.run_script(
            "?[workspace, context, name, state] <- [[$ws, $ctx, $name, 'desired']] :rm module { workspace, context, name, state }",
            p,
            ScriptMutability::Mutable,
        ).map_err(|e| anyhow::anyhow!("remove_module: {:?}", e))?;
        Ok(true)
    }

    pub fn upsert_aggregate(&self, workspace_path: &str, ctx: &str, aggregate: &Aggregate) -> Result<()> {
        let ws = canonicalize_path(workspace_path);
        self.db.run_script(
            "?[workspace, context, name, state, description, root_entity] <- [[$ws, $ctx, $name, 'desired', $desc, $root]] :put aggregate { workspace, context, name, state => description, root_entity }",
            params_map(&[("ws", &ws), ("ctx", ctx), ("name", &aggregate.name), ("desc", &aggregate.description), ("root", &aggregate.root_entity)]),
            ScriptMutability::Mutable,
        ).map_err(|e| anyhow::anyhow!("upsert_aggregate: {:?}", e))?;
        self.save_owner_meta(&ws, ctx, "aggregate", &aggregate.name, &aggregate.ownership, "desired")?;
        let _ = self.db.run_script(
            "?[workspace, context, aggregate, member_kind, member, state] := *aggregate_member{workspace, context, aggregate, member_kind, member, state}, workspace = $ws, context = $ctx, aggregate = $name, state = 'desired' :rm aggregate_member { workspace, context, aggregate, member_kind, member, state }",
            params_map(&[("ws", &ws), ("ctx", ctx), ("name", &aggregate.name)]),
            ScriptMutability::Mutable,
        );
        for entity in &aggregate.entities { self.db.run_script("?[workspace, context, aggregate, member_kind, member, state] <- [[$ws, $ctx, $name, 'entity', $member, 'desired']] :put aggregate_member { workspace, context, aggregate, member_kind, member, state }", params_map(&[("ws", &ws), ("ctx", ctx), ("name", &aggregate.name), ("member", entity)]), ScriptMutability::Mutable).map_err(|e| anyhow::anyhow!("upsert_aggregate entity: {:?}", e))?; }
        for vo in &aggregate.value_objects { self.db.run_script("?[workspace, context, aggregate, member_kind, member, state] <- [[$ws, $ctx, $name, 'value_object', $member, 'desired']] :put aggregate_member { workspace, context, aggregate, member_kind, member, state }", params_map(&[("ws", &ws), ("ctx", ctx), ("name", &aggregate.name), ("member", vo)]), ScriptMutability::Mutable).map_err(|e| anyhow::anyhow!("upsert_aggregate vo: {:?}", e))?; }
        Ok(())
    }

    pub fn remove_aggregate(&self, workspace_path: &str, ctx: &str, name: &str) -> Result<bool> { let ws = canonicalize_path(workspace_path); let p = params_map(&[("ws", &ws), ("ctx", ctx), ("name", name)]); let exists = self.db.run_script("?[n] := *aggregate{workspace: $ws, context: $ctx, name: $name, state: 'desired'}, n = $name", p.clone(), ScriptMutability::Immutable).map(|r| !r.rows.is_empty()).unwrap_or(false); if !exists { return Ok(false); } let _ = self.db.run_script("?[workspace, context, aggregate, member_kind, member, state] := *aggregate_member{workspace, context, aggregate, member_kind, member, state}, workspace = $ws, context = $ctx, aggregate = $name, state = 'desired' :rm aggregate_member { workspace, context, aggregate, member_kind, member, state }", p.clone(), ScriptMutability::Mutable); self.remove_owner_meta(&ws, ctx, "aggregate", name); self.db.run_script("?[workspace, context, name, state] <- [[$ws, $ctx, $name, 'desired']] :rm aggregate { workspace, context, name, state }", p, ScriptMutability::Mutable).map_err(|e| anyhow::anyhow!("remove_aggregate: {:?}", e))?; Ok(true) }

    pub fn upsert_policy(&self, workspace_path: &str, ctx: &str, policy: &Policy) -> Result<()> { let ws = canonicalize_path(workspace_path); let kind = Self::policy_kind_key(&policy.kind).to_string(); self.db.run_script("?[workspace, context, name, state, description, kind] <- [[$ws, $ctx, $name, 'desired', $desc, $kind]] :put policy { workspace, context, name, state => description, kind }", params_map(&[("ws", &ws), ("ctx", ctx), ("name", &policy.name), ("desc", &policy.description), ("kind", &kind)]), ScriptMutability::Mutable).map_err(|e| anyhow::anyhow!("upsert_policy: {:?}", e))?; self.save_owner_meta(&ws, ctx, "policy", &policy.name, &policy.ownership, "desired")?; let _ = self.db.run_script("?[workspace, context, policy, link_kind, link, idx, state] := *policy_link{workspace, context, policy, link_kind, link, idx, state}, workspace = $ws, context = $ctx, policy = $name, state = 'desired' :rm policy_link { workspace, context, policy, link_kind, link, idx, state }", params_map(&[("ws", &ws), ("ctx", ctx), ("name", &policy.name)]), ScriptMutability::Mutable); for (idx, trigger) in policy.triggers.iter().enumerate() { let mut p = params_map(&[("ws", &ws), ("ctx", ctx), ("name", &policy.name), ("link", trigger)]); p.insert("idx".into(), int_dv(idx as i64)); self.db.run_script("?[workspace, context, policy, link_kind, link, idx, state] <- [[$ws, $ctx, $name, 'trigger', $link, $idx, 'desired']] :put policy_link { workspace, context, policy, link_kind, link, idx, state }", p, ScriptMutability::Mutable).map_err(|e| anyhow::anyhow!("upsert_policy trigger: {:?}", e))?; } for (idx, command) in policy.commands.iter().enumerate() { let mut p = params_map(&[("ws", &ws), ("ctx", ctx), ("name", &policy.name), ("link", command)]); p.insert("idx".into(), int_dv(idx as i64)); self.db.run_script("?[workspace, context, policy, link_kind, link, idx, state] <- [[$ws, $ctx, $name, 'command', $link, $idx, 'desired']] :put policy_link { workspace, context, policy, link_kind, link, idx, state }", p, ScriptMutability::Mutable).map_err(|e| anyhow::anyhow!("upsert_policy command: {:?}", e))?; } Ok(()) }

    pub fn remove_policy(&self, workspace_path: &str, ctx: &str, name: &str) -> Result<bool> { let ws = canonicalize_path(workspace_path); let p = params_map(&[("ws", &ws), ("ctx", ctx), ("name", name)]); let exists = self.db.run_script("?[n] := *policy{workspace: $ws, context: $ctx, name: $name, state: 'desired'}, n = $name", p.clone(), ScriptMutability::Immutable).map(|r| !r.rows.is_empty()).unwrap_or(false); if !exists { return Ok(false); } let _ = self.db.run_script("?[workspace, context, policy, link_kind, link, idx, state] := *policy_link{workspace, context, policy, link_kind, link, idx, state}, workspace = $ws, context = $ctx, policy = $name, state = 'desired' :rm policy_link { workspace, context, policy, link_kind, link, idx, state }", p.clone(), ScriptMutability::Mutable); self.remove_owner_meta(&ws, ctx, "policy", name); self.db.run_script("?[workspace, context, name, state] <- [[$ws, $ctx, $name, 'desired']] :rm policy { workspace, context, name, state }", p, ScriptMutability::Mutable).map_err(|e| anyhow::anyhow!("remove_policy: {:?}", e))?; Ok(true) }

    pub fn upsert_read_model(&self, workspace_path: &str, ctx: &str, read_model: &ReadModel) -> Result<()> { let ws = canonicalize_path(workspace_path); self.db.run_script("?[workspace, context, name, state, description, source] <- [[$ws, $ctx, $name, 'desired', $desc, $src]] :put read_model { workspace, context, name, state => description, source }", params_map(&[("ws", &ws), ("ctx", ctx), ("name", &read_model.name), ("desc", &read_model.description), ("src", &read_model.source)]), ScriptMutability::Mutable).map_err(|e| anyhow::anyhow!("upsert_read_model: {:?}", e))?; self.save_owner_meta(&ws, ctx, "read_model", &read_model.name, &read_model.ownership, "desired")?; self.replace_owner_fields(&ws, ctx, "read_model", &read_model.name, &read_model.fields)?; Ok(()) }

    pub fn remove_read_model(&self, workspace_path: &str, ctx: &str, name: &str) -> Result<bool> { let ws = canonicalize_path(workspace_path); let p = params_map(&[("ws", &ws), ("ctx", ctx), ("name", name)]); let exists = self.db.run_script("?[n] := *read_model{workspace: $ws, context: $ctx, name: $name, state: 'desired'}, n = $name", p.clone(), ScriptMutability::Immutable).map(|r| !r.rows.is_empty()).unwrap_or(false); if !exists { return Ok(false); } self.remove_owner_meta(&ws, ctx, "read_model", name); let _ = self.db.run_script("?[workspace, context, owner_kind, owner, name, state] := *field{workspace, context, owner_kind, owner, name, state}, workspace = $ws, context = $ctx, owner_kind = 'read_model', owner = $name, state = 'desired' :rm field { workspace, context, owner_kind, owner, name, state }", p.clone(), ScriptMutability::Mutable); self.db.run_script("?[workspace, context, name, state] <- [[$ws, $ctx, $name, 'desired']] :rm read_model { workspace, context, name, state }", p, ScriptMutability::Mutable).map_err(|e| anyhow::anyhow!("remove_read_model: {:?}", e))?; Ok(true) }

    pub fn upsert_external_system(&self, workspace_path: &str, system: &ExternalSystem) -> Result<()> { let ws = canonicalize_path(workspace_path); self.db.run_script("?[workspace, name, state, description, kind, rationale] <- [[$ws, $name, 'desired', $desc, $kind, $rationale]] :put external_system { workspace, name, state => description, kind, rationale }", params_map(&[("ws", &ws), ("name", &system.name), ("desc", &system.description), ("kind", &system.kind), ("rationale", &system.rationale)]), ScriptMutability::Mutable).map_err(|e| anyhow::anyhow!("upsert_external_system: {:?}", e))?; self.save_owner_meta(&ws, "", "external_system", &system.name, &system.ownership, "desired")?; let _ = self.db.run_script("?[workspace, system, context, idx, state] := *external_system_context{workspace, system, context, idx, state}, workspace = $ws, system = $name, state = 'desired' :rm external_system_context { workspace, system, context, idx, state }", params_map(&[("ws", &ws), ("name", &system.name)]), ScriptMutability::Mutable); for (idx, ctx) in system.consumed_by_contexts.iter().enumerate() { let mut p = params_map(&[("ws", &ws), ("name", &system.name), ("ctx", ctx)]); p.insert("idx".into(), int_dv(idx as i64)); self.db.run_script("?[workspace, system, context, idx, state] <- [[$ws, $name, $ctx, $idx, 'desired']] :put external_system_context { workspace, system, context, idx, state }", p, ScriptMutability::Mutable).map_err(|e| anyhow::anyhow!("upsert_external_system ctx: {:?}", e))?; } Ok(()) }

    pub fn remove_external_system(&self, workspace_path: &str, name: &str) -> Result<bool> { let ws = canonicalize_path(workspace_path); let p = params_map(&[("ws", &ws), ("name", name)]); let exists = self.db.run_script("?[n] := *external_system{workspace: $ws, name: $name, state: 'desired'}, n = $name", p.clone(), ScriptMutability::Immutable).map(|r| !r.rows.is_empty()).unwrap_or(false); if !exists { return Ok(false); } self.remove_owner_meta(&ws, "", "external_system", name); let _ = self.db.run_script("?[workspace, system, context, idx, state] := *external_system_context{workspace, system, context, idx, state}, workspace = $ws, system = $name, state = 'desired' :rm external_system_context { workspace, system, context, idx, state }", p.clone(), ScriptMutability::Mutable); self.db.run_script("?[workspace, name, state] <- [[$ws, $name, 'desired']] :rm external_system { workspace, name, state }", p, ScriptMutability::Mutable).map_err(|e| anyhow::anyhow!("remove_external_system: {:?}", e))?; Ok(true) }

    pub fn upsert_architectural_decision(&self, workspace_path: &str, decision: &ArchitecturalDecision) -> Result<()> { let ws = canonicalize_path(workspace_path); let status = format!("{:?}", decision.status).to_lowercase(); self.db.run_script("?[workspace, id, state, title, status, scope, date, rationale] <- [[$ws, $id, 'desired', $title, $status, $scope, $date, $rationale]] :put architectural_decision { workspace, id, state => title, status, scope, date, rationale }", params_map(&[("ws", &ws), ("id", &decision.id), ("title", &decision.title), ("status", &status), ("scope", &decision.scope), ("date", &decision.date), ("rationale", &decision.rationale)]), ScriptMutability::Mutable).map_err(|e| anyhow::anyhow!("upsert_architectural_decision: {:?}", e))?; self.save_owner_meta(&ws, "", "architectural_decision", &decision.id, &decision.ownership, "desired")?; let _ = self.db.run_script("?[workspace, decision_id, context, idx, state] := *decision_context{workspace, decision_id, context, idx, state}, workspace = $ws, decision_id = $id, state = 'desired' :rm decision_context { workspace, decision_id, context, idx, state }", params_map(&[("ws", &ws), ("id", &decision.id)]), ScriptMutability::Mutable); let _ = self.db.run_script("?[workspace, decision_id, idx, state] := *decision_consequence{workspace, decision_id, idx, state}, workspace = $ws, decision_id = $id, state = 'desired' :rm decision_consequence { workspace, decision_id, idx, state }", params_map(&[("ws", &ws), ("id", &decision.id)]), ScriptMutability::Mutable); for (idx, ctx) in decision.contexts.iter().enumerate() { let mut p = params_map(&[("ws", &ws), ("id", &decision.id), ("ctx", ctx)]); p.insert("idx".into(), int_dv(idx as i64)); self.db.run_script("?[workspace, decision_id, context, idx, state] <- [[$ws, $id, $ctx, $idx, 'desired']] :put decision_context { workspace, decision_id, context, idx, state }", p, ScriptMutability::Mutable).map_err(|e| anyhow::anyhow!("upsert_architectural_decision ctx: {:?}", e))?; } for (idx, consequence) in decision.consequences.iter().enumerate() { let mut p = params_map(&[("ws", &ws), ("id", &decision.id), ("text", consequence)]); p.insert("idx".into(), int_dv(idx as i64)); self.db.run_script("?[workspace, decision_id, idx, state, text] <- [[$ws, $id, $idx, 'desired', $text]] :put decision_consequence { workspace, decision_id, idx, state => text }", p, ScriptMutability::Mutable).map_err(|e| anyhow::anyhow!("upsert_architectural_decision consequence: {:?}", e))?; } Ok(()) }

    pub fn remove_architectural_decision(&self, workspace_path: &str, id: &str) -> Result<bool> { let ws = canonicalize_path(workspace_path); let p = params_map(&[("ws", &ws), ("id", id)]); let exists = self.db.run_script("?[n] := *architectural_decision{workspace: $ws, id: $id, state: 'desired'}, n = $id", p.clone(), ScriptMutability::Immutable).map(|r| !r.rows.is_empty()).unwrap_or(false); if !exists { return Ok(false); } self.remove_owner_meta(&ws, "", "architectural_decision", id); let _ = self.db.run_script("?[workspace, decision_id, context, idx, state] := *decision_context{workspace, decision_id, context, idx, state}, workspace = $ws, decision_id = $id, state = 'desired' :rm decision_context { workspace, decision_id, context, idx, state }", p.clone(), ScriptMutability::Mutable); let _ = self.db.run_script("?[workspace, decision_id, idx, state] := *decision_consequence{workspace, decision_id, idx, state}, workspace = $ws, decision_id = $id, state = 'desired' :rm decision_consequence { workspace, decision_id, idx, state }", p.clone(), ScriptMutability::Mutable); self.db.run_script("?[workspace, id, state] <- [[$ws, $id, 'desired']] :rm architectural_decision { workspace, id, state }", p, ScriptMutability::Mutable).map_err(|e| anyhow::anyhow!("remove_architectural_decision: {:?}", e))?; Ok(true) }

    // ── Project Operations ─────────────────────────────────────────────────

    /// List all stored projects.
    pub fn list(&self) -> Result<Vec<ProjectInfo>> {
        let result = self
            .db
            .run_script(
                "?[workspace, name, updated_at] := *project{workspace, name, updated_at}",
                Default::default(),
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("Failed to list projects: {:?}", e))?;

        let mut projects: Vec<ProjectInfo> = result
            .rows
            .iter()
            .map(|r| ProjectInfo {
                workspace_path: dv_str(&r[0]),
                project_name: dv_str(&r[1]),
                updated_at: dv_str(&r[2]),
            })
            .collect();
        projects.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(projects)
    }

    /// Export a domain model to a JSON file.
    /// `state` can be `"desired"`, `"actual"`, or `"both"`.
    pub fn export_to_file(&self, workspace_path: &str, file_path: &str, state: &str) -> Result<()> {
        let json = match state {
            "actual" => {
                let model = self
                    .load_actual(workspace_path)?
                    .with_context(|| format!("No actual model found for workspace: {workspace_path}"))?;
                serde_json::to_string_pretty(&model)?
            }
            "both" => {
                let desired = self.load_desired(workspace_path)?;
                let actual = self.load_actual(workspace_path)?;
                serde_json::to_string_pretty(&serde_json::json!({
                    "desired": desired,
                    "actual": actual
                }))?
            }
            _ => {
                let model = self
                    .load_desired(workspace_path)?
                    .with_context(|| format!("No desired model found for workspace: {workspace_path}"))?;
                serde_json::to_string_pretty(&model)?
            }
        };
        std::fs::write(file_path, json)
            .with_context(|| format!("Failed to write file: {file_path}"))?;
        Ok(())
    }

    // ── Pure Datalog Differencing ──────────────────────────────────────────

    /// Compute the diff between desired and actual as a single Datalog union query.
    /// Returns `{pending_changes: [{kind, action, context?, name, owner_kind?, owner?}]}`.
    /// Covers ALL relation types: context, entity, service, event, value_object,
    /// repository, field, method, and invariant.
    pub fn diff_graph(&self, workspace_path: &str) -> Result<serde_json::Value> {
        let ws = canonicalize_path(workspace_path);
        let params = params_map(&[("ws", &ws)]);

        let rules: Vec<&str> = vec![
            // ── Context ──
            "?[kind, action, ctx, name, owner_kind, owner] := *context{workspace: $ws, name, state: 'desired'}, not *context{workspace: $ws, name, state: 'actual'}, kind = 'context', action = 'add', ctx = '', owner_kind = '', owner = ''",
            "?[kind, action, ctx, name, owner_kind, owner] := *context{workspace: $ws, name, state: 'actual'}, not *context{workspace: $ws, name, state: 'desired'}, kind = 'context', action = 'remove', ctx = '', owner_kind = '', owner = ''",
            // ── Entity ──
            "?[kind, action, ctx, name, owner_kind, owner] := *entity{workspace: $ws, context: ctx, name, state: 'desired'}, not *entity{workspace: $ws, context: ctx, name, state: 'actual'}, kind = 'entity', action = 'add', owner_kind = '', owner = ''",
            "?[kind, action, ctx, name, owner_kind, owner] := *entity{workspace: $ws, context: ctx, name, state: 'actual'}, not *entity{workspace: $ws, context: ctx, name, state: 'desired'}, kind = 'entity', action = 'remove', owner_kind = '', owner = ''",
            // ── Service ──
            "?[kind, action, ctx, name, owner_kind, owner] := *service{workspace: $ws, context: ctx, name, state: 'desired'}, not *service{workspace: $ws, context: ctx, name, state: 'actual'}, kind = 'service', action = 'add', owner_kind = '', owner = ''",
            "?[kind, action, ctx, name, owner_kind, owner] := *service{workspace: $ws, context: ctx, name, state: 'actual'}, not *service{workspace: $ws, context: ctx, name, state: 'desired'}, kind = 'service', action = 'remove', owner_kind = '', owner = ''",
            // ── Event ──
            "?[kind, action, ctx, name, owner_kind, owner] := *event{workspace: $ws, context: ctx, name, state: 'desired'}, not *event{workspace: $ws, context: ctx, name, state: 'actual'}, kind = 'event', action = 'add', owner_kind = '', owner = ''",
            "?[kind, action, ctx, name, owner_kind, owner] := *event{workspace: $ws, context: ctx, name, state: 'actual'}, not *event{workspace: $ws, context: ctx, name, state: 'desired'}, kind = 'event', action = 'remove', owner_kind = '', owner = ''",
            // ── Value Object ──
            "?[kind, action, ctx, name, owner_kind, owner] := *value_object{workspace: $ws, context: ctx, name, state: 'desired'}, not *value_object{workspace: $ws, context: ctx, name, state: 'actual'}, kind = 'value_object', action = 'add', owner_kind = '', owner = ''",
            "?[kind, action, ctx, name, owner_kind, owner] := *value_object{workspace: $ws, context: ctx, name, state: 'actual'}, not *value_object{workspace: $ws, context: ctx, name, state: 'desired'}, kind = 'value_object', action = 'remove', owner_kind = '', owner = ''",
            // ── Repository ──
            "?[kind, action, ctx, name, owner_kind, owner] := *repository{workspace: $ws, context: ctx, name, state: 'desired'}, not *repository{workspace: $ws, context: ctx, name, state: 'actual'}, kind = 'repository', action = 'add', owner_kind = '', owner = ''",
            "?[kind, action, ctx, name, owner_kind, owner] := *repository{workspace: $ws, context: ctx, name, state: 'actual'}, not *repository{workspace: $ws, context: ctx, name, state: 'desired'}, kind = 'repository', action = 'remove', owner_kind = '', owner = ''",
            // ── Field (cross-entity: entity, event, value_object) ──
            "?[kind, action, ctx, name, owner_kind, owner] := *field{workspace: $ws, context: ctx, owner_kind, owner, name, state: 'desired'}, not *field{workspace: $ws, context: ctx, owner_kind, owner, name, state: 'actual'}, kind = 'field', action = 'add'",
            "?[kind, action, ctx, name, owner_kind, owner] := *field{workspace: $ws, context: ctx, owner_kind, owner, name, state: 'actual'}, not *field{workspace: $ws, context: ctx, owner_kind, owner, name, state: 'desired'}, kind = 'field', action = 'remove'",
            // ── Method (cross-entity: entity, service, repository) ──
            "?[kind, action, ctx, name, owner_kind, owner] := *method{workspace: $ws, context: ctx, owner_kind, owner, name, state: 'desired'}, not *method{workspace: $ws, context: ctx, owner_kind, owner, name, state: 'actual'}, kind = 'method', action = 'add'",
            "?[kind, action, ctx, name, owner_kind, owner] := *method{workspace: $ws, context: ctx, owner_kind, owner, name, state: 'actual'}, not *method{workspace: $ws, context: ctx, owner_kind, owner, name, state: 'desired'}, kind = 'method', action = 'remove'",
            // ── Invariant (diff by text content, not position) ──
            "?[kind, action, ctx, name, owner_kind, owner] := *invariant{workspace: $ws, context: ctx, entity: owner, text: name, state: 'desired'}, not *invariant{workspace: $ws, context: ctx, entity: owner, text: name, state: 'actual'}, kind = 'invariant', action = 'add', owner_kind = 'entity'",
            "?[kind, action, ctx, name, owner_kind, owner] := *invariant{workspace: $ws, context: ctx, entity: owner, text: name, state: 'actual'}, not *invariant{workspace: $ws, context: ctx, entity: owner, text: name, state: 'desired'}, kind = 'invariant', action = 'remove', owner_kind = 'entity'",
        ];

        let script = rules.join(" ");
        let result = self
            .db
            .run_script(&script, params, ScriptMutability::Immutable)
            .map_err(|e| anyhow::anyhow!("diff_graph query: {:?}", e))?;

        let changes: Vec<serde_json::Value> = result
            .rows
            .iter()
            .map(|r| {
                let ctx = dv_str(&r[2]);
                let ok = dv_str(&r[4]);
                let ow = dv_str(&r[5]);
                let mut entry = json!({
                    "kind": dv_str(&r[0]),
                    "action": dv_str(&r[1]),
                    "name": dv_str(&r[3]),
                });
                if !ctx.is_empty() {
                    entry["context"] = json!(ctx);
                }
                if !ok.is_empty() {
                    entry["owner_kind"] = json!(ok);
                    entry["owner"] = json!(ow);
                }
                entry
            })
            .collect();

        Ok(json!({ "pending_changes": changes }))
    }

    // ── Live AST Bridge ───────────────────────────────────────────────────

    /// Project live AST imports into the ephemeral `live_import` table,
    /// then cross-reference against the domain model to detect violations.
    pub fn check_live_dependencies(
        &self,
        workspace_path: &str,
        live_deps: &[crate::domain::analyze::LiveDependency],
    ) -> Result<Vec<crate::domain::analyze::LiveDependency>> {
        let ws = canonicalize_path(workspace_path);

        // 1. Clear previous live_import rows
        let clear_params = params_map(&[("ws", &ws)]);
        let _ = self.db.run_script(
            "?[workspace, from_file, to_module] := *live_import{workspace: $ws, from_file, to_module} \
             :rm live_import { workspace, from_file, to_module }",
            clear_params,
            ScriptMutability::Mutable,
        );

        // 2. Insert current live imports
        if !live_deps.is_empty() {
            let mut values = Vec::new();
            for dep in live_deps {
                values.push(cozo::DataValue::List(vec![
                    cozo::DataValue::Str(ws.clone().into()),
                    cozo::DataValue::Str(dep.from_file.clone().into()),
                    cozo::DataValue::Str(dep.to_module.clone().into()),
                ]));
            }
            let params =
                BTreeMap::from([("rows".to_string(), cozo::DataValue::List(values))]);
            self.db
                .run_script(
                    "?[workspace, from_file, to_module] <- $rows \
                     :put live_import { workspace, from_file, to_module }",
                    params,
                    ScriptMutability::Mutable,
                )
                .map_err(|e| anyhow::anyhow!("insert live_imports: {:?}", e))?;
        }

        // 3. Cross-reference against modeled contexts (desired state)
        let query_params = params_map(&[("ws", &ws)]);
        let result = self
            .db
            .run_script(
                "modeled[m] := *context{workspace: $ws, module_path: m, state: 'desired'}, m != '' \
                 ?[from_file, to_module] := *live_import{workspace: $ws, from_file, to_module}, \
                     not modeled[to_module]",
                query_params,
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("check_live_dependencies: {:?}", e))?;

        Ok(result
            .rows
            .iter()
            .map(|r| crate::domain::analyze::LiveDependency {
                from_file: dv_str(&r[0]),
                to_module: dv_str(&r[1]),
            })
            .collect())
    }

    // ── Datalog Query Runners ─────────────────────────────────────────────

    /// Run an arbitrary Datalog query with `$ws` parameter.
    pub fn run_datalog(&self, script: &str, workspace: &str) -> Result<Vec<Vec<String>>> {
        let params = params_map(&[("ws", workspace)]);
        let result = self
            .db
            .run_script(script, params, ScriptMutability::Immutable)
            .map_err(|e| anyhow::anyhow!("Datalog query failed: {:?}", e))?;
        Ok(result
            .rows
            .iter()
            .map(|row| row.iter().map(dv_str).collect())
            .collect())
    }

    /// Run an arbitrary Datalog query, returning headers + rows.
    pub fn run_datalog_full(
        &self,
        script: &str,
        workspace: &str,
    ) -> Result<(Vec<String>, Vec<Vec<String>>)> {
        let params = params_map(&[("ws", workspace)]);
        let result = self
            .db
            .run_script(script, params, ScriptMutability::Immutable)
            .map_err(|e| anyhow::anyhow!("Datalog query failed: {:?}", e))?;
        let headers = result.headers.iter().map(|h| h.to_string()).collect();
        let rows = result
            .rows
            .iter()
            .map(|row| row.iter().map(dv_str).collect())
            .collect();
        Ok((headers, rows))
    }

    // ── Datalog Inference Queries (always query desired state) ─────────────

    pub fn transitive_deps(&self, workspace: &str, context: &str) -> Result<Vec<String>> {
        let params = params_map(&[("ws", workspace), ("ctx", context)]);
        let result = self
            .db
            .run_script(
                "transitive[a, c] := *context_dep{workspace: $ws, from_ctx: a, to_ctx: c, state: 'desired'} \
                 transitive[a, c] := transitive[a, b], *context_dep{workspace: $ws, from_ctx: b, to_ctx: c, state: 'desired'} \
                 ?[dep] := transitive[$ctx, dep]",
                params,
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("transitive_deps: {:?}", e))?;
        Ok(result.rows.iter().map(|r| dv_str(&r[0])).collect())
    }

    pub fn circular_deps(&self, workspace: &str) -> Result<Vec<(String, String)>> {
        let params = params_map(&[("ws", workspace)]);
        let result = self
            .db
            .run_script(
                "transitive[a, c] := *context_dep{workspace: $ws, from_ctx: a, to_ctx: c, state: 'desired'} \
                 transitive[a, c] := transitive[a, b], *context_dep{workspace: $ws, from_ctx: b, to_ctx: c, state: 'desired'} \
                 ?[a, b] := transitive[a, b], transitive[b, a]",
                params,
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("circular_deps: {:?}", e))?;
        Ok(result
            .rows
            .iter()
            .map(|r| (dv_str(&r[0]), dv_str(&r[1])))
            .collect())
    }

    pub fn layer_violations(&self, workspace: &str) -> Result<Vec<(String, String, String)>> {
        let params = params_map(&[("ws", workspace)]);
        let result = self
            .db
            .run_script(
                "?[context, service, dep] := \
                    *service{workspace: $ws, context, name: service, kind: 'domain', state: 'desired'}, \
                    *service_dep{workspace: $ws, context, service, dep, state: 'desired'}, \
                    *service{workspace: $ws, context, name: dep, kind: 'infrastructure', state: 'desired'}",
                params,
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("layer_violations: {:?}", e))?;
        Ok(result
            .rows
            .iter()
            .map(|r| (dv_str(&r[0]), dv_str(&r[1]), dv_str(&r[2])))
            .collect())
    }

    // ── Architecture Policy Operations ────────────────────────────────────

    /// Assign a bounded context to an architectural layer.
    pub fn upsert_layer_assignment(
        &self,
        workspace: &str,
        context: &str,
        layer: &str,
    ) -> Result<()> {
        let ws = canonicalize_path(workspace);
        let params = params_map(&[("ws", &ws), ("ctx", context), ("layer", layer)]);
        self.db
            .run_script(
                "?[workspace, context, layer] <- [[$ws, $ctx, $layer]] \
                 :put layer_assignment { workspace, context => layer }",
                params,
                ScriptMutability::Mutable,
            )
            .map_err(|e| anyhow::anyhow!("upsert_layer_assignment: {:?}", e))?;
        Ok(())
    }

    /// Remove a layer assignment for a bounded context.
    pub fn remove_layer_assignment(
        &self,
        workspace: &str,
        context: &str,
    ) -> Result<bool> {
        let ws = canonicalize_path(workspace);
        let params = params_map(&[("ws", &ws), ("ctx", context)]);
        let existing = self
            .db
            .run_script(
                "?[workspace, context] := *layer_assignment{workspace: $ws, context: $ctx} \
                 :rm layer_assignment { workspace, context }",
                params,
                ScriptMutability::Mutable,
            )
            .map_err(|e| anyhow::anyhow!("remove_layer_assignment: {:?}", e))?;
        Ok(!existing.rows.is_empty())
    }

    /// Add a dependency constraint between layers or contexts.
    /// `constraint_kind` is `"layer"` or `"context"`.
    /// `rule` is `"forbidden"` or `"allowed"`.
    pub fn upsert_dependency_constraint(
        &self,
        workspace: &str,
        constraint_kind: &str,
        source: &str,
        target: &str,
        rule: &str,
    ) -> Result<()> {
        let ws = canonicalize_path(workspace);
        let params = params_map(&[
            ("ws", &ws),
            ("kind", constraint_kind),
            ("src", source),
            ("tgt", target),
            ("rule", rule),
        ]);
        self.db
            .run_script(
                "?[workspace, constraint_kind, source, target, rule] <- [[$ws, $kind, $src, $tgt, $rule]] \
                 :put dependency_constraint { workspace, constraint_kind, source, target => rule }",
                params,
                ScriptMutability::Mutable,
            )
            .map_err(|e| anyhow::anyhow!("upsert_dependency_constraint: {:?}", e))?;
        Ok(())
    }

    /// Remove a dependency constraint.
    pub fn remove_dependency_constraint(
        &self,
        workspace: &str,
        constraint_kind: &str,
        source: &str,
        target: &str,
    ) -> Result<bool> {
        let ws = canonicalize_path(workspace);
        let params = params_map(&[
            ("ws", &ws),
            ("kind", constraint_kind),
            ("src", source),
            ("tgt", target),
        ]);
        let existing = self
            .db
            .run_script(
                "?[workspace, constraint_kind, source, target] := \
                    *dependency_constraint{workspace: $ws, constraint_kind: $kind, source: $src, target: $tgt} \
                 :rm dependency_constraint { workspace, constraint_kind, source, target }",
                params,
                ScriptMutability::Mutable,
            )
            .map_err(|e| anyhow::anyhow!("remove_dependency_constraint: {:?}", e))?;
        Ok(!existing.rows.is_empty())
    }

    /// List all layer assignments for a workspace.
    pub fn list_layer_assignments(
        &self,
        workspace: &str,
    ) -> Result<Vec<(String, String)>> {
        let ws = canonicalize_path(workspace);
        let params = params_map(&[("ws", &ws)]);
        let result = self
            .db
            .run_script(
                "?[context, layer] := *layer_assignment{workspace: $ws, context, layer}",
                params,
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("list_layer_assignments: {:?}", e))?;
        Ok(result
            .rows
            .iter()
            .map(|r| (dv_str(&r[0]), dv_str(&r[1])))
            .collect())
    }

    /// List all dependency constraints for a workspace.
    pub fn list_dependency_constraints(
        &self,
        workspace: &str,
    ) -> Result<Vec<(String, String, String, String)>> {
        let ws = canonicalize_path(workspace);
        let params = params_map(&[("ws", &ws)]);
        let result = self
            .db
            .run_script(
                "?[constraint_kind, source, target, rule] := \
                    *dependency_constraint{workspace: $ws, constraint_kind, source, target, rule}",
                params,
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("list_dependency_constraints: {:?}", e))?;
        Ok(result
            .rows
            .iter()
            .map(|r| (dv_str(&r[0]), dv_str(&r[1]), dv_str(&r[2]), dv_str(&r[3])))
            .collect())
    }

    /// Evaluate policy violations: find context dependencies that violate layer
    /// or context-level forbidden constraints.
    pub fn evaluate_policy_violations(
        &self,
        workspace: &str,
    ) -> Result<serde_json::Value> {
        let ws = canonicalize_path(workspace);
        let params = params_map(&[("ws", &ws)]);

        // Layer-based violations: context A (layer X) depends on context B (layer Y)
        // where X→Y is forbidden
        let layer_violations = self
            .db
            .run_script(
                "?[from_ctx, to_ctx, from_layer, to_layer] := \
                    *context_dep{workspace: $ws, from_ctx, to_ctx, state: 'desired'}, \
                    *layer_assignment{workspace: $ws, context: from_ctx, layer: from_layer}, \
                    *layer_assignment{workspace: $ws, context: to_ctx, layer: to_layer}, \
                    *dependency_constraint{workspace: $ws, constraint_kind: 'layer', \
                        source: from_layer, target: to_layer, rule: 'forbidden'}",
                params.clone(),
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("policy layer violations: {:?}", e))?;

        // Context-level violations: context A depends on context B where A→B is forbidden
        let context_violations = self
            .db
            .run_script(
                "?[from_ctx, to_ctx] := \
                    *context_dep{workspace: $ws, from_ctx, to_ctx, state: 'desired'}, \
                    *dependency_constraint{workspace: $ws, constraint_kind: 'context', \
                        source: from_ctx, target: to_ctx, rule: 'forbidden'}",
                params,
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("policy context violations: {:?}", e))?;

        let layer_items: Vec<serde_json::Value> = layer_violations
            .rows
            .iter()
            .map(|r| {
                json!({
                    "kind": "layer",
                    "from_context": dv_str(&r[0]),
                    "to_context": dv_str(&r[1]),
                    "from_layer": dv_str(&r[2]),
                    "to_layer": dv_str(&r[3]),
                    "rule": "forbidden",
                })
            })
            .collect();

        let context_items: Vec<serde_json::Value> = context_violations
            .rows
            .iter()
            .map(|r| {
                json!({
                    "kind": "context",
                    "from_context": dv_str(&r[0]),
                    "to_context": dv_str(&r[1]),
                    "rule": "forbidden",
                })
            })
            .collect();

        let all_violations: Vec<serde_json::Value> =
            layer_items.into_iter().chain(context_items).collect();

        Ok(json!({
            "status": if all_violations.is_empty() { "true" } else { "false" },
            "violations": all_violations,
            "count": all_violations.len(),
        }))
    }

    pub fn impact_analysis(
        &self,
        workspace: &str,
        context: &str,
        entity_name: &str,
    ) -> Result<serde_json::Value> {
        let params = params_map(&[("ws", workspace), ("ctx", context), ("ent", entity_name)]);

        let events = self
            .db
            .run_script(
                "?[context, event_name] := \
                    *event{workspace: $ws, context, name: event_name, source: $ent, state: 'desired'}",
                params.clone(),
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("impact events: {:?}", e))?;

        let services = self
            .db
            .run_script(
                "?[context, service_name] := \
                    *repository{workspace: $ws, context: $ctx, aggregate: $ent, name: repo_name, state: 'desired'}, \
                    *service_dep{workspace: $ws, context, service: service_name, dep: repo_name, state: 'desired'}",
                params.clone(),
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("impact services: {:?}", e))?;

        let reverse_params = params_map(&[("ws", workspace), ("ctx", context)]);
        let dependents = self
            .db
            .run_script(
                "transitive[a, c] := *context_dep{workspace: $ws, from_ctx: a, to_ctx: c, state: 'desired'} \
                 transitive[a, c] := transitive[a, b], *context_dep{workspace: $ws, from_ctx: b, to_ctx: c, state: 'desired'} \
                 ?[dependent] := transitive[dependent, $ctx]",
                reverse_params,
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("impact dependents: {:?}", e))?;

        let ast_impact = self
            .db
            .run_script(
                "ast[target, type] := *ast_edge{workspace: $ws, state: 'actual', from_node: $ent, to_node: target, edge_type: type} \
                 ast[target, type] := ast[mid, _], *ast_edge{workspace: $ws, state: 'actual', from_node: mid, to_node: target, edge_type: type} \
                 ?[target, type] := ast[target, type]",
                params_map(&[("ws", workspace), ("ent", entity_name)]),
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("ast impact: {:?}", e))?;

        Ok(json!({
            "entity": entity_name,
            "context": context,
            "affected_events": events.rows.iter()
                .map(|r| json!({"context": dv_str(&r[0]), "event": dv_str(&r[1])}))
                .collect::<Vec<_>>(),
            "affected_services": services.rows.iter()
                .map(|r| json!({"context": dv_str(&r[0]), "service": dv_str(&r[1])}))
                .collect::<Vec<_>>(),
            "dependent_contexts": dependents.rows.iter()
                .map(|r| dv_str(&r[0]))
                .collect::<Vec<_>>(),
            "ast_impact": ast_impact.rows.iter()
                .map(|r| json!({"target": dv_str(&r[0]), "type": dv_str(&r[1])}))
                .collect::<Vec<_>>(),
        }))
    }

    pub fn aggregate_roots_without_invariants(
        &self,
        workspace: &str,
    ) -> Result<Vec<(String, String)>> {
        let params = params_map(&[("ws", workspace)]);
        let result = self
            .db
            .run_script(
                "has_inv[ctx, ent] := *invariant{workspace: $ws, context: ctx, entity: ent, state: 'desired'} \
                 ?[context, entity] := \
                    *entity{workspace: $ws, context, name: entity, aggregate_root: true, state: 'desired'}, \
                    not has_inv[context, entity]",
                params,
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("aggregate_roots_without_invariants: {:?}", e))?;
        Ok(result
            .rows
            .iter()
            .map(|r| (dv_str(&r[0]), dv_str(&r[1])))
            .collect())
    }


    pub fn query_dependency_path(
        &self,
        workspace: &str,
        from_context: &str,
        to_context: &str,
    ) -> Result<Vec<Vec<String>>> {
        let params = params_map(&[("ws", workspace), ("from_ctx", from_context), ("to_ctx", to_context)]);
        let result = self
            .db
            .run_script(
                "reachable[a, b] := *context_dep{workspace: $ws, from_ctx: a, to_ctx: b, state: 'desired'} \
                 reachable[a, c] := reachable[a, b], *context_dep{workspace: $ws, from_ctx: b, to_ctx: c, state: 'desired'} \
                 on_path[a, b] := *context_dep{workspace: $ws, from_ctx: a, to_ctx: b, state: 'desired'}, reachable[a, $to_ctx], a == $from_ctx \
                 on_path[a, b] := *context_dep{workspace: $ws, from_ctx: a, to_ctx: b, state: 'desired'}, reachable[$from_ctx, a], reachable[b, $to_ctx] \
                 on_path[a, b] := *context_dep{workspace: $ws, from_ctx: a, to_ctx: b, state: 'desired'}, reachable[$from_ctx, a], b == $to_ctx \
                 ?[a, b] := on_path[a, b]",
                params,
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("query_dependency_path: {:?}", e))?;
        
        Ok(result.rows.iter().map(|r| vec![dv_str(&r[0]), dv_str(&r[1])]).collect())
    }

    pub fn can_delete_symbol(
        &self,
        workspace: &str,
        context: &str,
        entity_name: &str,
    ) -> Result<serde_json::Value> {
        let params = params_map(&[("ws", workspace), ("ctx", context), ("ent", entity_name)]);
        
        let aggreg = self.db.run_script(
            "?[agg] := *aggregate_member{workspace: $ws, context: $ctx, member: $ent, state: 'desired', aggregate: agg}",
            params.clone(),
            ScriptMutability::Immutable,
        ).map_err(|e| anyhow::anyhow!("check aggregate: {:?}", e))?;

        let events = self.db.run_script(
            "?[evt] := *event{workspace: $ws, context: $ctx, source: $ent, state: 'desired', name: evt}",
            params.clone(),
            ScriptMutability::Immutable,
        ).map_err(|e| anyhow::anyhow!("check events: {:?}", e))?;
        
        let repos = self.db.run_script(
            "?[repo] := *repository{workspace: $ws, context: $ctx, aggregate: $ent, state: 'desired', name: repo}",
            params.clone(),
            ScriptMutability::Immutable,
        ).map_err(|e| anyhow::anyhow!("check repo: {:?}", e))?;

        let has_deps = !aggreg.rows.is_empty() || !events.rows.is_empty() || !repos.rows.is_empty();

        Ok(serde_json::json!({
            "can_delete": !has_deps,
            "aggregates_referencing": aggreg.rows.iter().map(|r| dv_str(&r[0])).collect::<Vec<_>>(),
            "events_sourced": events.rows.iter().map(|r| dv_str(&r[0])).collect::<Vec<_>>(),
            "repositories_managing": repos.rows.iter().map(|r| dv_str(&r[0])).collect::<Vec<_>>(),
        }))
    }
    pub fn dependency_graph(&self, workspace: &str) -> Result<serde_json::Value> {
        let params = params_map(&[("ws", workspace)]);
        let contexts = self
            .db
            .run_script(
                "?[name, module_path] := *context{workspace: $ws, name, module_path, state: 'desired'}",
                params.clone(),
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("dependency_graph contexts: {:?}", e))?;
        let deps = self
            .db
            .run_script(
                "?[from_ctx, to_ctx] := *context_dep{workspace: $ws, from_ctx, to_ctx, state: 'desired'}",
                params.clone(),
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("dependency_graph deps: {:?}", e))?;
        let circular = self.circular_deps(workspace)?;

        Ok(json!({
            "nodes": contexts.rows.iter()
                .map(|r| json!({"name": dv_str(&r[0]), "module_path": dv_str(&r[1])}))
                .collect::<Vec<_>>(),
            "edges": deps.rows.iter()
                .map(|r| json!({"from": dv_str(&r[0]), "to": dv_str(&r[1])}))
                .collect::<Vec<_>>(),
            "circular_dependencies": circular.iter()
                .map(|(a, b)| json!({"a": a, "b": b}))
                .collect::<Vec<_>>(),
        }))
    }

    // ── Metalayer: Model Health ────────────────────────────────────────────

    pub fn model_health(&self, workspace: &str) -> Result<ModelHealth> {
        let canonical = canonicalize_path(workspace);
        let circular = self.circular_deps(&canonical).unwrap_or_default();
        let violations = self.layer_violations(&canonical).unwrap_or_default();
        let missing_invariants = self
            .aggregate_roots_without_invariants(&canonical)
            .unwrap_or_default();
        let orphans = self.orphan_contexts(&canonical).unwrap_or_default();
        let complexity = self.context_complexity(&canonical).unwrap_or_default();
        let god_contexts: Vec<String> = complexity
            .iter()
            .filter(|c| c.entity_count + c.service_count > 10)
            .map(|c| c.context.clone())
            .collect();
        let unsourced_events = self.unsourced_events(&canonical).unwrap_or_default();

        let critical = circular.len() + violations.len();
        let warnings = missing_invariants.len() + god_contexts.len() + unsourced_events.len();
        let info = orphans.len();
        let score = (100i32 - (critical as i32 * 20) - (warnings as i32 * 5) - (info as i32 * 2))
            .max(0) as u32;

        Ok(ModelHealth {
            score,
            circular_deps: circular.into_iter().map(|(a, b)| [a, b]).collect(),
            layer_violations: violations
                .into_iter()
                .map(|(ctx, svc, dep)| LayerViolation {
                    context: ctx,
                    domain_service: svc,
                    infra_dependency: dep,
                })
                .collect(),
            missing_invariants: missing_invariants
                .into_iter()
                .map(|(ctx, ent)| [ctx, ent])
                .collect(),
            orphan_contexts: orphans,
            god_contexts,
            unsourced_events,
            complexity,
        })
    }

    fn orphan_contexts(&self, workspace: &str) -> Result<Vec<String>> {
        let params = params_map(&[("ws", workspace)]);
        let result = self
            .db
            .run_script(
                "has_dep[ctx] := *context_dep{workspace: $ws, from_ctx: ctx, state: 'desired'} \
                 has_dep[ctx] := *context_dep{workspace: $ws, to_ctx: ctx, state: 'desired'} \
                 ?[name] := *context{workspace: $ws, name, state: 'desired'}, not has_dep[name]",
                params,
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("orphan_contexts: {:?}", e))?;
        Ok(result.rows.iter().map(|r| dv_str(&r[0])).collect())
    }

    fn context_complexity(&self, workspace: &str) -> Result<Vec<ContextComplexity>> {
        let params = params_map(&[("ws", workspace)]);
        let result = self
            .db
            .run_script(
                "ent_count[ctx, count(ent)] := *entity{workspace: $ws, context: ctx, name: ent, state: 'desired'} \
                 ent_count[ctx, 0] := *context{workspace: $ws, name: ctx, state: 'desired'}, not *entity{workspace: $ws, context: ctx, state: 'desired'} \
                 svc_count[ctx, count(svc)] := *service{workspace: $ws, context: ctx, name: svc, state: 'desired'} \
                 svc_count[ctx, 0] := *context{workspace: $ws, name: ctx, state: 'desired'}, not *service{workspace: $ws, context: ctx, state: 'desired'} \
                 evt_count[ctx, count(evt)] := *event{workspace: $ws, context: ctx, name: evt, state: 'desired'} \
                 evt_count[ctx, 0] := *context{workspace: $ws, name: ctx, state: 'desired'}, not *event{workspace: $ws, context: ctx, state: 'desired'} \
                 dep_count[ctx, count(dep)] := *context_dep{workspace: $ws, from_ctx: ctx, to_ctx: dep, state: 'desired'} \
                 dep_count[ctx, 0] := *context{workspace: $ws, name: ctx, state: 'desired'}, not *context_dep{workspace: $ws, from_ctx: ctx, state: 'desired'} \
                 ?[ctx, ents, svcs, evts, deps] := ent_count[ctx, ents], svc_count[ctx, svcs], evt_count[ctx, evts], dep_count[ctx, deps]",
                params,
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("context_complexity: {:?}", e))?;
        Ok(result
            .rows
            .iter()
            .map(|r| ContextComplexity {
                context: dv_str(&r[0]),
                entity_count: dv_u32(&r[1]),
                service_count: dv_u32(&r[2]),
                event_count: dv_u32(&r[3]),
                dep_count: dv_u32(&r[4]),
            })
            .collect())
    }

    fn unsourced_events(&self, workspace: &str) -> Result<Vec<[String; 2]>> {
        let params = params_map(&[("ws", workspace)]);
        let result = self
            .db
            .run_script(
                "?[context, name] := *event{workspace: $ws, context, name, source: '', state: 'desired'}",
                params,
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("unsourced_events: {:?}", e))?;
        Ok(result
            .rows
            .iter()
            .map(|r| [dv_str(&r[0]), dv_str(&r[1])])
            .collect())
    }
}

// ── Data Types ─────────────────────────────────────────────────────────────

/// Metadata about a stored project.
#[derive(Debug, Clone)]
pub struct ProjectInfo {
    pub workspace_path: String,
    pub project_name: String,
    pub updated_at: String,
}

/// Comprehensive model health report computed via Datalog inference.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelHealth {
    pub score: u32,
    pub circular_deps: Vec<[String; 2]>,
    pub layer_violations: Vec<LayerViolation>,
    pub missing_invariants: Vec<[String; 2]>,
    pub orphan_contexts: Vec<String>,
    pub god_contexts: Vec<String>,
    pub unsourced_events: Vec<[String; 2]>,
    pub complexity: Vec<ContextComplexity>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LayerViolation {
    pub context: String,
    pub domain_service: String,
    pub infra_dependency: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ContextComplexity {
    pub context: String,
    pub entity_count: u32,
    pub service_count: u32,
    pub event_count: u32,
    pub dep_count: u32,
}

// ── Helper Functions ───────────────────────────────────────────────────────

/// Normalize workspace path for consistent keying.
pub fn canonicalize_path(path: &str) -> String {
    let normalized = path.trim_end_matches('/');
    match std::fs::canonicalize(normalized) {
        Ok(p) => p.to_string_lossy().to_string(),
        Err(_) => normalized.to_string(),
    }
}

fn params_map(pairs: &[(&str, &str)]) -> BTreeMap<String, cozo::DataValue> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), cozo::DataValue::Str(v.to_string().into())))
        .collect()
}

fn int_dv(n: i64) -> cozo::DataValue {
    cozo::DataValue::Num(cozo::Num::Int(n))
}

/// Extract display string from a DataValue.
fn dv_str(val: &cozo::DataValue) -> String {
    match val {
        cozo::DataValue::Null => String::new(),
        cozo::DataValue::Bool(b) => b.to_string(),
        cozo::DataValue::Num(n) => match n {
            cozo::Num::Int(i) => i.to_string(),
            cozo::Num::Float(f) => f.to_string(),
        },
        cozo::DataValue::Str(s) => s.to_string(),
        cozo::DataValue::List(l) => {
            let items: Vec<String> = l.iter().map(dv_str).collect();
            format!("[{}]", items.join(", "))
        }
        _ => format!("{:?}", val),
    }
}

fn dv_u32(val: &cozo::DataValue) -> u32 {
    match val {
        cozo::DataValue::Num(cozo::Num::Int(i)) => *i as u32,
        cozo::DataValue::Num(cozo::Num::Float(f)) => *f as u32,
        _ => 0,
    }
}

fn dv_i64(val: &cozo::DataValue) -> i64 {
    match val {
        cozo::DataValue::Num(cozo::Num::Int(i)) => *i,
        cozo::DataValue::Num(cozo::Num::Float(f)) => *f as i64,
        _ => 0,
    }
}

fn chrono_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let secs_per_day = 86400u64;
    let days = now / secs_per_day;
    let rem = now % secs_per_day;
    let hours = rem / 3600;
    let minutes = (rem % 3600) / 60;
    let seconds = rem % 60;
    let (year, month, day) = days_to_date(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

fn days_to_date(days: u64) -> (u64, u64, u64) {
    let mut y = 1970;
    let mut remaining = days;
    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        y += 1;
    }
    let month_days: &[u64] = if is_leap(y) {
        &[31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        &[31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut m = 1u64;
    for &md in month_days {
        if remaining < md {
            break;
        }
        remaining -= md;
        m += 1;
    }
    (y, m, remaining + 1)
}

fn is_leap(y: u64) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;

    fn test_model(name: &str) -> DomainModel {
        DomainModel {
            name: name.into(),
            description: "Test project".into(),
            bounded_contexts: vec![],
            external_systems: vec![],
            architectural_decisions: vec![],
            ownership: Ownership::default(),
            rules: vec![],
            tech_stack: TechStack::default(),
            conventions: Conventions::default(),
            ast_edges: vec![],
        }
    }

    fn full_model() -> DomainModel {
        DomainModel {
            name: "FullTest".into(),
            description: "Full test model".into(),
            bounded_contexts: vec![
                BoundedContext {
                    api_endpoints: vec![],
                    name: "Identity".into(),
                    description: "Auth context".into(),
                    module_path: "src/identity".into(),
                    ownership: Ownership::default(),
                    aggregates: vec![],
                    policies: vec![],
                    read_models: vec![],
                    entities: vec![Entity {
                        name: "User".into(),
                        description: "A user".into(),
                        aggregate_root: true,
                        fields: vec![Field {
                            name: "id".into(),
                            field_type: "UserId".into(),
                            required: true,
                            description: "".into(),
                        }],
                        methods: vec![],
                        invariants: vec!["Email must be unique".into()],
                        file_path: None,
                        start_line: None,
                        end_line: None,
                    }],
                    value_objects: vec![],
                    services: vec![Service {
                        name: "AuthService".into(),
                        description: "Handles auth".into(),
                        kind: ServiceKind::Application,
                        methods: vec![],
                        dependencies: vec![],
                        file_path: None,
                        start_line: None,
                        end_line: None,
                    }],
                    repositories: vec![],
                    events: vec![],
                    modules: vec![],
                    dependencies: vec![],
                },
                BoundedContext {
                    api_endpoints: vec![],
                    name: "Billing".into(),
                    description: "Billing context".into(),
                    module_path: "src/billing".into(),
                    ownership: Ownership::default(),
                    aggregates: vec![],
                    policies: vec![],
                    read_models: vec![],
                    entities: vec![Entity {
                        name: "Subscription".into(),
                        description: "A subscription".into(),
                        aggregate_root: false,
                        fields: vec![],
                        methods: vec![],
                        invariants: vec![],
                        file_path: None,
                        start_line: None,
                        end_line: None,
                    }],
                    value_objects: vec![],
                    services: vec![],
                    repositories: vec![],
                    events: vec![],
                    modules: vec![],
                    dependencies: vec!["Identity".into()],
                },
            ],
            external_systems: vec![],
            architectural_decisions: vec![],
            ownership: Ownership::default(),
            rules: vec![ArchitecturalRule {
                id: "LAYER-001".into(),
                description: "Domain must not depend on infra".into(),
                severity: Severity::Error,
                scope: "domain".into(),
            }],
            tech_stack: TechStack::default(),
            conventions: Conventions::default(),
            ast_edges: vec![],
        }
    }

    /// Model with rich sub-structures to exercise field/method/param round-tripping.
    fn rich_model() -> DomainModel {
        DomainModel {
            name: "RichTest".into(),
            description: "Rich model with all sub-structures".into(),
            bounded_contexts: vec![BoundedContext {
                    api_endpoints: vec![],
                name: "Catalog".into(),
                description: "Product catalog".into(),
                module_path: "src/catalog".into(),
                ownership: Ownership::default(),
                aggregates: vec![],
                policies: vec![],
                read_models: vec![],
                entities: vec![Entity {
                    name: "Product".into(),
                    description: "A product".into(),
                    aggregate_root: true,
                    fields: vec![
                        Field {
                            name: "id".into(),
                            field_type: "ProductId".into(),
                            required: true,
                            description: "Primary key".into(),
                        },
                        Field {
                            name: "name".into(),
                            field_type: "String".into(),
                            required: true,
                            description: "".into(),
                        },
                        Field {
                            name: "price".into(),
                            field_type: "Money".into(),
                            required: false,
                            description: "".into(),
                        },
                    ],
                    methods: vec![
                        Method {
                            name: "create".into(),
                            description: "Create a new product".into(),
                            parameters: vec![
                                Field {
                                    name: "name".into(),
                                    field_type: "String".into(),
                                    required: true,
                                    description: "".into(),
                                },
                                Field {
                                    name: "price".into(),
                                    field_type: "Money".into(),
                                    required: true,
                                    description: "".into(),
                                },
                            ],
                            return_type: "Product".into(),
                            file_path: None,
                            start_line: None,
                            end_line: None,
                        },
                        Method {
                            name: "update_price".into(),
                            description: "".into(),
                            parameters: vec![Field {
                                name: "new_price".into(),
                                field_type: "Money".into(),
                                required: true,
                                description: "".into(),
                            }],
                            return_type: "".into(),
                            file_path: None,
                            start_line: None,
                            end_line: None,
                        },
                    ],
                    invariants: vec![
                        "Name must not be empty".into(),
                        "Price must be positive".into(),
                    ],
                    file_path: None,
                    start_line: None,
                    end_line: None,
                }],
                value_objects: vec![ValueObject {
                    name: "Money".into(),
                    description: "Monetary value".into(),
                    fields: vec![
                        Field {
                            name: "amount".into(),
                            field_type: "Decimal".into(),
                            required: true,
                            description: "".into(),
                        },
                        Field {
                            name: "currency".into(),
                            field_type: "String".into(),
                            required: true,
                            description: "".into(),
                        },
                    ],
                    validation_rules: vec![
                        "Amount must be non-negative".into(),
                        "Currency must be ISO 4217".into(),
                    ],
                    file_path: None,
                    start_line: None,
                    end_line: None,
                }],
                services: vec![Service {
                    name: "CatalogService".into(),
                    description: "Application service".into(),
                    kind: ServiceKind::Application,
                    methods: vec![Method {
                        name: "list_products".into(),
                        description: "List all products".into(),
                        parameters: vec![],
                        return_type: "Vec<Product>".into(),
                        file_path: None,
                        start_line: None,
                        end_line: None,
                    }],
                    dependencies: vec![],
                    file_path: None,
                    start_line: None,
                    end_line: None,
                }],
                repositories: vec![Repository {
                    name: "ProductRepository".into(),
                    aggregate: "Product".into(),
                    methods: vec![
                        Method {
                            name: "find_by_id".into(),
                            description: "".into(),
                            parameters: vec![Field {
                                name: "id".into(),
                                field_type: "ProductId".into(),
                                required: true,
                                description: "".into(),
                            }],
                            return_type: "Option<Product>".into(),
                            file_path: None,
                            start_line: None,
                            end_line: None,
                        },
                        Method {
                            name: "save".into(),
                            description: "".into(),
                            parameters: vec![Field {
                                name: "product".into(),
                                field_type: "Product".into(),
                                required: true,
                                description: "".into(),
                            }],
                            return_type: "".into(),
                            file_path: None,
                            start_line: None,
                            end_line: None,
                        },
                    ],
                    file_path: None,
                    start_line: None,
                    end_line: None,
                }],
                events: vec![DomainEvent {
                    name: "ProductCreated".into(),
                    description: "Emitted when a product is created".into(),
                    source: "Product".into(),
                    fields: vec![
                        Field {
                            name: "product_id".into(),
                            field_type: "ProductId".into(),
                            required: true,
                            description: "".into(),
                        },
                        Field {
                            name: "name".into(),
                            field_type: "String".into(),
                            required: true,
                            description: "".into(),
                        },
                    ],
                    file_path: None,
                    start_line: None,
                    end_line: None,
                }],
                modules: vec![],
                dependencies: vec![],
            }],
            external_systems: vec![],
            architectural_decisions: vec![],
            ownership: Ownership::default(),
            rules: vec![],
            tech_stack: TechStack::default(),
            conventions: Conventions::default(),
            ast_edges: vec![],
        }
    }

    fn temp_store() -> Store {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = temp_dir().join(format!(
            "dendrites_cozo_test_{}_{}.db",
            std::process::id(),
            id
        ));
        Store::open(&path).unwrap()
    }

    #[test]
    fn test_save_and_load() {
        let store = temp_store();
        let model = full_model();
        store.save_desired("/tmp/test-save", &model).unwrap();
        let loaded = store.load_desired("/tmp/test-save").unwrap().unwrap();
        assert_eq!(loaded.name, "FullTest");
        assert_eq!(loaded.bounded_contexts.len(), 2);
        let identity = loaded
            .bounded_contexts
            .iter()
            .find(|c| c.name == "Identity")
            .unwrap();
        assert_eq!(identity.entities.len(), 1);
        assert_eq!(identity.entities[0].fields.len(), 1);
        assert_eq!(identity.entities[0].fields[0].name, "id");
        assert_eq!(identity.entities[0].fields[0].field_type, "UserId");
        assert!(identity.entities[0].fields[0].required);
        assert_eq!(loaded.rules.len(), 1);
    }

    #[test]
    fn test_rich_model_round_trip() {
        let store = temp_store();
        let model = rich_model();
        store.save_desired("/tmp/test-rich", &model).unwrap();
        let loaded = store.load_desired("/tmp/test-rich").unwrap().unwrap();

        let catalog = loaded
            .bounded_contexts
            .iter()
            .find(|c| c.name == "Catalog")
            .unwrap();

        // Entity fields
        let product = catalog
            .entities
            .iter()
            .find(|e| e.name == "Product")
            .unwrap();
        assert_eq!(product.fields.len(), 3);
        assert_eq!(product.fields[0].name, "id");
        assert_eq!(product.fields[1].name, "name");
        assert_eq!(product.fields[2].name, "price");
        assert!(!product.fields[2].required);

        // Entity methods + parameters
        assert_eq!(product.methods.len(), 2);
        assert_eq!(product.methods[0].name, "create");
        assert_eq!(product.methods[0].return_type, "Product");
        assert_eq!(product.methods[0].parameters.len(), 2);
        assert_eq!(product.methods[0].parameters[0].name, "name");
        assert_eq!(product.methods[0].parameters[1].name, "price");
        assert_eq!(product.methods[1].name, "update_price");
        assert_eq!(product.methods[1].parameters.len(), 1);

        // Entity invariants (ordered)
        assert_eq!(product.invariants.len(), 2);
        assert_eq!(product.invariants[0], "Name must not be empty");
        assert_eq!(product.invariants[1], "Price must be positive");

        // Value object fields + validation rules
        let money = catalog
            .value_objects
            .iter()
            .find(|v| v.name == "Money")
            .unwrap();
        assert_eq!(money.fields.len(), 2);
        assert_eq!(money.fields[0].name, "amount");
        assert_eq!(money.validation_rules.len(), 2);
        assert_eq!(money.validation_rules[0], "Amount must be non-negative");
        assert_eq!(money.validation_rules[1], "Currency must be ISO 4217");

        // Service methods
        let cat_svc = catalog
            .services
            .iter()
            .find(|s| s.name == "CatalogService")
            .unwrap();
        assert_eq!(cat_svc.methods.len(), 1);
        assert_eq!(cat_svc.methods[0].name, "list_products");
        assert_eq!(cat_svc.methods[0].return_type, "Vec<Product>");
        assert!(cat_svc.methods[0].parameters.is_empty());

        // Repository methods + params
        let repo = catalog
            .repositories
            .iter()
            .find(|r| r.name == "ProductRepository")
            .unwrap();
        assert_eq!(repo.aggregate, "Product");
        assert_eq!(repo.methods.len(), 2);
        assert_eq!(repo.methods[0].name, "find_by_id");
        assert_eq!(repo.methods[0].parameters.len(), 1);
        assert_eq!(repo.methods[0].parameters[0].name, "id");
        assert_eq!(repo.methods[1].name, "save");

        // Event fields
        let evt = catalog
            .events
            .iter()
            .find(|e| e.name == "ProductCreated")
            .unwrap();
        assert_eq!(evt.fields.len(), 2);
        assert_eq!(evt.fields[0].name, "product_id");
        assert_eq!(evt.source, "Product");
    }

    #[test]
    fn test_rich_model_accept_and_reset() {
        let store = temp_store();
        let ws = "/tmp/test-rich-accept";
        store.save_desired(ws, &rich_model()).unwrap();
        store.accept(ws).unwrap();

        let actual = store.load_actual(ws).unwrap().unwrap();
        let cat = actual
            .bounded_contexts
            .iter()
            .find(|c| c.name == "Catalog")
            .unwrap();
        assert_eq!(cat.entities[0].fields.len(), 3);
        assert_eq!(cat.entities[0].methods.len(), 2);
        assert_eq!(cat.value_objects[0].fields.len(), 2);
        assert_eq!(cat.repositories[0].methods.len(), 2);
        assert_eq!(cat.events[0].fields.len(), 2);

        // Modify desired, then reset
        let mut modified = rich_model();
        modified.bounded_contexts[0].entities[0].fields.push(Field {
            name: "sku".into(),
            field_type: "String".into(),
            required: false,
            description: "".into(),
        });
        store.save_desired(ws, &modified).unwrap();
        let desired = store.load_desired(ws).unwrap().unwrap();
        assert_eq!(desired.bounded_contexts[0].entities[0].fields.len(), 4);

        let reset = store.reset(ws).unwrap().unwrap();
        assert_eq!(reset.bounded_contexts[0].entities[0].fields.len(), 3);
    }

    #[test]
    fn test_diff_graph_field_level() {
        let store = temp_store();
        let ws = "/tmp/test-diff-field";
        store.save_desired(ws, &rich_model()).unwrap();
        store.accept(ws).unwrap();

        // Add a field to Product
        let mut modified = rich_model();
        modified.bounded_contexts[0].entities[0].fields.push(Field {
            name: "sku".into(),
            field_type: "String".into(),
            required: false,
            description: "".into(),
        });
        store.save_desired(ws, &modified).unwrap();

        let diff = store.diff_graph(ws).unwrap();
        let changes = diff["pending_changes"].as_array().unwrap();
        assert!(!changes.is_empty());

        // Should contain a field-level add for "sku"
        let field_add = changes
            .iter()
            .find(|c| c["kind"] == "field" && c["name"] == "sku" && c["action"] == "add");
        assert!(field_add.is_some(), "Expected field-level diff for 'sku': {:?}", changes);
        let fa = field_add.unwrap();
        assert_eq!(fa["owner_kind"], "entity");
        assert_eq!(fa["owner"], "Product");
    }

    #[test]
    fn test_diff_graph_method_level() {
        let store = temp_store();
        let ws = "/tmp/test-diff-method";
        store.save_desired(ws, &rich_model()).unwrap();
        store.accept(ws).unwrap();

        // Add a method to CatalogService
        let mut modified = rich_model();
        modified.bounded_contexts[0].services[0]
            .methods
            .push(Method {
                name: "search".into(),
                description: "".into(),
                parameters: vec![],
                return_type: "Vec<Product>".into(),
                file_path: None,
                start_line: None,
                end_line: None,
            });
        store.save_desired(ws, &modified).unwrap();

        let diff = store.diff_graph(ws).unwrap();
        let changes = diff["pending_changes"].as_array().unwrap();

        let method_add = changes
            .iter()
            .find(|c| c["kind"] == "method" && c["name"] == "search" && c["action"] == "add");
        assert!(
            method_add.is_some(),
            "Expected method-level diff for 'search': {:?}",
            changes
        );
        assert_eq!(method_add.unwrap()["owner_kind"], "service");
    }

    #[test]
    fn test_datalog_query_fields() {
        let store = temp_store();
        let ws = "/tmp/test-datalog-fields";
        store.save_desired(ws, &rich_model()).unwrap();

        // Query all entity fields via raw Datalog
        let rows = store
            .run_datalog(
                "?[ctx, entity, field_name, field_type] := \
                    *field{workspace: $ws, context: ctx, owner_kind: 'entity', \
                           owner: entity, name: field_name, state: 'desired', field_type}",
                ws,
            )
            .unwrap();
        assert_eq!(rows.len(), 3); // id, name, price on Product

        // Query all methods across all owner types
        let methods = store
            .run_datalog(
                "?[owner_kind, owner, method_name] := \
                    *method{workspace: $ws, owner_kind, owner, name: method_name, state: 'desired'}",
                ws,
            )
            .unwrap();
        // Product: create, update_price; CatalogService: list_products; ProductRepository: find_by_id, save
        assert_eq!(methods.len(), 5);

        // Query method parameters
        let params = store
            .run_datalog(
                "?[owner, method, param_name, param_type] := \
                    *method_param{workspace: $ws, owner, method, name: param_name, \
                                  state: 'desired', param_type}",
                ws,
            )
            .unwrap();
        // create(name, price), update_price(new_price), find_by_id(id), save(product)
        assert_eq!(params.len(), 5);
    }

    #[test]
    fn test_upsert() {
        let store = temp_store();
        let ws = "/tmp/test-upsert";
        store.save_desired(ws, &test_model("First")).unwrap();
        store.save_desired(ws, &test_model("Second")).unwrap();
        let loaded = store.load_desired(ws).unwrap().unwrap();
        assert_eq!(loaded.name, "Second");
    }

    #[test]
    fn test_load_nonexistent() {
        let store = temp_store();
        assert!(store.load_desired("/tmp/nonexistent").unwrap().is_none());
    }

    #[test]
    fn test_list_projects() {
        let store = temp_store();
        store
            .save_desired("/tmp/test-list-1", &test_model("P1"))
            .unwrap();
        store
            .save_desired("/tmp/test-list-2", &test_model("P2"))
            .unwrap();
        let projects = store.list().unwrap();
        assert_eq!(projects.len(), 2);
    }

    #[test]
    fn test_accept_and_load_actual() {
        let store = temp_store();
        let ws = "/tmp/test-accept";
        let model = full_model();
        store.save_desired(ws, &model).unwrap();
        assert!(store.load_actual(ws).unwrap().is_none());
        store.accept(ws).unwrap();
        let actual = store.load_actual(ws).unwrap().unwrap();
        assert_eq!(actual.bounded_contexts.len(), 2);
    }

    #[test]
    fn test_reset() {
        let store = temp_store();
        let ws = "/tmp/test-reset";
        let model = full_model();
        store.save_desired(ws, &model).unwrap();
        store.accept(ws).unwrap();
        let mut modified = full_model();
        modified.bounded_contexts.push(BoundedContext {
                    api_endpoints: vec![],
            name: "NewCtx".into(),
            description: "".into(),
            module_path: "".into(),
            ownership: Ownership::default(),
            aggregates: vec![],
            policies: vec![],
            read_models: vec![],
            entities: vec![],
            value_objects: vec![],
            services: vec![],
            repositories: vec![],
            events: vec![],
            modules: vec![],
            dependencies: vec![],
        });
        store.save_desired(ws, &modified).unwrap();
        let desired = store.load_desired(ws).unwrap().unwrap();
        assert_eq!(desired.bounded_contexts.len(), 3);
        let reset = store.reset(ws).unwrap().unwrap();
        assert_eq!(reset.bounded_contexts.len(), 2);
    }

    #[test]
    fn test_diff_graph_pure_datalog() {
        let store = temp_store();
        let ws = "/tmp/test-diff";
        let model = full_model();
        store.save_desired(ws, &model).unwrap();
        let diff = store.diff_graph(ws).unwrap();
        let changes = diff["pending_changes"].as_array().unwrap();
        assert!(!changes.is_empty());
        store.accept(ws).unwrap();
        let diff = store.diff_graph(ws).unwrap();
        let changes = diff["pending_changes"].as_array().unwrap();
        assert!(changes.is_empty());
    }

    #[test]
    fn test_transitive_deps() {
        let store = temp_store();
        let ws = "/tmp/test-trans";
        let model = full_model();
        store.save_desired(ws, &model).unwrap();
        let deps = store
            .transitive_deps(&canonicalize_path(ws), "Billing")
            .unwrap();
        assert!(deps.contains(&"Identity".to_string()));
    }

    #[test]
    fn test_circular_deps() {
        let store = temp_store();
        let ws = "/tmp/test-circular";
        let mut model = full_model();
        if let Some(identity) = model
            .bounded_contexts
            .iter_mut()
            .find(|c| c.name == "Identity")
        {
            identity.dependencies.push("Billing".into());
        }
        store.save_desired(ws, &model).unwrap();
        let cycles = store.circular_deps(&canonicalize_path(ws)).unwrap();
        assert!(!cycles.is_empty());
    }

    #[test]
    fn test_no_circular_deps() {
        let store = temp_store();
        let ws = "/tmp/test-no-circ";
        store.save_desired(ws, &full_model()).unwrap();
        let cycles = store.circular_deps(&canonicalize_path(ws)).unwrap();
        assert!(cycles.is_empty());
    }

    #[test]
    fn test_aggregate_roots_without_invariants() {
        let store = temp_store();
        let ws = "/tmp/test-agg";
        let model = full_model();
        store.save_desired(ws, &model).unwrap();
        let missing = store
            .aggregate_roots_without_invariants(&canonicalize_path(ws))
            .unwrap();
        assert!(missing.is_empty());
    }

    #[test]
    fn test_impact_analysis() {
        let store = temp_store();
        let ws = "/tmp/test-impact";
        store.save_desired(ws, &full_model()).unwrap();
        let canonical = canonicalize_path(ws);
        let result = store
            .impact_analysis(&canonical, "Identity", "User")
            .unwrap();
        assert!(result.get("entity").is_some());
    }

    #[test]
    fn test_dependency_graph() {
        let store = temp_store();
        let ws = "/tmp/test-depgraph";
        store.save_desired(ws, &full_model()).unwrap();
        let canonical = canonicalize_path(ws);
        let graph = store.dependency_graph(&canonical).unwrap();
        let nodes = graph["nodes"].as_array().unwrap();
        let edges = graph["edges"].as_array().unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0]["from"], "Billing");
        assert_eq!(edges[0]["to"], "Identity");
    }

    #[test]
    fn test_raw_datalog_query() {
        let store = temp_store();
        let model = full_model();
        store.save_desired("/tmp/test-raw", &model).unwrap();
        let rows = store
            .run_datalog(
                "?[name, aggregate_root] := *entity{workspace: $ws, name, aggregate_root, state: 'desired'}",
                "/tmp/test-raw",
            )
            .unwrap();
        assert_eq!(rows.len(), 2);
    }
}
