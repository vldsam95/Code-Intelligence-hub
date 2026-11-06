use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use codeintel_core::{absence_result, FileInventoryEntry, SourceRevision};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphNode {
    pub id: String,
    pub kind: String,
    pub qualified_name: String,
    pub display_name: String,
    pub file_path: String,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub signature: Option<String>,
    pub language: String,
    pub exported: bool,
    pub entry_point: bool,
    /// Deterministic, source-derived identity inputs. These are retained with the node so a
    /// collision can be diagnosed without reading source bodies or relying on log archaeology.
    #[serde(default)]
    pub logical_root: String,
    #[serde(default)]
    pub canonical_identity: String,
    #[serde(default)]
    pub provenance: Vec<NodeProvenance>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeProvenance {
    pub origin: String,
    pub producer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphEdge {
    pub id: String,
    pub source_id: String,
    pub target_id: String,
    pub kind: String,
    pub origin: String,
    pub confidence: f64,
    pub source_file: String,
    pub source_range: String,
    pub source_revision_id: String,
    pub analyzer: String,
    pub analyzer_version: String,
    pub ambiguity_reason: Option<String>,
    pub last_seen_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeEdgeObservation {
    pub source_symbol: String,
    pub target_symbol: String,
    pub kind: String,
    pub count: u64,
}

pub struct RuntimeIngest<'a> {
    pub runtime_run_id: &'a str,
    pub source_revision_id: &'a str,
    pub workload: &'a str,
    pub observed_at: &'a str,
    pub coverage_scope: &'a str,
    pub adapter: &'a str,
    pub adapter_version: &'a str,
    pub edges: &'a [RuntimeEdgeObservation],
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Coverage {
    pub supported_languages: Vec<String>,
    pub unsupported_languages: Vec<String>,
    pub parsed_files: usize,
    pub failed_files: Vec<String>,
    pub semantic: String,
    pub framework: String,
    #[serde(default)]
    pub semantic_resolved_calls: usize,
    #[serde(default)]
    pub semantic_ambiguous_calls: usize,
    #[serde(default)]
    pub semantic_unresolved_calls: usize,
    #[serde(default)]
    pub semantic_external_or_unindexed_calls: usize,
    #[serde(default)]
    pub semantic_unresolved_files: Vec<String>,
    #[serde(default)]
    pub semantic_external_or_unindexed_files: Vec<String>,
    #[serde(default)]
    pub semantic_ambiguous_files: Vec<String>,
    #[serde(default)]
    pub dynamic_boundaries: Vec<String>,
    #[serde(default)]
    pub semantic_unknowns: Vec<String>,
    #[serde(default)]
    pub framework_adapters: Vec<String>,
    #[serde(default)]
    pub framework_unknowns: Vec<String>,
}

impl Coverage {
    pub fn absence_scope_complete(&self) -> bool {
        self.unsupported_languages.is_empty()
            && self.failed_files.is_empty()
            && self.semantic == "COMPLETE"
            && self.framework == "COMPLETE"
            && self.semantic_unresolved_files.is_empty()
            && self.semantic_external_or_unindexed_files.is_empty()
            && self.semantic_ambiguous_files.is_empty()
            && self.framework_unknowns.is_empty()
            && self.semantic_unknowns.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResponse<T> {
    pub summary: String,
    pub items: Vec<T>,
    pub coverage: Coverage,
    pub absence_proven: bool,
    pub unknowns: Vec<String>,
    pub context_handle: Option<String>,
    pub items_remaining: usize,
}

pub struct GraphBuilder {
    path: PathBuf,
    connection: Connection,
}

impl GraphBuilder {
    pub fn create(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        configure(&connection)?;
        create_schema(&connection)?;
        Ok(Self {
            path: path.to_path_buf(),
            connection,
        })
    }

    pub fn begin(&mut self) -> Result<GraphTransaction<'_>> {
        Ok(GraphTransaction {
            tx: self.connection.transaction()?,
        })
    }

    pub fn validate(self, revision: &SourceRevision, coverage: &Coverage) -> Result<PathBuf> {
        self.connection.execute(
            "INSERT OR REPLACE INTO index_metadata(key, value) VALUES (?1, ?2)",
            params!["source_revision_id", revision.source_revision_id],
        )?;
        self.connection.execute(
            "INSERT OR REPLACE INTO index_metadata(key, value) VALUES (?1, ?2)",
            params!["coverage", serde_json::to_string(coverage)?],
        )?;
        self.connection.execute(
            "INSERT OR REPLACE INTO index_metadata(key, value) VALUES (?1, ?2)",
            params!["state", "VALIDATING"],
        )?;
        let integrity: String = self
            .connection
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        if integrity != "ok" {
            self.connection.execute(
                "INSERT OR REPLACE INTO index_metadata(key, value) VALUES ('state', 'INVALID')",
                [],
            )?;
            return Err(anyhow!("Graph integrity check failed: {integrity}"));
        }
        self.connection.execute(
            "INSERT OR REPLACE INTO index_metadata(key, value) VALUES ('state', 'CURRENT')",
            [],
        )?;
        self.connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        drop(self.connection);
        Ok(self.path)
    }
}

pub struct GraphTransaction<'a> {
    tx: Transaction<'a>,
}

impl GraphTransaction<'_> {
    pub fn add_inventory(&self, entry: &FileInventoryEntry) -> Result<()> {
        self.tx.execute(
            "INSERT INTO file_inventory(logical_path,size,mode,content_sha256) VALUES (?1,?2,?3,?4)",
            params![entry.logical_path, entry.size, entry.mode, entry.content_sha256],
        )?;
        Ok(())
    }

    pub fn add_file_content(&self, path: &str, language: &str, content: &str) -> Result<()> {
        self.tx.execute(
            "INSERT INTO source_fts(path,language,content) VALUES (?1,?2,?3)",
            params![path, language, content],
        )?;
        Ok(())
    }

    pub fn add_node(&self, node: &GraphNode) -> Result<()> {
        let inserted = self.tx.execute(
            "INSERT INTO nodes(id,kind,qualified_name,display_name,file_path,start_line,start_column,end_line,end_column,signature,language,exported,entry_point,logical_root,canonical_identity) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            params![
                node.id,
                node.kind,
                node.qualified_name,
                node.display_name,
                node.file_path,
                node.start_line,
                node.start_column,
                node.end_line,
                node.end_column,
                node.signature,
                node.language,
                node.exported,
                node.entry_point,
                node.logical_root,
                node.canonical_identity,
            ],
        );
        if let Err(error) = inserted {
            let existing = self
                .tx
                .query_row(
                    "SELECT id,kind,qualified_name,display_name,file_path,start_line,start_column,end_line,end_column,signature,language,exported,entry_point,logical_root,canonical_identity FROM nodes WHERE id=?1",
                    params![node.id],
                    node_from_row,
                )
                .optional()?;
            let existing = existing.ok_or_else(|| {
                anyhow!(
                    "node insert failed but the colliding node could not be read: node_id={}",
                    node.id
                )
            })?;
            // A parser, semantic resolver or framework adapter may describe the same logical
            // entity.  That is an explicit provenance merge only when the full, versioned
            // canonical identity agrees; every other primary-key collision is a hard error.
            if existing.canonical_identity == node.canonical_identity {
                self.add_node_provenance(node)?;
                return Ok(());
            }
            return Err(anyhow!(
                "NODE_ID_COLLISION: node_id={} existing_candidate={} incoming_candidate={}",
                node.id,
                serde_json::to_string(&existing)?,
                serde_json::to_string(node)?,
            ))
            .context(error.to_string());
        }
        self.add_node_provenance(node)?;
        Ok(())
    }

    fn add_node_provenance(&self, node: &GraphNode) -> Result<()> {
        for provenance in &node.provenance {
            let present: Option<i64> = self.tx.query_row(
                "SELECT 1 FROM node_provenance WHERE node_id=?1 AND origin=?2 AND producer=?3 AND canonical_identity=?4",
                params![node.id, provenance.origin, provenance.producer, node.canonical_identity],
                |row| row.get(0),
            ).optional()?;
            if present.is_none() {
                self.tx.execute(
                "INSERT INTO node_provenance(node_id,origin,producer,canonical_identity) VALUES (?1,?2,?3,?4)",
                params![node.id, provenance.origin, provenance.producer, node.canonical_identity],
            )?;
            }
        }
        Ok(())
    }

    pub fn add_edge(&self, edge: &GraphEdge) -> Result<()> {
        if edge.origin.contains("HEURISTIC") && (edge.confidence - 1.0).abs() < f64::EPSILON {
            return Err(anyhow!("Heuristic edge cannot have exact confidence"));
        }
        self.tx.execute(
            "INSERT INTO edges(id,source_id,target_id,kind,origin,confidence,source_file,source_range,source_revision_id,analyzer,analyzer_version,ambiguity_reason,last_seen_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![
                edge.id,
                edge.source_id,
                edge.target_id,
                edge.kind,
                edge.origin,
                edge.confidence,
                edge.source_file,
                edge.source_range,
                edge.source_revision_id,
                edge.analyzer,
                edge.analyzer_version,
                edge.ambiguity_reason,
                edge.last_seen_at,
            ],
        )?;
        Ok(())
    }

    pub fn commit(self) -> Result<()> {
        self.tx.commit()?;
        Ok(())
    }
}

