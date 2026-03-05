use anyhow::{Context, Result};
use cozo::{DbInstance, ScriptMutability};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::domain::model::DomainModel;

/// CozoDB-backed store for domain models, keyed by workspace path.
/// Database lives at `~/.dendrites/dendrites.db`.
///
/// Architecture:
/// - `project` relation stores full JSON models (backward-compatible import/export)
/// - Relational decomposition into `context`, `entity`, `service`, `event`, etc.
///   enables Datalog-based inference queries (transitive deps, circular deps,
///   layer violations, impact analysis).
pub struct Store {
    db: DbInstance,
}

impl Store {
    /// Open (or create) the store at the default location `~/.dendrites/dendrites.db`.
    pub fn open_default() -> Result<Self> {
        let db_path = default_db_path()?;
        Self::open(&db_path)
    }

    /// Open (or create) the store at a specific path.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }

        let db = DbInstance::new("sqlite", path.to_str().unwrap_or(""), Default::default())
            .map_err(|e| anyhow::anyhow!("Failed to open CozoDB: {:?}", e))?;

        // Initialize schema — run each :create individually, ignoring
        // "already exists" errors for idempotent re-opens.
        Self::init_schema(&db)?;

        Ok(Self { db })
    }

    /// Initialize all CozoDB stored relations. Silently skips already-existing ones.
    fn init_schema(db: &DbInstance) -> Result<()> {
        let schemas = vec![
            ":create project { workspace_path: String => project_name: String, model_json: String, baseline_json: String? default null, updated_at: String }",
            ":create context { workspace: String, name: String => description: String default '', module_path: String default '' }",
            ":create context_dep { workspace: String, from_ctx: String, to_ctx: String }",
            ":create entity { workspace: String, context: String, name: String => description: String default '', aggregate_root: Bool default false }",
            ":create entity_field { workspace: String, context: String, entity: String, name: String => field_type: String default '', required: Bool default false, description: String default '' }",
            ":create entity_method { workspace: String, context: String, entity: String, name: String => description: String default '', return_type: String default '' }",
            ":create method_param { workspace: String, context: String, owner_kind: String, owner: String, method: String, name: String => param_type: String default '' }",
            ":create invariant { workspace: String, context: String, entity: String, idx: Int => text: String }",
            ":create service { workspace: String, context: String, name: String => description: String default '', kind: String default 'domain' }",
            ":create service_dep { workspace: String, context: String, service: String, dep: String }",
            ":create service_method { workspace: String, context: String, service: String, name: String => description: String default '', return_type: String default '' }",
            ":create event { workspace: String, context: String, name: String => description: String default '', source: String default '' }",
            ":create event_field { workspace: String, context: String, event: String, name: String => field_type: String default '', required: Bool default false, description: String default '' }",
            ":create value_object { workspace: String, context: String, name: String => description: String default '' }",
            ":create repository { workspace: String, context: String, name: String => aggregate: String default '' }",
            ":create arch_rule { workspace: String, id: String => description: String default '', severity: String default 'error', scope: String default '' }",
        ];

        for schema in schemas {
            // Ignore "already exists" errors — this makes open() idempotent
            let _ = db.run_script(schema, Default::default(), ScriptMutability::Mutable);
        }
        Ok(())
    }

    /// Load the desired domain model for a workspace. Returns `None` if no model exists.
    pub fn load_desired(&self, workspace_path: &str) -> Result<Option<DomainModel>> {
        let canonical = canonicalize_path(workspace_path);
        let params = params_map(&[("ws", &canonical)]);

        let result = self
            .db
            .run_script(
                "?[model_json] := *project{workspace_path: $ws, model_json}",
                params,
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("Failed to query project: {:?}", e))?;

        if result.rows.is_empty() {
            return Ok(None);
        }

        let json = row_string(&result.rows[0], 0)?;
        let model: DomainModel = serde_json::from_str(&json)
            .context("Failed to parse stored domain model")?;
        Ok(Some(model))
    }

    /// Save (upsert) the desired domain model for a workspace.
    /// Also syncs the relational decomposition for Datalog inference.
    pub fn save_desired(&self, workspace_path: &str, model: &DomainModel) -> Result<()> {
        let canonical = canonicalize_path(workspace_path);
        let json = serde_json::to_string_pretty(model)
            .context("Failed to serialize domain model")?;
        let now = chrono_now();

        // Read existing baseline_json (if any) so :put doesn't overwrite it with null
        let existing_baseline = {
            let params = params_map(&[("ws", &canonical)]);
            let result = self.db.run_script(
                "?[baseline_json] := *project{workspace_path: $ws, baseline_json}",
                params,
                ScriptMutability::Immutable,
            );
            match result {
                Ok(r) if !r.rows.is_empty() => {
                    match &r.rows[0][0] {
                        cozo::DataValue::Str(s) => Some(s.to_string()),
                        _ => None,
                    }
                }
                _ => None,
            }
        };

        // Build params — include baseline if it exists
        let baseline_placeholder = existing_baseline.unwrap_or_default();
        let has_baseline = !baseline_placeholder.is_empty();

        if has_baseline {
            let params = params_map(&[
                ("ws", &canonical),
                ("name", &model.name),
                ("json", &json),
                ("baseline", &baseline_placeholder),
                ("now", &now),
            ]);
            self.db
                .run_script(
                    "?[workspace_path, project_name, model_json, baseline_json, updated_at] <- \
                        [[$ws, $name, $json, $baseline, $now]] \
                     :put project { workspace_path => project_name, model_json, baseline_json, updated_at }",
                    params,
                    ScriptMutability::Mutable,
                )
                .map_err(|e| anyhow::anyhow!("Failed to save domain model: {:?}", e))?;
        } else {
            let params = params_map(&[
                ("ws", &canonical),
                ("name", &model.name),
                ("json", &json),
                ("now", &now),
            ]);
            self.db
                .run_script(
                    "?[workspace_path, project_name, model_json, updated_at] <- \
                        [[$ws, $name, $json, $now]] \
                     :put project { workspace_path => project_name, model_json, updated_at }",
                    params,
                    ScriptMutability::Mutable,
                )
                .map_err(|e| anyhow::anyhow!("Failed to save domain model: {:?}", e))?;
        }

        // Sync relational decomposition for Datalog queries
        self.sync_relations(&canonical, model)?;

        Ok(())
    }

    /// List all stored projects with their workspace paths and names.
    pub fn list(&self) -> Result<Vec<ProjectInfo>> {
        let result = self
            .db
            .run_script(
                "?[workspace_path, project_name, updated_at] := \
                    *project{workspace_path, project_name, updated_at}",
                Default::default(),
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("Failed to list projects: {:?}", e))?;

        let mut projects = Vec::new();
        for row in &result.rows {
            projects.push(ProjectInfo {
                workspace_path: row_string(row, 0)?,
                project_name: row_string(row, 1)?,
                updated_at: row_string(row, 2)?,
            });
        }

        // Sort by updated_at descending
        projects.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(projects)
    }

    /// Load the actual domain model for a workspace (reflects current implementation).
    pub fn load_actual(&self, workspace_path: &str) -> Result<Option<DomainModel>> {
        let canonical = canonicalize_path(workspace_path);
        let params = params_map(&[("ws", &canonical)]);

        let result = self
            .db
            .run_script(
                "?[baseline_json] := *project{workspace_path: $ws, baseline_json}",
                params,
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("Failed to query actual model: {:?}", e))?;

        if result.rows.is_empty() {
            return Ok(None);
        }

        let val = &result.rows[0][0];
        match val {
            cozo::DataValue::Null => Ok(None),
            cozo::DataValue::Str(s) => {
                let model: DomainModel = serde_json::from_str(s)
                    .context("Failed to parse stored actual model")?;
                Ok(Some(model))
            }
            _ => Ok(None),
        }
    }

    /// Accept: promote desired → actual (baseline_json = model_json).
    pub fn accept(&self, workspace_path: &str) -> Result<()> {
        let canonical = canonicalize_path(workspace_path);
        let params = params_map(&[("ws", &canonical)]);

        // Read model_json and other value columns
        let result = self
            .db
            .run_script(
                "?[project_name, model_json, updated_at] := \
                    *project{workspace_path: $ws, project_name, model_json, updated_at}",
                params.clone(),
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("Failed to read project for accept: {:?}", e))?;

        if result.rows.is_empty() {
            return Err(anyhow::anyhow!("No project found for workspace"));
        }

        let project_name = row_string(&result.rows[0], 0)?;
        let model_json = row_string(&result.rows[0], 1)?;
        let updated_at = row_string(&result.rows[0], 2)?;

        // Re-insert with baseline_json = model_json using :put (full row replacement)
        let params2 = params_map(&[
            ("ws", &canonical),
            ("name", &project_name),
            ("json", &model_json),
            ("baseline", &model_json),
            ("at", &updated_at),
        ]);
        self.db
            .run_script(
                "?[workspace_path, project_name, model_json, baseline_json, updated_at] <- \
                    [[$ws, $name, $json, $baseline, $at]] \
                 :put project { workspace_path => project_name, model_json, baseline_json, updated_at }",
                params2,
                ScriptMutability::Mutable,
            )
            .map_err(|e| anyhow::anyhow!("Failed to accept desired model: {:?}", e))?;
        Ok(())
    }

    /// Reset: revert desired → actual (model_json = baseline_json).
    pub fn reset(&self, workspace_path: &str) -> Result<Option<DomainModel>> {
        let canonical = canonicalize_path(workspace_path);
        let params = params_map(&[("ws", &canonical)]);

        // Read the full row
        let result = self
            .db
            .run_script(
                "?[project_name, model_json, baseline_json, updated_at] := \
                    *project{workspace_path: $ws, project_name, model_json, baseline_json, updated_at}",
                params.clone(),
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("Failed to query project for reset: {:?}", e))?;

        if result.rows.is_empty() {
            return Ok(None);
        }

        let baseline_val = &result.rows[0][2];
        let baseline = match baseline_val {
            cozo::DataValue::Str(s) => s.to_string(),
            cozo::DataValue::Null => return Ok(None),
            _ => return Ok(None),
        };

        let project_name = row_string(&result.rows[0], 0)?;
        let updated_at = row_string(&result.rows[0], 3)?;

        // Re-insert with model_json = baseline_json using :put
        let params2 = params_map(&[
            ("ws", &canonical),
            ("name", &project_name),
            ("json", &baseline),
            ("baseline", &baseline),
            ("at", &updated_at),
        ]);
        self.db
            .run_script(
                "?[workspace_path, project_name, model_json, baseline_json, updated_at] <- \
                    [[$ws, $name, $json, $baseline, $at]] \
                 :put project { workspace_path => project_name, model_json, baseline_json, updated_at }",
                params2,
                ScriptMutability::Mutable,
            )
            .map_err(|e| anyhow::anyhow!("Failed to reset desired model: {:?}", e))?;

        // Re-sync relations with the baseline model
        let model: DomainModel = serde_json::from_str(&baseline)
            .context("Failed to parse baseline model")?;
        self.sync_relations(&canonical, &model)?;

        self.load_desired(workspace_path)
    }

    /// Import a domain model from a JSON file into the store for a given workspace.
    pub fn import_from_file(&self, workspace_path: &str, file_path: &str) -> Result<DomainModel> {
        let model = DomainModel::load(file_path)?;
        self.save_desired(workspace_path, &model)?;
        self.accept(workspace_path)?;
        Ok(model)
    }

    /// Export a domain model from the store to a JSON file.
    pub fn export_to_file(&self, workspace_path: &str, file_path: &str) -> Result<()> {
        let model = self
            .load_desired(workspace_path)?
            .with_context(|| format!("No model found for workspace: {workspace_path}"))?;
        let json = serde_json::to_string_pretty(&model)?;
        std::fs::write(file_path, json)
            .with_context(|| format!("Failed to write file: {file_path}"))?;
        Ok(())
    }

    // ─── Relational Decomposition (sync model → CozoDB relations) ──────────

    /// Sync the domain model into CozoDB's relational tuples for Datalog queries.
    /// This clears all existing tuples for this workspace and re-inserts from the model.
    fn sync_relations(&self, workspace: &str, model: &DomainModel) -> Result<()> {
        // Clear existing tuples for this workspace
        self.clear_workspace_relations(workspace)?;

        // Insert bounded contexts
        for bc in &model.bounded_contexts {
            self.insert_context(workspace, bc)?;
        }

        // Insert architectural rules
        for rule in &model.rules {
            let params = params_map(&[
                ("ws", workspace),
                ("id", &rule.id),
                ("desc", &rule.description),
                ("sev", &format!("{:?}", rule.severity).to_lowercase()),
                ("scope", &rule.scope),
            ]);
            self.db
                .run_script(
                    "?[workspace, id, description, severity, scope] <- \
                        [[$ws, $id, $desc, $sev, $scope]] \
                     :put arch_rule { workspace, id => description, severity, scope }",
                    params,
                    ScriptMutability::Mutable,
                )
                .map_err(|e| anyhow::anyhow!("Failed to insert rule: {:?}", e))?;
        }

        Ok(())
    }

    /// Clear all relational tuples for a workspace.
    fn clear_workspace_relations(&self, workspace: &str) -> Result<()> {
        let params = params_map(&[("ws", workspace)]);
        let relations = [
            ("context", "workspace, name"),
            ("context_dep", "workspace, from_ctx, to_ctx"),
            ("entity", "workspace, context, name"),
            ("entity_field", "workspace, context, entity, name"),
            ("entity_method", "workspace, context, entity, name"),
            ("method_param", "workspace, context, owner_kind, owner, method, name"),
            ("invariant", "workspace, context, entity, idx"),
            ("service", "workspace, context, name"),
            ("service_dep", "workspace, context, service, dep"),
            ("service_method", "workspace, context, service, name"),
            ("event", "workspace, context, name"),
            ("event_field", "workspace, context, event, name"),
            ("value_object", "workspace, context, name"),
            ("repository", "workspace, context, name"),
            ("arch_rule", "workspace, id"),
        ];

        for (rel, keys) in relations {
            let key_list: Vec<&str> = keys.split(", ").collect();
            let binding = key_list.join(", ");
            let script = format!(
                "?[{binding}] := *{rel}{{{binding}}}, workspace = $ws :rm {rel} {{{binding}}}"
            );
            self.db
                .run_script(&script, params.clone(), ScriptMutability::Mutable)
                .map_err(|e| {
                    anyhow::anyhow!("Failed to clear {rel} for workspace: {:?}", e)
                })?;
        }

        Ok(())
    }

    /// Insert a bounded context and all its children into CozoDB relations.
    fn insert_context(
        &self,
        workspace: &str,
        bc: &crate::domain::model::BoundedContext,
    ) -> Result<()> {
        // Insert context
        let params = params_map(&[
            ("ws", workspace),
            ("name", &bc.name),
            ("desc", &bc.description),
            ("mod_path", &bc.module_path),
        ]);
        self.db
            .run_script(
                "?[workspace, name, description, module_path] <- \
                    [[$ws, $name, $desc, $mod_path]] \
                 :put context { workspace, name => description, module_path }",
                params,
                ScriptMutability::Mutable,
            )
            .map_err(|e| anyhow::anyhow!("Failed to insert context: {:?}", e))?;

        // Insert context dependencies
        for dep in &bc.dependencies {
            let params = params_map(&[("ws", workspace), ("from", &bc.name), ("to", dep)]);
            self.db
                .run_script(
                    "?[workspace, from_ctx, to_ctx] <- [[$ws, $from, $to]] \
                     :put context_dep { workspace, from_ctx, to_ctx }",
                    params,
                    ScriptMutability::Mutable,
                )
                .map_err(|e| anyhow::anyhow!("Failed to insert context dep: {:?}", e))?;
        }

        // Insert entities
        for entity in &bc.entities {
            self.insert_entity(workspace, &bc.name, entity)?;
        }

        // Insert services
        for svc in &bc.services {
            self.insert_service(workspace, &bc.name, svc)?;
        }

        // Insert events
        for evt in &bc.events {
            self.insert_event(workspace, &bc.name, evt)?;
        }

        // Insert value objects
        for vo in &bc.value_objects {
            let params = params_map(&[
                ("ws", workspace),
                ("ctx", &bc.name),
                ("name", &vo.name),
                ("desc", &vo.description),
            ]);
            self.db
                .run_script(
                    "?[workspace, context, name, description] <- \
                        [[$ws, $ctx, $name, $desc]] \
                     :put value_object { workspace, context, name => description }",
                    params,
                    ScriptMutability::Mutable,
                )
                .map_err(|e| anyhow::anyhow!("Failed to insert value object: {:?}", e))?;
        }

        // Insert repositories
        for repo in &bc.repositories {
            let params = params_map(&[
                ("ws", workspace),
                ("ctx", &bc.name),
                ("name", &repo.name),
                ("agg", &repo.aggregate),
            ]);
            self.db
                .run_script(
                    "?[workspace, context, name, aggregate] <- \
                        [[$ws, $ctx, $name, $agg]] \
                     :put repository { workspace, context, name => aggregate }",
                    params,
                    ScriptMutability::Mutable,
                )
                .map_err(|e| anyhow::anyhow!("Failed to insert repository: {:?}", e))?;
        }

        Ok(())
    }

    /// Insert an entity and its fields, methods, invariants.
    fn insert_entity(
        &self,
        workspace: &str,
        context: &str,
        entity: &crate::domain::model::Entity,
    ) -> Result<()> {
        let params = params_map(&[
            ("ws", workspace),
            ("ctx", context),
            ("name", &entity.name),
            ("desc", &entity.description),
        ]);
        // Need to pass aggregate_root as a boolean, not string
        let mut p = params;
        p.insert(
            "agg".into(),
            cozo::DataValue::Bool(entity.aggregate_root),
        );
        self.db
            .run_script(
                "?[workspace, context, name, description, aggregate_root] <- \
                    [[$ws, $ctx, $name, $desc, $agg]] \
                 :put entity { workspace, context, name => description, aggregate_root }",
                p,
                ScriptMutability::Mutable,
            )
            .map_err(|e| anyhow::anyhow!("Failed to insert entity '{}': {:?}", entity.name, e))?;

        // Fields
        for field in &entity.fields {
            let mut params = params_map(&[
                ("ws", workspace),
                ("ctx", context),
                ("ent", &entity.name),
                ("name", &field.name),
                ("ftype", &field.field_type),
                ("desc", &field.description),
            ]);
            params.insert("req".into(), cozo::DataValue::Bool(field.required));
            self.db
                .run_script(
                    "?[workspace, context, entity, name, field_type, required, description] <- \
                        [[$ws, $ctx, $ent, $name, $ftype, $req, $desc]] \
                     :put entity_field { workspace, context, entity, name \
                        => field_type, required, description }",
                    params,
                    ScriptMutability::Mutable,
                )
                .map_err(|e| anyhow::anyhow!("Failed to insert entity field: {:?}", e))?;
        }

        // Methods
        for method in &entity.methods {
            let params = params_map(&[
                ("ws", workspace),
                ("ctx", context),
                ("ent", &entity.name),
                ("name", &method.name),
                ("desc", &method.description),
                ("rtype", &method.return_type),
            ]);
            self.db
                .run_script(
                    "?[workspace, context, entity, name, description, return_type] <- \
                        [[$ws, $ctx, $ent, $name, $desc, $rtype]] \
                     :put entity_method { workspace, context, entity, name \
                        => description, return_type }",
                    params,
                    ScriptMutability::Mutable,
                )
                .map_err(|e| anyhow::anyhow!("Failed to insert entity method: {:?}", e))?;

            // Method parameters
            for param in &method.parameters {
                let params = params_map(&[
                    ("ws", workspace),
                    ("ctx", context),
                    ("kind", "entity"),
                    ("owner", &entity.name),
                    ("method", &method.name),
                    ("name", &param.name),
                    ("ptype", &param.field_type),
                ]);
                self.db
                    .run_script(
                        "?[workspace, context, owner_kind, owner, method, name, param_type] <- \
                            [[$ws, $ctx, $kind, $owner, $method, $name, $ptype]] \
                         :put method_param { workspace, context, owner_kind, owner, method, name \
                            => param_type }",
                        params,
                        ScriptMutability::Mutable,
                    )
                    .map_err(|e| anyhow::anyhow!("Failed to insert method param: {:?}", e))?;
            }
        }

        // Invariants
        for (idx, inv) in entity.invariants.iter().enumerate() {
            let params = params_map(&[
                ("ws", workspace),
                ("ctx", context),
                ("ent", &entity.name),
                ("text", inv),
            ]);
            let mut p = params;
            p.insert(
                "idx".into(),
                cozo::DataValue::Num(cozo::Num::Int(idx as i64)),
            );
            self.db
                .run_script(
                    "?[workspace, context, entity, idx, text] <- \
                        [[$ws, $ctx, $ent, $idx, $text]] \
                     :put invariant { workspace, context, entity, idx => text }",
                    p,
                    ScriptMutability::Mutable,
                )
                .map_err(|e| anyhow::anyhow!("Failed to insert invariant: {:?}", e))?;
        }

        Ok(())
    }

    /// Insert a service and its methods, dependencies.
    fn insert_service(
        &self,
        workspace: &str,
        context: &str,
        svc: &crate::domain::model::Service,
    ) -> Result<()> {
        let kind_str = format!("{:?}", svc.kind).to_lowercase();
        let params = params_map(&[
            ("ws", workspace),
            ("ctx", context),
            ("name", &svc.name),
            ("desc", &svc.description),
            ("kind", &kind_str),
        ]);
        self.db
            .run_script(
                "?[workspace, context, name, description, kind] <- \
                    [[$ws, $ctx, $name, $desc, $kind]] \
                 :put service { workspace, context, name => description, kind }",
                params,
                ScriptMutability::Mutable,
            )
            .map_err(|e| anyhow::anyhow!("Failed to insert service: {:?}", e))?;

        // Service dependencies
        for dep in &svc.dependencies {
            let params = params_map(&[
                ("ws", workspace),
                ("ctx", context),
                ("svc", &svc.name),
                ("dep", dep),
            ]);
            self.db
                .run_script(
                    "?[workspace, context, service, dep] <- [[$ws, $ctx, $svc, $dep]] \
                     :put service_dep { workspace, context, service, dep }",
                    params,
                    ScriptMutability::Mutable,
                )
                .map_err(|e| anyhow::anyhow!("Failed to insert service dep: {:?}", e))?;
        }

        // Service methods
        for method in &svc.methods {
            let params = params_map(&[
                ("ws", workspace),
                ("ctx", context),
                ("svc", &svc.name),
                ("name", &method.name),
                ("desc", &method.description),
                ("rtype", &method.return_type),
            ]);
            self.db
                .run_script(
                    "?[workspace, context, service, name, description, return_type] <- \
                        [[$ws, $ctx, $svc, $name, $desc, $rtype]] \
                     :put service_method { workspace, context, service, name \
                        => description, return_type }",
                    params,
                    ScriptMutability::Mutable,
                )
                .map_err(|e| anyhow::anyhow!("Failed to insert service method: {:?}", e))?;
        }

        Ok(())
    }

    /// Insert a domain event and its fields.
    fn insert_event(
        &self,
        workspace: &str,
        context: &str,
        evt: &crate::domain::model::DomainEvent,
    ) -> Result<()> {
        let params = params_map(&[
            ("ws", workspace),
            ("ctx", context),
            ("name", &evt.name),
            ("desc", &evt.description),
            ("src", &evt.source),
        ]);
        self.db
            .run_script(
                "?[workspace, context, name, description, source] <- \
                    [[$ws, $ctx, $name, $desc, $src]] \
                 :put event { workspace, context, name => description, source }",
                params,
                ScriptMutability::Mutable,
            )
            .map_err(|e| anyhow::anyhow!("Failed to insert event: {:?}", e))?;

        // Event fields
        for field in &evt.fields {
            let mut params = params_map(&[
                ("ws", workspace),
                ("ctx", context),
                ("evt", &evt.name),
                ("name", &field.name),
                ("ftype", &field.field_type),
                ("desc", &field.description),
            ]);
            params.insert("req".into(), cozo::DataValue::Bool(field.required));
            self.db
                .run_script(
                    "?[workspace, context, event, name, field_type, required, description] <- \
                        [[$ws, $ctx, $evt, $name, $ftype, $req, $desc]] \
                     :put event_field { workspace, context, event, name \
                        => field_type, required, description }",
                    params,
                    ScriptMutability::Mutable,
                )
                .map_err(|e| anyhow::anyhow!("Failed to insert event field: {:?}", e))?;
        }

        Ok(())
    }

    // ─── Datalog Inference Queries ──────────────────────────────────────────

    /// Run a Datalog query against the domain model and return raw results.
    #[allow(dead_code)]
    pub fn run_datalog(&self, script: &str, workspace: &str) -> Result<Vec<Vec<String>>> {
        let params = params_map(&[("ws", workspace)]);
        let result = self
            .db
            .run_script(script, params, ScriptMutability::Immutable)
            .map_err(|e| anyhow::anyhow!("Datalog query failed: {:?}", e))?;

        let mut rows = Vec::new();
        for row in &result.rows {
            let mut cells = Vec::new();
            for val in row {
                cells.push(datavalue_to_string(val));
            }
            rows.push(cells);
        }
        Ok(rows)
    }

    /// Get headers + rows from a Datalog query.
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
        let mut rows = Vec::new();
        for row in &result.rows {
            let mut cells = Vec::new();
            for val in row {
                cells.push(datavalue_to_string(val));
            }
            rows.push(cells);
        }
        Ok((headers, rows))
    }

    /// Detect transitive dependencies from a bounded context.
    /// Uses Datalog recursion: `transitive[a, c] := *context_dep{..., from_ctx: a, to_ctx: c}`
    ///                         `transitive[a, c] := transitive[a, b], *context_dep{..., from_ctx: b, to_ctx: c}`
    pub fn transitive_deps(&self, workspace: &str, context: &str) -> Result<Vec<String>> {
        let params = params_map(&[("ws", workspace), ("ctx", context)]);
        let result = self
            .db
            .run_script(
                "transitive[a, c] := *context_dep{workspace: $ws, from_ctx: a, to_ctx: c} \
                 transitive[a, c] := transitive[a, b], *context_dep{workspace: $ws, from_ctx: b, to_ctx: c} \
                 ?[dep] := transitive[$ctx, dep]",
                params,
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("Transitive dep query failed: {:?}", e))?;

        Ok(result
            .rows
            .iter()
            .map(|r| datavalue_to_string(&r[0]))
            .collect())
    }

    /// Detect circular dependencies in the context dependency graph.
    /// A cycle exists if context A transitively depends on itself.
    pub fn circular_deps(&self, workspace: &str) -> Result<Vec<(String, String)>> {
        let params = params_map(&[("ws", workspace)]);
        let result = self
            .db
            .run_script(
                "transitive[a, c] := *context_dep{workspace: $ws, from_ctx: a, to_ctx: c} \
                 transitive[a, c] := transitive[a, b], *context_dep{workspace: $ws, from_ctx: b, to_ctx: c} \
                 ?[a, b] := transitive[a, b], transitive[b, a]",
                params,
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("Circular dep query failed: {:?}", e))?;

        Ok(result
            .rows
            .iter()
            .map(|r| (datavalue_to_string(&r[0]), datavalue_to_string(&r[1])))
            .collect())
    }

    /// Find layer violations: domain entities/services that depend on infrastructure.
    /// Checks if any service in the domain layer has a dependency on an infrastructure service.
    pub fn layer_violations(&self, workspace: &str) -> Result<Vec<(String, String, String)>> {
        let params = params_map(&[("ws", workspace)]);
        let result = self
            .db
            .run_script(
                "?[context, service, dep] := \
                    *service{workspace: $ws, context, name: service, kind: 'domain'}, \
                    *service_dep{workspace: $ws, context, service, dep}, \
                    *service{workspace: $ws, context, name: dep, kind: 'infrastructure'}",
                params,
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("Layer violation query failed: {:?}", e))?;

        Ok(result
            .rows
            .iter()
            .map(|r| {
                (
                    datavalue_to_string(&r[0]),
                    datavalue_to_string(&r[1]),
                    datavalue_to_string(&r[2]),
                )
            })
            .collect())
    }

    /// Impact analysis: what entities, services, and events are affected by changing
    /// a given entity (through events, services, and context dependencies).
    pub fn impact_analysis(
        &self,
        workspace: &str,
        context: &str,
        entity_name: &str,
    ) -> Result<serde_json::Value> {
        let params = params_map(&[
            ("ws", workspace),
            ("ctx", context),
            ("ent", entity_name),
        ]);

        // Events sourced from this entity
        let events = self
            .db
            .run_script(
                "?[context, event_name] := \
                    *event{workspace: $ws, context, name: event_name, source: $ent}",
                params.clone(),
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("Impact events query failed: {:?}", e))?;

        // Services that depend on repositories for this entity's aggregate
        let services = self
            .db
            .run_script(
                "?[context, service_name] := \
                    *repository{workspace: $ws, context: $ctx, aggregate: $ent, name: repo_name}, \
                    *service_dep{workspace: $ws, context, service: service_name, dep: repo_name}",
                params.clone(),
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("Impact services query failed: {:?}", e))?;

        // Contexts that DEPEND ON this context (reverse deps)
        let reverse_params = params_map(&[("ws", workspace), ("ctx", context)]);
        let dependents = self
            .db
            .run_script(
                "transitive[a, c] := *context_dep{workspace: $ws, from_ctx: a, to_ctx: c} \
                 transitive[a, c] := transitive[a, b], *context_dep{workspace: $ws, from_ctx: b, to_ctx: c} \
                 ?[dependent] := transitive[dependent, $ctx]",
                reverse_params,
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("Reverse dep query failed: {:?}", e))?;

        Ok(serde_json::json!({
            "entity": entity_name,
            "context": context,
            "affected_events": events.rows.iter()
                .map(|r| serde_json::json!({
                    "context": datavalue_to_string(&r[0]),
                    "event": datavalue_to_string(&r[1]),
                }))
                .collect::<Vec<_>>(),
            "affected_services": services.rows.iter()
                .map(|r| serde_json::json!({
                    "context": datavalue_to_string(&r[0]),
                    "service": datavalue_to_string(&r[1]),
                }))
                .collect::<Vec<_>>(),
            "dependent_contexts": dependents.rows.iter()
                .map(|r| datavalue_to_string(&r[0]))
                .collect::<Vec<_>>(),
        }))
    }

    /// Find aggregate roots that lack invariants (potential quality issue).
    pub fn aggregate_roots_without_invariants(
        &self,
        workspace: &str,
    ) -> Result<Vec<(String, String)>> {
        let params = params_map(&[("ws", workspace)]);
        let result = self
            .db
            .run_script(
                "has_invariant[ctx, ent] := *invariant{workspace: $ws, context: ctx, entity: ent} \
                 ?[context, entity] := \
                    *entity{workspace: $ws, context, name: entity, aggregate_root: true}, \
                    not has_invariant[context, entity]",
                params,
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("Aggregate roots query failed: {:?}", e))?;

        Ok(result
            .rows
            .iter()
            .map(|r| (datavalue_to_string(&r[0]), datavalue_to_string(&r[1])))
            .collect())
    }

    /// Get a full graph summary: all contexts and their dependencies.
    pub fn dependency_graph(&self, workspace: &str) -> Result<serde_json::Value> {
        let params = params_map(&[("ws", workspace)]);

        let contexts = self
            .db
            .run_script(
                "?[name, module_path] := *context{workspace: $ws, name, module_path}",
                params.clone(),
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("Context query failed: {:?}", e))?;

        let deps = self
            .db
            .run_script(
                "?[from_ctx, to_ctx] := *context_dep{workspace: $ws, from_ctx, to_ctx}",
                params.clone(),
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("Dep query failed: {:?}", e))?;

        let circular = self.circular_deps(workspace)?;

        Ok(serde_json::json!({
            "nodes": contexts.rows.iter()
                .map(|r| serde_json::json!({
                    "name": datavalue_to_string(&r[0]),
                    "module_path": datavalue_to_string(&r[1]),
                }))
                .collect::<Vec<_>>(),
            "edges": deps.rows.iter()
                .map(|r| serde_json::json!({
                    "from": datavalue_to_string(&r[0]),
                    "to": datavalue_to_string(&r[1]),
                }))
                .collect::<Vec<_>>(),
            "circular_dependencies": circular.iter()
                .map(|(a, b)| serde_json::json!({"a": a, "b": b}))
                .collect::<Vec<_>>(),
        }))
    }

    // ─── Metalayer: Inference-driven Model Health ───────────────────────────

    /// Compute a comprehensive model health report using Datalog inference.
    /// This aggregates multiple analysis queries into a single metalayer payload
    /// that can be injected into prompts and resources to influence all AI interactions.
    pub fn model_health(&self, workspace: &str) -> Result<ModelHealth> {
        let canonical = canonicalize_path(workspace);

        // 1. Circular dependencies (critical architectural issue)
        let circular = self.circular_deps(&canonical).unwrap_or_default();

        // 2. Layer violations (domain depending on infra)
        let violations = self.layer_violations(&canonical).unwrap_or_default();

        // 3. Aggregate roots without invariants (DDD quality)
        let missing_invariants = self
            .aggregate_roots_without_invariants(&canonical)
            .unwrap_or_default();

        // 4. Orphan contexts (no incoming or outgoing deps)
        let orphans = self.orphan_contexts(&canonical).unwrap_or_default();

        // 5. Entity and service counts per context (complexity distribution)
        let complexity = self.context_complexity(&canonical).unwrap_or_default();

        // 6. God contexts (contexts with disproportionate entity/service count)
        let god_contexts: Vec<String> = complexity
            .iter()
            .filter(|c| c.entity_count + c.service_count > 10)
            .map(|c| c.context.clone())
            .collect();

        // 7. Events without source entities
        let unsourced_events = self.unsourced_events(&canonical).unwrap_or_default();

        // Compute overall health score (0-100)
        let critical_issues = circular.len() + violations.len();
        let warnings = missing_invariants.len() + god_contexts.len() + unsourced_events.len();
        let info = orphans.len();
        let score = (100i32
            - (critical_issues as i32 * 20)
            - (warnings as i32 * 5)
            - (info as i32 * 2))
        .max(0) as u32;

        Ok(ModelHealth {
            score,
            circular_deps: circular
                .into_iter()
                .map(|(a, b)| [a, b])
                .collect(),
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

    /// Find bounded contexts with no incoming or outgoing dependencies.
    fn orphan_contexts(&self, workspace: &str) -> Result<Vec<String>> {
        let params = params_map(&[("ws", workspace)]);
        let result = self
            .db
            .run_script(
                "has_dep[ctx] := *context_dep{workspace: $ws, from_ctx: ctx} \
                 has_dep[ctx] := *context_dep{workspace: $ws, to_ctx: ctx} \
                 ?[name] := *context{workspace: $ws, name}, not has_dep[name]",
                params,
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("Orphan context query failed: {:?}", e))?;

        Ok(result
            .rows
            .iter()
            .map(|r| datavalue_to_string(&r[0]))
            .collect())
    }

    /// Compute entity/service/event counts per bounded context.
    fn context_complexity(&self, workspace: &str) -> Result<Vec<ContextComplexity>> {
        let params = params_map(&[("ws", workspace)]);

        // Get contexts with entity counts
        let result = self
            .db
            .run_script(
                "ent_count[ctx, count(ent)] := *entity{workspace: $ws, context: ctx, name: ent} \
                 ent_count[ctx, 0] := *context{workspace: $ws, name: ctx}, not *entity{workspace: $ws, context: ctx} \
                 svc_count[ctx, count(svc)] := *service{workspace: $ws, context: ctx, name: svc} \
                 svc_count[ctx, 0] := *context{workspace: $ws, name: ctx}, not *service{workspace: $ws, context: ctx} \
                 evt_count[ctx, count(evt)] := *event{workspace: $ws, context: ctx, name: evt} \
                 evt_count[ctx, 0] := *context{workspace: $ws, name: ctx}, not *event{workspace: $ws, context: ctx} \
                 dep_count[ctx, count(dep)] := *context_dep{workspace: $ws, from_ctx: ctx, to_ctx: dep} \
                 dep_count[ctx, 0] := *context{workspace: $ws, name: ctx}, not *context_dep{workspace: $ws, from_ctx: ctx} \
                 ?[ctx, ents, svcs, evts, deps] := ent_count[ctx, ents], svc_count[ctx, svcs], evt_count[ctx, evts], dep_count[ctx, deps]",
                params,
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("Complexity query failed: {:?}", e))?;

        Ok(result
            .rows
            .iter()
            .map(|r| ContextComplexity {
                context: datavalue_to_string(&r[0]),
                entity_count: datavalue_to_u32(&r[1]),
                service_count: datavalue_to_u32(&r[2]),
                event_count: datavalue_to_u32(&r[3]),
                dep_count: datavalue_to_u32(&r[4]),
            })
            .collect())
    }

    /// Find events that have no source entity set.
    fn unsourced_events(&self, workspace: &str) -> Result<Vec<[String; 2]>> {
        let params = params_map(&[("ws", workspace)]);
        let result = self
            .db
            .run_script(
                "?[context, name] := *event{workspace: $ws, context, name, source: ''}",
                params,
                ScriptMutability::Immutable,
            )
            .map_err(|e| anyhow::anyhow!("Unsourced events query failed: {:?}", e))?;

        Ok(result
            .rows
            .iter()
            .map(|r| {
                [
                    datavalue_to_string(&r[0]),
                    datavalue_to_string(&r[1]),
                ]
            })
            .collect())
    }
}

/// Metadata about a stored project.
#[derive(Debug, Clone)]
pub struct ProjectInfo {
    pub workspace_path: String,
    pub project_name: String,
    pub updated_at: String,
}

/// Comprehensive model health report computed via Datalog inference.
/// This is the metalayer payload that drives all prompt/resource enrichment.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelHealth {
    /// Overall health score (0–100). Deducted by issues.
    pub score: u32,
    /// Circular dependency pairs [from, to] — critical.
    pub circular_deps: Vec<[String; 2]>,
    /// Domain services depending on infrastructure — critical.
    pub layer_violations: Vec<LayerViolation>,
    /// Aggregate roots with no invariants defined [context, entity] — warning.
    pub missing_invariants: Vec<[String; 2]>,
    /// Bounded contexts with zero incoming or outgoing dependencies — info.
    pub orphan_contexts: Vec<String>,
    /// Bounded contexts with >10 entities+services — warning.
    pub god_contexts: Vec<String>,
    /// Events with empty source [context, event] — warning.
    pub unsourced_events: Vec<[String; 2]>,
    /// Per-context complexity breakdown.
    pub complexity: Vec<ContextComplexity>,
}

/// Layer violation: a domain service depending on an infrastructure service.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LayerViolation {
    pub context: String,
    pub domain_service: String,
    pub infra_dependency: String,
}

