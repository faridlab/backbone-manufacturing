use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use super::AuditMetadata;

/// Strongly-typed ID for WorkstationProductivity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkstationProductivityId(pub Uuid);

impl WorkstationProductivityId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for WorkstationProductivityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for WorkstationProductivityId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for WorkstationProductivityId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<WorkstationProductivityId> for Uuid {
    fn from(id: WorkstationProductivityId) -> Self { id.0 }
}

impl AsRef<Uuid> for WorkstationProductivityId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for WorkstationProductivityId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WorkstationProductivity {
    pub id: Uuid,
    pub company_id: Uuid,
    pub workstation_id: Uuid,
    pub job_card_id: Option<Uuid>,
    pub loss_id: Uuid,
    pub date_start: DateTime<Utc>,
    pub date_end: Option<DateTime<Utc>>,
    pub description: Option<String>,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl WorkstationProductivity {
    /// Create a builder for WorkstationProductivity
    pub fn builder() -> WorkstationProductivityBuilder {
        <WorkstationProductivityBuilder as Default>::default()
    }

    /// Create a new WorkstationProductivity with required fields
    pub fn new(company_id: Uuid, workstation_id: Uuid, loss_id: Uuid, date_start: DateTime<Utc>) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id,
            workstation_id,
            job_card_id: None,
            loss_id,
            date_start,
            date_end: None,
            description: None,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> WorkstationProductivityId {
        WorkstationProductivityId(self.id)
    }

    /// Get when this entity was created
    pub fn created_at(&self) -> Option<&DateTime<Utc>> {
        self.metadata.created_at.as_ref()
    }

    /// Get when this entity was last updated
    pub fn updated_at(&self) -> Option<&DateTime<Utc>> {
        self.metadata.updated_at.as_ref()
    }

    /// Check if this entity is soft deleted
    pub fn is_deleted(&self) -> bool {
        self.metadata.deleted_at.is_some()
    }

    /// Check if this entity is active (not deleted)
    pub fn is_active(&self) -> bool {
        self.metadata.deleted_at.is_none()
    }

    /// Get when this entity was deleted
    pub fn deleted_at(&self) -> Option<&DateTime<Utc>> {
        self.metadata.deleted_at.as_ref()
    }

    /// Get who created this entity
    pub fn created_by(&self) -> Option<&Uuid> {
        self.metadata.created_by.as_ref()
    }

    /// Get who last updated this entity
    pub fn updated_by(&self) -> Option<&Uuid> {
        self.metadata.updated_by.as_ref()
    }

    /// Get who deleted this entity
    pub fn deleted_by(&self) -> Option<&Uuid> {
        self.metadata.deleted_by.as_ref()
    }


    // ==========================================================
    // Fluent Setters (with_* for optional fields)
    // ==========================================================

    /// Set the job_card_id field (chainable)
    pub fn with_job_card_id(mut self, value: Uuid) -> Self {
        self.job_card_id = Some(value);
        self
    }

    /// Set the date_end field (chainable)
    pub fn with_date_end(mut self, value: DateTime<Utc>) -> Self {
        self.date_end = Some(value);
        self
    }

    /// Set the description field (chainable)
    pub fn with_description(mut self, value: String) -> Self {
        self.description = Some(value);
        self
    }

    // ==========================================================
    // Partial Update
    // ==========================================================

    /// Apply partial updates from a map of field name to JSON value
    pub fn apply_patch(&mut self, fields: std::collections::HashMap<String, serde_json::Value>) {
        for (key, value) in fields {
            match key.as_str() {
                "company_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.company_id = v; }
                }
                "workstation_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.workstation_id = v; }
                }
                "job_card_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.job_card_id = v; }
                }
                "loss_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.loss_id = v; }
                }
                "date_start" => {
                    if let Ok(v) = serde_json::from_value(value) { self.date_start = v; }
                }
                "date_end" => {
                    if let Ok(v) = serde_json::from_value(value) { self.date_end = v; }
                }
                "description" => {
                    if let Ok(v) = serde_json::from_value(value) { self.description = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for WorkstationProductivity {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "WorkstationProductivity"
    }
}

impl backbone_core::PersistentEntity for WorkstationProductivity {
    fn entity_id(&self) -> String {
        self.id.to_string()
    }
    fn set_entity_id(&mut self, id: String) {
        if let Ok(uuid) = uuid::Uuid::parse_str(&id) {
            self.id = uuid;
        }
    }
    fn created_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata.created_at
    }
    fn set_created_at(&mut self, ts: chrono::DateTime<chrono::Utc>) {
        self.metadata.created_at = Some(ts);
    }
    fn updated_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata.updated_at
    }
    fn set_updated_at(&mut self, ts: chrono::DateTime<chrono::Utc>) {
        self.metadata.updated_at = Some(ts);
    }
    fn deleted_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata.deleted_at
    }
    fn set_deleted_at(&mut self, ts: Option<chrono::DateTime<chrono::Utc>>) {
        self.metadata.deleted_at = ts;
    }
}

impl backbone_orm::EntityRepoMeta for WorkstationProductivity {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("workstation_id".to_string(), "uuid".to_string());
        m.insert("job_card_id".to_string(), "uuid".to_string());
        m.insert("loss_id".to_string(), "uuid".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &[]
    }
    fn company_field() -> Option<&'static str> {
        Some("company_id")
    }
}

/// Builder for WorkstationProductivity entity
///
/// Provides a fluent API for constructing WorkstationProductivity instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct WorkstationProductivityBuilder {
    company_id: Option<Uuid>,
    workstation_id: Option<Uuid>,
    job_card_id: Option<Uuid>,
    loss_id: Option<Uuid>,
    date_start: Option<DateTime<Utc>>,
    date_end: Option<DateTime<Utc>>,
    description: Option<String>,
}

impl WorkstationProductivityBuilder {
    /// Set the company_id field (required)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the workstation_id field (required)
    pub fn workstation_id(mut self, value: Uuid) -> Self {
        self.workstation_id = Some(value);
        self
    }

    /// Set the job_card_id field (optional)
    pub fn job_card_id(mut self, value: Uuid) -> Self {
        self.job_card_id = Some(value);
        self
    }

    /// Set the loss_id field (required)
    pub fn loss_id(mut self, value: Uuid) -> Self {
        self.loss_id = Some(value);
        self
    }

    /// Set the date_start field (default: `Utc::now()`)
    pub fn date_start(mut self, value: DateTime<Utc>) -> Self {
        self.date_start = Some(value);
        self
    }

    /// Set the date_end field (optional)
    pub fn date_end(mut self, value: DateTime<Utc>) -> Self {
        self.date_end = Some(value);
        self
    }

    /// Set the description field (optional)
    pub fn description(mut self, value: String) -> Self {
        self.description = Some(value);
        self
    }

    /// Build the WorkstationProductivity entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<WorkstationProductivity, String> {
        let company_id = self.company_id.ok_or_else(|| "company_id is required".to_string())?;
        let workstation_id = self.workstation_id.ok_or_else(|| "workstation_id is required".to_string())?;
        let loss_id = self.loss_id.ok_or_else(|| "loss_id is required".to_string())?;

        Ok(WorkstationProductivity {
            id: Uuid::new_v4(),
            company_id,
            workstation_id,
            job_card_id: self.job_card_id,
            loss_id,
            date_start: self.date_start.unwrap_or(Utc::now()),
            date_end: self.date_end,
            description: self.description,
            metadata: AuditMetadata::default(),
        })
    }
}