pub fn atomic_publish(staging_path: &Path, current_path: &Path) -> Result<()> {
    let parent = current_path
        .parent()
        .ok_or_else(|| anyhow!("graph path has no parent"))?;
    fs::create_dir_all(parent)?;
    let previous = current_path.with_extension("sqlite3.previous");
    if current_path.exists() {
        if previous.exists() {
            fs::remove_file(&previous).context("remove old derived previous graph")?;
        }
        fs::rename(current_path, &previous).context("preserve previous graph snapshot")?;
    }
    if let Err(error) = fs::rename(staging_path, current_path) {
        if previous.exists() && !current_path.exists() {
            let _ = fs::rename(&previous, current_path);
        }
        return Err(error).context("publish staged graph atomically");
    }
    Ok(())
}

pub struct GraphStore {
    connection: Connection,
}

impl GraphStore {
    pub fn open(path: &Path) -> Result<Self> {
        let connection = Connection::open(path)?;
        configure(&connection)?;
        let state: Option<String> = connection
            .query_row(
                "SELECT value FROM index_metadata WHERE key='state'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if state.as_deref() != Some("CURRENT") {
            return Err(anyhow!("Graph is not CURRENT (state={state:?})"));
        }
        Ok(Self { connection })
    }

    pub fn revision(&self) -> Result<String> {
        self.connection
            .query_row(
                "SELECT value FROM index_metadata WHERE key='source_revision_id'",
                [],
                |row| row.get(0),
            )
            .context("graph revision metadata missing")
    }

