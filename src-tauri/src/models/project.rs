use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub tasks: Vec<ProjectTask>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTask {
    pub id: String,
    pub name: String,
    /// "builder" — use serde rename because "type" is a reserved keyword in Rust.
    #[serde(rename = "type")]
    pub task_type: String,
    /// Full BuilderConfig snapshot (stored as opaque JSON).
    pub config: serde_json::Value,
    /// BuilderDatabank[] snapshot (stored as opaque JSON).
    pub databanks: serde_json::Value,
    pub status: String, // "idle" | "running" | "paused"
    pub strategies_count: u32,
    pub databank_count: u32,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run_at: Option<String>,
}