/// Complexity metrics for a single bounded context.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ContextComplexity {
    pub context: String,
    pub entity_count: u32,
    pub service_count: u32,
    pub event_count: u32,
    pub dep_count: u32,
}

// ─── Helpers ───────────────────────────────────────────────────────────────

/// Returns the default database path: `~/.dendrites/dendrites.db`
fn default_db_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Could not determine home directory")?;
    Ok(home.join(".dendrites").join("dendrites.db"))
}

/// Normalize workspace path for consistent keying.
pub fn canonicalize_path(path: &str) -> String {
    let normalized = path.trim_end_matches('/');
    match std::fs::canonicalize(normalized) {
        Ok(p) => p.to_string_lossy().to_string(),
        Err(_) => normalized.to_string(),
    }
}

/// Build a CozoDB parameter map from string key-value pairs.
fn params_map(pairs: &[(&str, &str)]) -> BTreeMap<String, cozo::DataValue> {
    let mut map = BTreeMap::new();
    for (k, v) in pairs {
        map.insert(k.to_string(), cozo::DataValue::Str(v.to_string().into()));
    }
    map
}

/// Extract a string from a NamedRows cell.
fn row_string(row: &[cozo::DataValue], idx: usize) -> Result<String> {
    match &row[idx] {
        cozo::DataValue::Str(s) => Ok(s.to_string()),
        other => Ok(datavalue_to_string(other)),
    }
}