    pub fn coverage(&self) -> Result<Coverage> {
        let raw: String = self.connection.query_row(
            "SELECT value FROM index_metadata WHERE key='coverage'",
            [],
            |row| row.get(0),
        )?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub fn find_symbol(&self, query: &str, limit: usize) -> Result<QueryResponse<GraphNode>> {
        let limit = limit.clamp(1, 100);
        let like = format!("%{query}%");
        let mut statement = self.connection.prepare(
            "SELECT id,kind,qualified_name,display_name,file_path,start_line,start_column,end_line,end_column,signature,language,exported,entry_point,logical_root,canonical_identity FROM nodes WHERE qualified_name=?1 OR display_name=?1 OR qualified_name LIKE ?2 ORDER BY CASE WHEN qualified_name=?1 THEN 0 WHEN display_name=?1 THEN 1 ELSE 2 END, qualified_name LIMIT ?3",
        )?;
        let items = statement
            .query_map(params![query, like, limit as i64], node_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let coverage = self.coverage()?;
        let absence = absence_result(items.len(), coverage.absence_scope_complete());
        Ok(QueryResponse {
            summary: format!("Found {} matching symbols.", items.len()),
            absence_proven: absence["absence_proven"].as_bool().unwrap_or(false),
            unknowns: if items.is_empty() && !coverage.absence_scope_complete() {
                vec![
                    "No symbol result is not proof of absence; analysis coverage is partial."
                        .into(),
                ]
            } else {
                vec![]
            },
            coverage,
            items,
            context_handle: None,
            items_remaining: 0,
        })
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<QueryResponse<serde_json::Value>> {
        let limit = limit.clamp(1, 50);
        let safe_query = natural_fts_query(query);
        let items = if safe_query.is_empty() {
            vec![]
        } else {
            let mut statement = self.connection.prepare(
                "SELECT path,language,snippet(source_fts,2,'[',']',' … ',12),rank FROM source_fts WHERE source_fts MATCH ?1 ORDER BY rank LIMIT ?2",
            )?;
            let rows = statement
                .query_map(params![safe_query, limit as i64], |row| {
                    Ok(serde_json::json!({
                        "path": row.get::<_, String>(0)?,
                        "language": row.get::<_, String>(1)?,
                        "snippet": row.get::<_, String>(2)?,
                        "rank": row.get::<_, f64>(3)?,
                    }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        let coverage = self.coverage()?;
        Ok(QueryResponse {
            summary: format!("Found {} FTS matches.", items.len()),
            absence_proven: items.is_empty() && coverage.absence_scope_complete(),
            unknowns: if items.is_empty() && !coverage.absence_scope_complete() {
                vec!["Search coverage is partial.".into()]
            } else {
                vec![]
            },
            coverage,
            items,
            context_handle: None,
            items_remaining: 0,
        })
    }

    pub fn graph_query(
        &self,
        kind: Option<&str>,
        file_path: Option<&str>,
        limit: usize,
    ) -> Result<QueryResponse<GraphNode>> {
        let limit = limit.clamp(1, 100);
        let mut statement = self.connection.prepare(
            "SELECT id,kind,qualified_name,display_name,file_path,start_line,start_column,end_line,end_column,signature,language,exported,entry_point,logical_root,canonical_identity
             FROM nodes
             WHERE (?1 IS NULL OR kind = ?1) AND (?2 IS NULL OR file_path = ?2)
             ORDER BY file_path,start_line,qualified_name LIMIT ?3",
        )?;
        let items = statement
            .query_map(params![kind, file_path, limit as i64], node_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let coverage = self.coverage()?;
        Ok(QueryResponse {
            summary: format!("Found {} bounded graph nodes.", items.len()),
            absence_proven: items.is_empty() && coverage.absence_scope_complete(),
            unknowns: if items.is_empty() && !coverage.absence_scope_complete() {
                vec!["An empty bounded graph query is not proof of application absence.".into()]
            } else {
                vec![]
            },
            coverage,
            items,
            context_handle: None,
            items_remaining: 0,
        })
    }

    /// Build a bounded, ephemeral visualization payload. It contains symbol metadata and
    /// provenance-bearing relationships only: never source bodies and never an unbounded dump.
    pub fn visualization(&self, max_nodes: usize, max_edges: usize) -> Result<serde_json::Value> {
        let max_nodes = max_nodes.clamp(20, 300);
        let max_edges = max_edges.clamp(20, 600);
        let total_nodes: usize =
            self.connection
                .query_row("SELECT COUNT(*) FROM nodes", [], |row| row.get(0))?;
        let total_edges: usize =
            self.connection
                .query_row("SELECT COUNT(*) FROM edges", [], |row| row.get(0))?;
        let mut statement = self.connection.prepare(
            "WITH degree_counts AS (
               SELECT node_id, COUNT(*) AS degree FROM (
                 SELECT source_id AS node_id FROM edges
                 UNION ALL
                 SELECT target_id AS node_id FROM edges
               ) GROUP BY node_id
             )
             SELECT n.id,n.kind,n.qualified_name,n.display_name,n.file_path,n.language,
                    n.entry_point,n.exported,n.start_line
             FROM nodes n LEFT JOIN degree_counts d ON d.node_id=n.id
             ORDER BY n.entry_point DESC, COALESCE(d.degree, 0) DESC, n.exported DESC,
               CASE n.kind WHEN 'ROUTE' THEN 0 WHEN 'ENDPOINT' THEN 1 WHEN 'CLASS' THEN 2
                 WHEN 'FUNCTION' THEN 3 WHEN 'METHOD' THEN 4 ELSE 5 END,
               n.file_path,n.start_line
             LIMIT ?1",
        )?;
        let nodes = statement
            .query_map(params![max_nodes as i64], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "kind": row.get::<_, String>(1)?,
                    "qualified_name": row.get::<_, String>(2)?,
                    "label": row.get::<_, String>(3)?,
                    "path": row.get::<_, String>(4)?,
                    "language": row.get::<_, String>(5)?,
                    "entry_point": row.get::<_, bool>(6)?,
                    "exported": row.get::<_, bool>(7)?,
                    "line": row.get::<_, u32>(8)?,
                }))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let selected: HashSet<String> = nodes
            .iter()
            .filter_map(|node| node.get("id").and_then(serde_json::Value::as_str))
            .map(str::to_string)
            .collect();
        let mut edge_statement = self.connection.prepare(
            "SELECT source_id,target_id,kind,origin,confidence,ambiguity_reason
             FROM edges ORDER BY confidence DESC,last_seen_at DESC LIMIT 5000",
        )?;
        let mut edges = Vec::with_capacity(max_edges);
        for row in edge_statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, f64>(4)?,
                row.get::<_, Option<String>>(5)?,
            ))
        })? {
            let (source, target, kind, origin, confidence, ambiguity_reason) = row?;
            if selected.contains(&source) && selected.contains(&target) {
                edges.push(serde_json::json!({
                    "source": source,
                    "target": target,
                    "kind": kind,
                    "origin": origin,
                    "confidence": confidence,
                    "ambiguity_reason": ambiguity_reason,
                }));
                if edges.len() == max_edges {
                    break;
                }
            }
        }
        Ok(serde_json::json!({
            "nodes": nodes,
            "edges": edges,
            "node_count_total": total_nodes,
            "edge_count_total": total_edges,
            "nodes_remaining": total_nodes.saturating_sub(selected.len()),
            "edges_remaining": total_edges.saturating_sub(edges.len()),
            "bounded": true,
            "ephemeral": true,
            "source_bodies_included": false,
            "coverage": self.coverage()?,
            "warning": "This visualization is discovery evidence, not proof of correctness or absence."
        }))
    }

    /// Return a paginated endpoint inventory without source bodies or dynamic-expression details.
    pub fn endpoint_inventory(&self, page: usize, limit: usize) -> Result<serde_json::Value> {
        let limit = limit.clamp(1, 100);
        let page = page.max(1);
        let offset = (page - 1).saturating_mul(limit);
        let total: usize = self.connection.query_row(
            "SELECT COUNT(*) FROM nodes WHERE kind IN ('ROUTE','ENDPOINT')",
            [],
            |row| row.get(0),
        )?;
        let mut statement = self.connection.prepare(
            "SELECT id,display_name,file_path,start_line,language,kind
             FROM nodes WHERE kind IN ('ROUTE','ENDPOINT')
             ORDER BY file_path,start_line,id LIMIT ?1 OFFSET ?2",
        )?;
        let endpoints = statement
            .query_map(params![limit as i64, offset as i64], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "label": row.get::<_, String>(1)?,
                    "source_path": row.get::<_, String>(2)?,
                    "line": row.get::<_, u32>(3)?,
                    "language": row.get::<_, String>(4)?,
                    "kind": row.get::<_, String>(5)?,
                }))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let coverage = self.coverage()?;
        Ok(serde_json::json!({
            "page": page,
            "limit": limit,
            "total": total,
            "remaining": total.saturating_sub(offset.saturating_add(endpoints.len())),
            "endpoints": endpoints,
            "coverage": {
                "semantic": coverage.semantic,
                "framework": coverage.framework,
                "parsed_files": coverage.parsed_files,
                "failed_file_count": coverage.failed_files.len(),
                "unsupported_languages": coverage.unsupported_languages,
            },
            "source_bodies_included": false,
            "dynamic_expressions_included": false,
            "bounded": true,
        }))
    }

    /// Build a bounded, source-body-free route-to-callee view for correlating runtime security
    /// evidence with the indexed revision. Only practical control/data-flow edges are followed.
    pub fn bounded_flow_subgraph(
        &self,
        root_id: &str,
        max_depth: usize,
        max_nodes: usize,
    ) -> Result<serde_json::Value> {
        let max_depth = max_depth.clamp(1, 6);
        let max_nodes = max_nodes.clamp(1, 100);
        let root = self
            .connection
            .query_row(
                "SELECT id,kind,qualified_name,display_name,file_path,start_line,start_column,end_line,end_column,signature,language,exported,entry_point,logical_root,canonical_identity FROM nodes WHERE id=?1",
                params![root_id],
                node_from_row,
            )
            .optional()?;
        let Some(root) = root else {
            return Ok(serde_json::json!({
                "status": "UNKNOWN",
                "nodes": [],
                "edges": [],
                "unknowns": ["The requested root symbol is not present in the current graph."],
                "bounded": true,
                "source_bodies_included": false,
                "dynamic_expressions_included": false,
            }));
        };

        let mut selected = HashSet::from([root.id.clone()]);
        let mut queue = VecDeque::from([(root.id.clone(), 0_usize)]);
        let mut nodes = vec![compact_node(&root)];
        let mut edges = vec![];
        let mut edge_ids = HashSet::new();
        let revision = self.revision()?;
        while let Some((source_id, depth)) = queue.pop_front() {
            if depth >= max_depth || selected.len() >= max_nodes {
                continue;
            }
            let mut static_statement = self.connection.prepare(
                "SELECT n.id,n.kind,n.qualified_name,n.display_name,n.file_path,n.start_line,n.start_column,n.end_line,n.end_column,n.signature,n.language,n.exported,n.entry_point,n.logical_root,n.canonical_identity,e.id,e.kind,e.origin,e.confidence,e.ambiguity_reason
                 FROM edges e JOIN nodes n ON e.target_id=n.id
                 WHERE e.source_id=?1 AND e.kind IN (
                   'CALLS','ROUTES_TO','HTTP_CALLS','ASYNC_CALLS','DISPATCHES','LISTENS_TO',
                   'SCHEDULES','READS_FROM','WRITES_TO','FLOWS_TO'
                 ) ORDER BY e.confidence DESC,e.id LIMIT 100",
            )?;
            let static_rows = static_statement
                .query_map(params![source_id], |row| {
                    Ok((
                        node_from_row(row)?,
                        row.get::<_, String>(15)?,
                        row.get::<_, String>(16)?,
                        row.get::<_, String>(17)?,
                        row.get::<_, f64>(18)?,
                        row.get::<_, Option<String>>(19)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for (target, edge_id, kind, origin, confidence, ambiguity_reason) in static_rows {
                if selected.len() >= max_nodes && !selected.contains(&target.id) {
                    continue;
                }
                if edge_ids.insert(edge_id) {
                    edges.push(serde_json::json!({
                        "source": source_id,
                        "target": target.id,
                        "kind": kind,
                        "origin": origin,
                        "confidence": confidence,
                        "ambiguity_reason": ambiguity_reason,
                        "evidence": "static",
                    }));
                }
                if selected.insert(target.id.clone()) {
                    nodes.push(compact_node(&target));
                    queue.push_back((target.id, depth + 1));
                }
            }

            let mut runtime_statement = self.connection.prepare(
                "SELECT n.id,n.kind,n.qualified_name,n.display_name,n.file_path,n.start_line,n.start_column,n.end_line,n.end_column,n.signature,n.language,n.exported,n.entry_point,n.logical_root,n.canonical_identity,r.id,r.kind,r.count,r.adapter
                 FROM runtime_edges r JOIN nodes n ON r.target_symbol=n.id
                 WHERE r.source_symbol=?1 AND r.source_revision_id=?2 AND r.kind IN (
                   'RUNTIME_CALLS','HTTP_CALLS','ASYNC_CALLS','RUNTIME_READS_FROM',
                   'RUNTIME_WRITES_TO','RUNTIME_DISPATCHES'
                 ) ORDER BY r.count DESC,r.id LIMIT 100",
            )?;
            let runtime_rows = runtime_statement
                .query_map(params![source_id, revision], |row| {
                    Ok((
                        node_from_row(row)?,
                        row.get::<_, String>(15)?,
                        row.get::<_, String>(16)?,
                        row.get::<_, u64>(17)?,
                        row.get::<_, String>(18)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for (target, edge_id, kind, count, adapter) in runtime_rows {
                if selected.len() >= max_nodes && !selected.contains(&target.id) {
                    continue;
                }
                if edge_ids.insert(format!("runtime:{edge_id}")) {
                    edges.push(serde_json::json!({
                        "source": source_id,
                        "target": target.id,
                        "kind": kind,
                        "origin": adapter,
                        "count": count,
                        "evidence": "runtime",
                    }));
                }
                if selected.insert(target.id.clone()) {
                    nodes.push(compact_node(&target));
                    queue.push_back((target.id, depth + 1));
                }
            }
        }
        let coverage = self.coverage()?;
        let no_downstream = edges.is_empty();
        Ok(serde_json::json!({
            "status": "FOUND",
            "nodes": nodes,
            "edges": edges,
            "unknowns": if no_downstream {
                vec!["The route was found, but no bounded downstream flow was proven."]
            } else {
                Vec::<&str>::new()
            },
            "coverage": {
                "semantic": coverage.semantic,
                "framework": coverage.framework,
                "failed_file_count": coverage.failed_files.len(),
                "unsupported_languages": coverage.unsupported_languages,
            },
            "max_depth": max_depth,
            "max_nodes": max_nodes,
            "bounded": true,
            "source_bodies_included": false,
            "dynamic_expressions_included": false,
        }))
    }

    pub fn edge_evidence(&self, symbol_id: &str, limit: usize) -> Result<QueryResponse<GraphEdge>> {
        let limit = limit.clamp(1, 100);
        let mut statement = self.connection.prepare(
            "SELECT id,source_id,target_id,kind,origin,confidence,source_file,source_range,source_revision_id,analyzer,analyzer_version,ambiguity_reason,last_seen_at
             FROM edges WHERE source_id=?1 OR target_id=?1
             ORDER BY confidence DESC,last_seen_at DESC LIMIT ?2",
        )?;
        let items = statement
            .query_map(params![symbol_id, limit as i64], edge_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let coverage = self.coverage()?;
        Ok(QueryResponse {
            summary: format!("Found {} provenance-bearing edges.", items.len()),
            absence_proven: items.is_empty() && coverage.absence_scope_complete(),
            unknowns: if items.is_empty() && !coverage.absence_scope_complete() {
                vec!["No edge evidence is not proof that the relationship does not exist.".into()]
            } else {
                vec![]
            },
            coverage,
            items,
            context_handle: None,
            items_remaining: 0,
        })
    }

    pub fn ingest_runtime_edges(&mut self, input: RuntimeIngest<'_>) -> Result<usize> {
        if self.revision()? != input.source_revision_id {
            return Err(anyhow!(
                "Runtime evidence revision does not match the CURRENT graph"
            ));
        }
        let tx = self.connection.transaction()?;
        tx.execute(
            "DELETE FROM runtime_edges WHERE runtime_run_id=?1",
            params![input.runtime_run_id],
        )?;
        for (index, runtime_edge) in input.edges.iter().enumerate() {
            tx.execute(
                "INSERT INTO runtime_edges(id,runtime_run_id,source_revision_id,source_symbol,target_symbol,kind,workload,observed_at,count,coverage_scope,adapter,adapter_version)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                params![
                    format!("{}:{index}", input.runtime_run_id),
                    input.runtime_run_id,
                    input.source_revision_id,
                    runtime_edge.source_symbol,
                    runtime_edge.target_symbol,
                    runtime_edge.kind,
                    input.workload,
                    input.observed_at,
                    runtime_edge.count,
                    input.coverage_scope,
                    input.adapter,
                    input.adapter_version,
                ],
            )?;
        }
        tx.commit()?;
        Ok(input.edges.len())
    }

    pub fn neighbors(
        &self,
        symbol_id: &str,
        direction: &str,
        limit: usize,
    ) -> Result<QueryResponse<serde_json::Value>> {
        let (join, where_column) = if direction == "callees" {
            ("e.target_id=n.id", "e.source_id")
        } else {
            ("e.source_id=n.id", "e.target_id")
        };
        let sql = format!(
            "SELECT n.id,n.kind,n.qualified_name,n.file_path,n.start_line,e.kind,e.origin,e.confidence,e.ambiguity_reason FROM edges e JOIN nodes n ON {join} WHERE {where_column}=?1 ORDER BY e.confidence DESC LIMIT ?2"
        );
        let mut statement = self.connection.prepare(&sql)?;
        let items = statement
            .query_map(params![symbol_id, limit.clamp(1, 100) as i64], |row| {
                Ok(serde_json::json!({
                    "symbol_id": row.get::<_, String>(0)?,
                    "kind": row.get::<_, String>(1)?,
                    "qualified_name": row.get::<_, String>(2)?,
                    "path": row.get::<_, String>(3)?,
                    "line": row.get::<_, u32>(4)?,
                    "edge_kind": row.get::<_, String>(5)?,
                    "origin": row.get::<_, String>(6)?,
                    "confidence": row.get::<_, f64>(7)?,
                    "ambiguity_reason": row.get::<_, Option<String>>(8)?,
                }))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let coverage = self.coverage()?;
        Ok(QueryResponse {
            summary: format!("Found {} {}.", items.len(), direction),
            absence_proven: items.is_empty() && coverage.absence_scope_complete(),
            unknowns: if items.is_empty() && !coverage.absence_scope_complete() {
                vec![format!(
                    "No {direction} in the graph is not proof of absence."
                )]
            } else {
                vec![]
            },
            coverage,
            items,
            context_handle: None,
            items_remaining: 0,
        })
    }

    pub fn find_path(
        &self,
        source_id: &str,
        sink_id: &str,
        max_depth: usize,
    ) -> Result<Vec<String>> {
        let mut statement = self
            .connection
            .prepare("SELECT source_id,target_id FROM edges")?;
        let mut graph: HashMap<String, Vec<String>> = HashMap::new();
        for row in statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })? {
            let (source, target) = row?;
            graph.entry(source).or_default().push(target);
        }
        Ok(shortest_path(graph, source_id, sink_id, max_depth))
    }

    /// Practical flow reachability deliberately excludes structural edges such as CONTAINS and
    /// IMPORTS. Runtime observations are included only when pinned to this graph revision.
    pub fn find_flow_path(
        &self,
        source_id: &str,
        sink_id: &str,
        max_depth: usize,
    ) -> Result<Vec<String>> {
        let mut graph: HashMap<String, Vec<String>> = HashMap::new();
        let mut statement = self.connection.prepare(
            "SELECT source_id,target_id FROM edges WHERE kind IN (
               'CALLS','ROUTES_TO','HTTP_CALLS','ASYNC_CALLS','DISPATCHES','LISTENS_TO',
               'READS_FROM','WRITES_TO','FLOWS_TO'
             )",
        )?;
        for row in statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })? {
            let (source, target) = row?;
            graph.entry(source).or_default().push(target);
        }
        let revision = self.revision()?;
        let mut runtime_statement = self.connection.prepare(
            "SELECT source_symbol,target_symbol FROM runtime_edges
             WHERE source_revision_id=?1 AND kind IN (
               'RUNTIME_CALLS','HTTP_CALLS','ASYNC_CALLS','RUNTIME_READS_FROM',
               'RUNTIME_WRITES_TO','RUNTIME_DISPATCHES'
             )",
        )?;
        for row in runtime_statement.query_map(params![revision], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })? {
            let (source, target) = row?;
            graph.entry(source).or_default().push(target);
        }
        Ok(shortest_path(graph, source_id, sink_id, max_depth))
    }

    pub fn dead_code_candidates(&self, limit: usize) -> Result<Vec<serde_json::Value>> {
        let mut statement = self.connection.prepare(
            "SELECT n.id,n.qualified_name,n.kind,n.file_path,n.start_line,n.exported,n.entry_point,COUNT(e.id) callers FROM nodes n LEFT JOIN edges e ON e.target_id=n.id AND e.kind IN ('CALLS','REFERENCES','ROUTES_TO','LISTENS_TO','SCHEDULES','TESTS') WHERE n.kind IN ('FUNCTION','METHOD','CLASS') GROUP BY n.id HAVING callers=0 AND n.exported=0 AND n.entry_point=0 LIMIT ?1",
        )?;
        let candidates = statement
            .query_map(params![limit.clamp(1, 200) as i64], |row| {
                Ok(serde_json::json!({
                    "symbol": row.get::<_, String>(1)?,
                    "symbol_id": row.get::<_, String>(0)?,
                    "kind": row.get::<_, String>(2)?,
                    "path": row.get::<_, String>(3)?,
                    "line": row.get::<_, u32>(4)?,
                    "status": "CANDIDATE",
                    "confidence": 0.55,
                    "evidence": {"static_callers": 0},
                    "unknowns": ["Dynamic/framework/config/runtime references may exist."],
                    "auto_delete_allowed": false
                }))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(candidates)
    }
}

fn compact_node(node: &GraphNode) -> serde_json::Value {
    serde_json::json!({
        "id": node.id,
        "kind": node.kind,
        "qualified_name": node.qualified_name,
        "path": node.file_path,
        "line": node.start_line,
        "language": node.language,
    })
}

fn natural_fts_query(input: &str) -> String {
    input
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .filter(|term| term.chars().count() >= 2)
        .take(12)
        .map(|term| format!("\"{term}\""))
        .collect::<Vec<_>>()
        .join(" OR ")
}

fn shortest_path(
    graph: HashMap<String, Vec<String>>,
    source_id: &str,
    sink_id: &str,
    max_depth: usize,
) -> Vec<String> {
    let mut queue = VecDeque::from([(source_id.to_string(), vec![source_id.to_string()])]);
    let mut visited = HashSet::new();
    while let Some((current, path)) = queue.pop_front() {
        if current == sink_id {
            return path;
        }
        if path.len() > max_depth || !visited.insert(current.clone()) {
            continue;
        }
        for next in graph.get(&current).into_iter().flatten() {
            let mut next_path = path.clone();
            next_path.push(next.clone());
            queue.push_back((next.clone(), next_path));
        }
    }
    vec![]
}

fn configure(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=5000;",
    )?;
    Ok(())
}

fn create_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS index_metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS file_inventory(
          logical_path TEXT PRIMARY KEY,size INTEGER NOT NULL,mode INTEGER NOT NULL,content_sha256 TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS nodes(
          id TEXT PRIMARY KEY,kind TEXT NOT NULL,qualified_name TEXT NOT NULL,display_name TEXT NOT NULL,
          file_path TEXT NOT NULL,start_line INTEGER NOT NULL,start_column INTEGER NOT NULL,end_line INTEGER NOT NULL,
          end_column INTEGER NOT NULL,signature TEXT,language TEXT NOT NULL,exported INTEGER NOT NULL DEFAULT 0,
          entry_point INTEGER NOT NULL DEFAULT 0,logical_root TEXT NOT NULL DEFAULT 'primary',
          canonical_identity TEXT NOT NULL DEFAULT ''
        );
        CREATE INDEX IF NOT EXISTS ix_nodes_qualified ON nodes(qualified_name);
        CREATE INDEX IF NOT EXISTS ix_nodes_display ON nodes(display_name);
        CREATE INDEX IF NOT EXISTS ix_nodes_file ON nodes(file_path,start_line);
        CREATE TABLE IF NOT EXISTS node_provenance(
          node_id TEXT NOT NULL,origin TEXT NOT NULL,producer TEXT NOT NULL,canonical_identity TEXT NOT NULL,
          UNIQUE(node_id,origin,producer,canonical_identity),
          FOREIGN KEY(node_id) REFERENCES nodes(id)
        );
        CREATE INDEX IF NOT EXISTS ix_node_provenance_node ON node_provenance(node_id);
        CREATE TABLE IF NOT EXISTS edges(
          id TEXT PRIMARY KEY,source_id TEXT NOT NULL,target_id TEXT NOT NULL,kind TEXT NOT NULL,origin TEXT NOT NULL,
          confidence REAL NOT NULL,source_file TEXT NOT NULL,source_range TEXT NOT NULL,source_revision_id TEXT NOT NULL,
          analyzer TEXT NOT NULL,analyzer_version TEXT NOT NULL,ambiguity_reason TEXT,last_seen_at TEXT NOT NULL,
          FOREIGN KEY(source_id) REFERENCES nodes(id),FOREIGN KEY(target_id) REFERENCES nodes(id)
        );
        CREATE INDEX IF NOT EXISTS ix_edges_source_kind ON edges(source_id,kind);
        CREATE INDEX IF NOT EXISTS ix_edges_target_kind ON edges(target_id,kind);
        CREATE VIRTUAL TABLE IF NOT EXISTS source_fts USING fts5(path UNINDEXED,language UNINDEXED,content,tokenize='unicode61');
        CREATE TABLE IF NOT EXISTS runtime_edges(
          id TEXT PRIMARY KEY,runtime_run_id TEXT NOT NULL,source_revision_id TEXT NOT NULL,source_symbol TEXT NOT NULL,
          target_symbol TEXT NOT NULL,kind TEXT NOT NULL,workload TEXT NOT NULL,observed_at TEXT NOT NULL,count INTEGER NOT NULL,
          coverage_scope TEXT NOT NULL,adapter TEXT NOT NULL,adapter_version TEXT NOT NULL
        );
        ",
    )?;
    Ok(())
}

fn node_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<GraphNode> {
    Ok(GraphNode {
        id: row.get(0)?,
        kind: row.get(1)?,
        qualified_name: row.get(2)?,
        display_name: row.get(3)?,
        file_path: row.get(4)?,
        start_line: row.get(5)?,
        start_column: row.get(6)?,
        end_line: row.get(7)?,
        end_column: row.get(8)?,
        signature: row.get(9)?,
        language: row.get(10)?,
        exported: row.get(11)?,
        entry_point: row.get(12)?,
        logical_root: row.get(13)?,
        canonical_identity: row.get(14)?,
        provenance: vec![],
    })
}

fn edge_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<GraphEdge> {
    Ok(GraphEdge {
        id: row.get(0)?,
        source_id: row.get(1)?,
        target_id: row.get(2)?,
        kind: row.get(3)?,
        origin: row.get(4)?,
        confidence: row.get(5)?,
        source_file: row.get(6)?,
        source_range: row.get(7)?,
        source_revision_id: row.get(8)?,
        analyzer: row.get(9)?,
        analyzer_version: row.get(10)?,
        ambiguity_reason: row.get(11)?,
        last_seen_at: row.get(12)?,
    })
}

pub fn staging_path(current: &Path) -> PathBuf {
    current.with_file_name(format!("graph.building.{}.sqlite3", Uuid::new_v4()))
}

pub struct EdgeEvidence<'a> {
    pub source_id: &'a str,
    pub target_id: &'a str,
    pub kind: &'a str,
    pub origin: &'a str,
    pub confidence: f64,
    pub source_file: &'a str,
    pub source_range: &'a str,
    pub revision: &'a str,
    pub ambiguity_reason: Option<String>,
}

pub fn edge(input: EdgeEvidence<'_>) -> GraphEdge {
    GraphEdge {
        id: Uuid::new_v4().to_string(),
        source_id: input.source_id.into(),
        target_id: input.target_id.into(),
        kind: input.kind.into(),
        origin: input.origin.into(),
        confidence: input.confidence,
        source_file: input.source_file.into(),
        source_range: input.source_range.into(),
        source_revision_id: input.revision.into(),
        analyzer: "codeintel-indexer".into(),
        analyzer_version: env!("CARGO_PKG_VERSION").into(),
        ambiguity_reason: input.ambiguity_reason,
        last_seen_at: Utc::now().to_rfc3339(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn node(id: &str) -> GraphNode {
        GraphNode {
            id: id.into(),
            kind: "FUNCTION".into(),
            qualified_name: id.into(),
            display_name: id.into(),
            file_path: "flow.py".into(),
            start_line: 1,
            start_column: 0,
            end_line: 1,
            end_column: 1,
            signature: None,
            language: "python".into(),
            exported: false,
            entry_point: false,
            logical_root: "primary".into(),
            canonical_identity: format!("test:{id}"),
            provenance: vec![NodeProvenance {
                origin: "STATIC_EXACT".into(),
                producer: "test".into(),
            }],
        }
    }

    fn current_store(edges: &[(&str, &str, &str)]) -> (tempfile::TempDir, GraphStore) {
        let dir = tempdir().unwrap();
        let graph_path = dir.path().join("graph.sqlite3");
        let mut builder = GraphBuilder::create(&graph_path).unwrap();
        let tx = builder.begin().unwrap();
        for id in ["source", "middle", "sink"] {
            let mut candidate = node(id);
            if id == "source" {
                candidate.kind = "ROUTE".into();
                candidate.display_name = "POST /api/chat".into();
                candidate.qualified_name = candidate.display_name.clone();
                candidate.entry_point = true;
            }
            tx.add_node(&candidate).unwrap();
        }
        tx.add_file_content(
            "flow.py",
            "python",
            "remote reader uses server-side pagination and graph visualization",
        )
        .unwrap();
        for (source, target, kind) in edges {
            tx.add_edge(&edge(EdgeEvidence {
                source_id: source,
                target_id: target,
                kind,
                origin: "STATIC_EXACT",
                confidence: 1.0,
                source_file: "flow.py",
                source_range: "1",
                revision: "rev",
                ambiguity_reason: None,
            }))
            .unwrap();
        }
        tx.commit().unwrap();
        builder
            .validate(
                &SourceRevision {
                    source_revision_id: "rev".into(),
                    inventory_hash: "inventory".into(),
                    policy_hash: "policy".into(),
                    indexer_version: "test".into(),
                    root_mapping_version: 1,
                    files: vec![],
                    volatile_files: vec![],
                    created_at: Utc::now(),
                },
                &Coverage {
                    supported_languages: vec!["python".into()],
                    semantic: "COMPLETE".into(),
                    framework: "COMPLETE".into(),
                    ..Coverage::default()
                },
            )
            .unwrap();
        let store = GraphStore::open(&graph_path).unwrap();
        (dir, store)
    }

    #[test]
    fn heuristic_edges_are_never_exact() {
        let dir = tempdir().unwrap();
        let mut builder = GraphBuilder::create(&dir.path().join("graph.sqlite3")).unwrap();
        let tx = builder.begin().unwrap();
        for id in ["a", "b"] {
            tx.add_node(&GraphNode {
                id: id.into(),
                kind: "FUNCTION".into(),
                qualified_name: id.into(),
                display_name: id.into(),
                file_path: "a.py".into(),
                start_line: 1,
                start_column: 0,
                end_line: 1,
                end_column: 1,
                signature: None,
                language: "python".into(),
                exported: false,
                entry_point: false,
                logical_root: "primary".into(),
                canonical_identity: format!("test:{id}"),
                provenance: vec![NodeProvenance {
                    origin: "STATIC_EXACT".into(),
                    producer: "test".into(),
                }],
            })
            .unwrap();
        }
        assert!(tx
            .add_edge(&edge(EdgeEvidence {
                source_id: "a",
                target_id: "b",
                kind: "CALLS",
                origin: "STATIC_HEURISTIC",
                confidence: 1.0,
                source_file: "a.py",
                source_range: "1",
                revision: "rev",
                ambiguity_reason: None,
            }))
            .is_err());
    }

    #[test]
    fn different_canonical_candidates_with_same_primary_key_are_diagnosed() {
        let dir = tempdir().unwrap();
        let mut builder = GraphBuilder::create(&dir.path().join("graph.sqlite3")).unwrap();
        let tx = builder.begin().unwrap();
        let first = node("node:v2:forced-collision");
        tx.add_node(&first).unwrap();
        let mut second = first.clone();
        second.file_path = "other.py".into();
        second.qualified_name = "other".into();
        second.display_name = "other".into();
        second.canonical_identity = "codeintel-node-v2|different".into();
        let error = format!("{:#}", tx.add_node(&second).unwrap_err());
        assert!(error.contains("NODE_ID_COLLISION"));
        assert!(error.contains("incoming_candidate"));
    }

    #[test]
    fn same_canonical_candidate_merges_provenance_explicitly() {
        let dir = tempdir().unwrap();
        let graph = dir.path().join("graph.sqlite3");
        let mut builder = GraphBuilder::create(&graph).unwrap();
        let tx = builder.begin().unwrap();
        let first = node("same");
        tx.add_node(&first).unwrap();
        let mut duplicate = first.clone();
        duplicate.provenance.push(NodeProvenance {
            origin: "FRAMEWORK_RESOLVED".into(),
            producer: "framework:test".into(),
        });
        tx.add_node(&duplicate).unwrap();
        let count: i64 = tx
            .tx
            .query_row(
                "SELECT COUNT(*) FROM node_provenance WHERE node_id='same'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn structural_path_is_not_reported_as_dataflow() {
        let (_dir, store) = current_store(&[("source", "sink", "CONTAINS")]);
        assert_eq!(
            store.find_path("source", "sink", 4).unwrap(),
            vec!["source", "sink"]
        );
        assert!(store
            .find_flow_path("source", "sink", 4)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn visualization_is_bounded_ephemeral_and_contains_no_source_body() {
        let (_dir, store) =
            current_store(&[("source", "middle", "CALLS"), ("middle", "sink", "CALLS")]);
        let graph = store.visualization(20, 20).unwrap();
        assert_eq!(graph["bounded"], true);
        assert_eq!(graph["ephemeral"], true);
        assert_eq!(graph["source_bodies_included"], false);
        assert!(graph["nodes"].as_array().unwrap().len() <= 20);
        assert!(graph["edges"].as_array().unwrap().len() <= 20);
        assert!(!graph.to_string().contains("source_body"));
    }

    #[test]
    fn endpoint_inventory_is_paginated_and_excludes_source_bodies() {
        let (_dir, store) = current_store(&[]);
        let inventory = store.endpoint_inventory(1, 25).unwrap();
        assert_eq!(inventory["total"], 1);
        assert_eq!(inventory["source_bodies_included"], false);
        assert_eq!(inventory["dynamic_expressions_included"], false);
        assert_eq!(inventory["endpoints"][0]["label"], "POST /api/chat");
        assert!(!inventory
            .to_string()
            .contains("remote reader uses server-side pagination"));
    }

    #[test]
    fn bounded_flow_subgraph_correlates_route_without_source_bodies() {
        let (_dir, store) = current_store(&[
            ("source", "middle", "ROUTES_TO"),
            ("middle", "sink", "CALLS"),
        ]);
        let flow = store.bounded_flow_subgraph("source", 4, 20).unwrap();
        assert_eq!(flow["status"], "FOUND");
        assert_eq!(flow["source_bodies_included"], false);
        assert_eq!(flow["dynamic_expressions_included"], false);
        assert_eq!(flow["nodes"].as_array().unwrap().len(), 3);
        assert_eq!(flow["edges"].as_array().unwrap().len(), 2);
        assert!(!flow
            .to_string()
            .contains("remote reader uses server-side pagination"));
    }

    #[test]
    fn natural_language_search_escapes_fts_operators_and_hyphens() {
        let (_dir, store) = current_store(&[]);
        let result = store
            .search("server-side OR graph:visualization", 20)
            .unwrap();
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0]["path"], "flow.py");
    }

    #[test]
    fn call_and_flow_edges_form_practical_dataflow_path() {
        let (_dir, store) = current_store(&[
            ("source", "middle", "CALLS"),
            ("middle", "sink", "FLOWS_TO"),
        ]);
        assert_eq!(
            store.find_flow_path("source", "sink", 4).unwrap(),
            vec!["source", "middle", "sink"]
        );
    }

    #[test]
    fn current_runtime_edges_participate_in_flow_reachability() {
        let (_dir, mut store) = current_store(&[]);
        store
            .ingest_runtime_edges(RuntimeIngest {
                runtime_run_id: "runtime",
                source_revision_id: "rev",
                workload: "test workload",
                observed_at: "2026-08-17T00:00:00Z",
                coverage_scope: "{}",
                adapter: "generic_trace_ingest",
                adapter_version: "test",
                edges: &[RuntimeEdgeObservation {
                    source_symbol: "source".into(),
                    target_symbol: "sink".into(),
                    kind: "RUNTIME_CALLS".into(),
                    count: 1,
                }],
            })
            .unwrap();
        assert_eq!(
            store.find_flow_path("source", "sink", 4).unwrap(),
            vec!["source", "sink"]
        );
    }

    #[test]
    fn failed_atomic_publication_restores_previous_current_file() {
        let dir = tempdir().unwrap();
        let current = dir.path().join("graph.sqlite3");
        let missing_staging = dir.path().join("graph.building.sqlite3");
        fs::write(&current, b"previous-current").unwrap();
        assert!(atomic_publish(&missing_staging, &current).is_err());
        assert_eq!(fs::read(&current).unwrap(), b"previous-current");
    }

    #[test]
    fn corrupt_derived_graph_can_be_atomically_replaced() {
        let dir = tempdir().unwrap();
        let current = dir.path().join("graph.sqlite3");
        let staging = dir.path().join("graph.building.sqlite3");
        fs::write(&current, b"not a sqlite database").unwrap();
        assert!(GraphStore::open(&current).is_err());

        let builder = GraphBuilder::create(&staging).unwrap();
        builder
            .validate(
                &SourceRevision {
                    source_revision_id: "rebuilt".into(),
                    inventory_hash: "inventory".into(),
                    policy_hash: "policy".into(),
                    indexer_version: "test".into(),
                    root_mapping_version: 1,
                    files: vec![],
                    volatile_files: vec![],
                    created_at: Utc::now(),
                },
                &Coverage {
                    semantic: "COMPLETE".into(),
                    framework: "COMPLETE".into(),
                    ..Coverage::default()
                },
            )
            .unwrap();
        atomic_publish(&staging, &current).unwrap();
        assert_eq!(
            GraphStore::open(&current).unwrap().revision().unwrap(),
            "rebuilt"
        );
        assert_eq!(
            fs::read(current.with_extension("sqlite3.previous")).unwrap(),
            b"not a sqlite database"
        );
    }
}