/// Convert a DataValue to a display string.
fn datavalue_to_string(val: &cozo::DataValue) -> String {
    match val {
        cozo::DataValue::Null => "null".to_string(),
        cozo::DataValue::Bool(b) => b.to_string(),
        cozo::DataValue::Num(n) => match n {
            cozo::Num::Int(i) => i.to_string(),
            cozo::Num::Float(f) => f.to_string(),
        },
        cozo::DataValue::Str(s) => s.to_string(),
        cozo::DataValue::List(l) => {
            let items: Vec<String> = l.iter().map(datavalue_to_string).collect();
            format!("[{}]", items.join(", "))
        }
        _ => format!("{:?}", val),
    }
}

/// Convert a DataValue to u32 (for aggregate counts).
fn datavalue_to_u32(val: &cozo::DataValue) -> u32 {
    match val {
        cozo::DataValue::Num(cozo::Num::Int(i)) => *i as u32,
        cozo::DataValue::Num(cozo::Num::Float(f)) => *f as u32,
        _ => 0,
    }
}

/// Get current timestamp as ISO string.
fn chrono_now() -> String {
    // Simple UTC timestamp without chrono dependency
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Format as basic datetime
    let secs_per_day = 86400u64;
    let days = now / secs_per_day;
    let rem = now % secs_per_day;
    let hours = rem / 3600;
    let minutes = (rem % 3600) / 60;
    let seconds = rem % 60;
    // Days since 1970-01-01
    let (year, month, day) = days_to_date(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

fn days_to_date(days: u64) -> (u64, u64, u64) {
    // Simplified date calculation
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
    let months_days: Vec<u64> = if is_leap(y) {
        vec![31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        vec![31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut m = 1;
    for md in &months_days {
        if remaining < *md {
            break;
        }
        remaining -= *md;
        m += 1;
    }
    (y, m, remaining + 1)
}

fn is_leap(y: u64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::*;
    use std::env::temp_dir;

    fn test_model(name: &str) -> DomainModel {
        DomainModel {
            name: name.to_string(),
            description: "Test project".into(),
            bounded_contexts: vec![],
            rules: vec![],
            tech_stack: TechStack::default(),
            conventions: Conventions::default(),
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

    fn full_model() -> DomainModel {
        DomainModel {
            name: "TestProject".into(),
            description: "Test".into(),
            bounded_contexts: vec![
                BoundedContext {
                    name: "Identity".into(),
                    description: "Auth context".into(),
                    module_path: "src/identity".into(),
                    entities: vec![Entity {
                        name: "User".into(),
                        description: "A user".into(),
                        aggregate_root: true,
                        fields: vec![Field {
                            name: "id".into(),
                            field_type: "UserId".into(),
                            required: true,
                            description: "Unique ID".into(),
                        }],
                        methods: vec![Method {
                            name: "register".into(),
                            description: "Register user".into(),
                            parameters: vec![Field {
                                name: "email".into(),
                                field_type: "Email".into(),
                                required: true,
                                description: "".into(),
                            }],
                            return_type: "Result<User>".into(),
                        }],
                        invariants: vec!["Email must be unique".into()],
                    }],
                    value_objects: vec![ValueObject {
                        name: "Email".into(),
                        description: "Validated email".into(),
                        fields: vec![],
                        validation_rules: vec![],
                    }],
                    services: vec![Service {
                        name: "AuthService".into(),
                        description: "Handles auth".into(),
                        kind: ServiceKind::Application,
                        methods: vec![],
                        dependencies: vec!["UserRepository".into()],
                    }],
                    repositories: vec![Repository {
                        name: "UserRepository".into(),
                        aggregate: "User".into(),
                        methods: vec![],
                    }],
                    events: vec![DomainEvent {
                        name: "UserRegistered".into(),
                        description: "Emitted on registration".into(),
                        fields: vec![Field {
                            name: "user_id".into(),
                            field_type: "UserId".into(),
                            required: true,
                            description: "".into(),
                        }],
                        source: "User".into(),
                    }],
                    dependencies: vec![],
                },
                BoundedContext {
                    name: "Billing".into(),
                    description: "Billing context".into(),
                    module_path: "src/billing".into(),
                    entities: vec![Entity {
                        name: "Subscription".into(),
                        description: "A subscription".into(),
                        aggregate_root: true,
                        fields: vec![],
                        methods: vec![],
                        invariants: vec![],  // No invariants — should be flagged
                    }],
                    value_objects: vec![],
                    services: vec![],
                    repositories: vec![],
                    events: vec![],
                    dependencies: vec!["Identity".into()],
                },
            ],
            rules: vec![ArchitecturalRule {
                id: "LAYER-001".into(),
                description: "Domain must not depend on infra".into(),
                severity: Severity::Error,
                scope: "domain".into(),
            }],
            tech_stack: TechStack::default(),
            conventions: Conventions::default(),
        }
    }

    #[test]
    fn test_save_and_load() {
        let store = temp_store();
        let model = test_model("TestProject");
        store.save_desired("/tmp/my-project", &model).unwrap();

        let loaded = store.load_desired("/tmp/my-project").unwrap().unwrap();
        assert_eq!(loaded.name, "TestProject");
    }

    #[test]
    fn test_load_nonexistent() {
        let store = temp_store();
        let result = store.load_desired("/tmp/does-not-exist").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_list_projects() {
        let store = temp_store();
        store
            .save_desired("/tmp/proj-a", &test_model("ProjectA"))
            .unwrap();
        store
            .save_desired("/tmp/proj-b", &test_model("ProjectB"))
            .unwrap();

        let projects = store.list().unwrap();
        assert_eq!(projects.len(), 2);
    }

    #[test]
    fn test_upsert() {
        let store = temp_store();
        store
            .save_desired("/tmp/my-project", &test_model("V1"))
            .unwrap();
        store
            .save_desired("/tmp/my-project", &test_model("V2"))
            .unwrap();

        let projects = store.list().unwrap();
        assert_eq!(projects.len(), 1);

        let loaded = store.load_desired("/tmp/my-project").unwrap().unwrap();
        assert_eq!(loaded.name, "V2");
    }

    #[test]
    fn test_accept_and_load_actual() {
        let store = temp_store();
        let model = full_model();
        store.save_desired("/tmp/test-accept", &model).unwrap();
        store.accept("/tmp/test-accept").unwrap();

        let actual = store.load_actual("/tmp/test-accept").unwrap().unwrap();
        assert_eq!(actual.name, "TestProject");
    }

    #[test]
    fn test_reset() {
        let store = temp_store();
        let model = full_model();
        store.save_desired("/tmp/test-reset", &model).unwrap();
        store.accept("/tmp/test-reset").unwrap();

        // Modify desired
        let mut modified = model.clone();
        modified.name = "Modified".into();
        store.save_desired("/tmp/test-reset", &modified).unwrap();

        // Verify modification
        let loaded = store.load_desired("/tmp/test-reset").unwrap().unwrap();
        assert_eq!(loaded.name, "Modified");

        // Reset
        let reset = store.reset("/tmp/test-reset").unwrap().unwrap();
        assert_eq!(reset.name, "TestProject");
    }

    // ─── Datalog Inference Tests ───────────────────────────────────────

    #[test]
    fn test_transitive_deps() {
        let store = temp_store();
        let mut model = full_model();
        // Add a third context: Notifications depends on Billing
        model.bounded_contexts.push(BoundedContext {
            name: "Notifications".into(),
            description: "".into(),
            module_path: "src/notifications".into(),
            entities: vec![],
            value_objects: vec![],
            services: vec![],
            repositories: vec![],
            events: vec![],
            dependencies: vec!["Billing".into()],
        });
        store.save_desired("/tmp/test-trans", &model).unwrap();

        // Notifications → Billing → Identity (transitively)
        let deps = store.transitive_deps("/tmp/test-trans", "Notifications").unwrap();
        assert!(deps.contains(&"Billing".to_string()));
        assert!(deps.contains(&"Identity".to_string()));
    }

    #[test]
    fn test_circular_deps() {
        let store = temp_store();
        let mut model = full_model();
        // Create circular: Identity → Billing (already Billing → Identity)
        model.bounded_contexts[0]
            .dependencies
            .push("Billing".into());
        store.save_desired("/tmp/test-circular", &model).unwrap();

        let cycles = store.circular_deps("/tmp/test-circular").unwrap();
        assert!(!cycles.is_empty());
    }

    #[test]
    fn test_no_circular_deps() {
        let store = temp_store();
        let model = full_model();
        store.save_desired("/tmp/test-no-circular", &model).unwrap();

        let cycles = store.circular_deps("/tmp/test-no-circular").unwrap();
        assert!(cycles.is_empty());
    }

    #[test]
    fn test_aggregate_roots_without_invariants() {
        let store = temp_store();
        let model = full_model();
        store.save_desired("/tmp/test-agg", &model).unwrap();

        // Subscription is an aggregate root with no invariants
        let missing = store
            .aggregate_roots_without_invariants("/tmp/test-agg")
            .unwrap();
        assert!(missing
            .iter()
            .any(|(_, e)| e == "Subscription"));
        // User has invariants, should NOT appear
        assert!(!missing.iter().any(|(_, e)| e == "User"));
    }

    #[test]
    fn test_impact_analysis() {
        let store = temp_store();
        let model = full_model();
        store.save_desired("/tmp/test-impact", &model).unwrap();

        let impact = store
            .impact_analysis("/tmp/test-impact", "Identity", "User")
            .unwrap();

        // UserRegistered event is sourced from User
        let events = impact["affected_events"].as_array().unwrap();
        assert!(events
            .iter()
            .any(|e| e["event"] == "UserRegistered"));

        // Billing depends on Identity
        let dependents = impact["dependent_contexts"].as_array().unwrap();
        assert!(dependents
            .iter()
            .any(|d| d.as_str() == Some("Billing")));
    }

    #[test]
    fn test_dependency_graph() {
        let store = temp_store();
        let model = full_model();
        store.save_desired("/tmp/test-graph", &model).unwrap();

        let graph = store.dependency_graph("/tmp/test-graph").unwrap();
        let nodes = graph["nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), 2);
        let edges = graph["edges"].as_array().unwrap();
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
                "?[name, aggregate_root] := *entity{workspace: $ws, name, aggregate_root}",
                "/tmp/test-raw",
            )
            .unwrap();
        assert_eq!(rows.len(), 2); // User + Subscription
    }
}
