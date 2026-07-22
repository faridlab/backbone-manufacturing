use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use rust_decimal::Decimal;

use super::JobCardStatus;
use super::AuditMetadata;

/// Strongly-typed ID for JobCard
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JobCardId(pub Uuid);

impl JobCardId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for JobCardId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for JobCardId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for JobCardId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<JobCardId> for Uuid {
    fn from(id: JobCardId) -> Self { id.0 }
}

impl AsRef<Uuid> for JobCardId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for JobCardId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct JobCard {
    pub id: Uuid,
    pub company_id: Uuid,
    pub work_order_id: Uuid,
    pub operation_id: Uuid,
    pub workstation_id: Uuid,
    pub total_time_mins: Decimal,
    pub hour_rate: Decimal,
    pub operating_cost: Decimal,
    pub status: JobCardStatus,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl JobCard {
    /// Create a builder for JobCard
    pub fn builder() -> JobCardBuilder {
        JobCardBuilder::default()
    }

    /// Create a new JobCard with required fields
    pub fn new(company_id: Uuid, work_order_id: Uuid, operation_id: Uuid, workstation_id: Uuid, total_time_mins: Decimal, hour_rate: Decimal, operating_cost: Decimal, status: JobCardStatus) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id,
            work_order_id,
            operation_id,
            workstation_id,
            total_time_mins,
            hour_rate,
            operating_cost,
            status,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> JobCardId {
        JobCardId(self.id)
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

    /// Get the current status
    pub fn status(&self) -> &JobCardStatus {
        &self.status
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
                "work_order_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.work_order_id = v; }
                }
                "operation_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.operation_id = v; }
                }
                "workstation_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.workstation_id = v; }
                }
                "total_time_mins" => {
                    if let Ok(v) = serde_json::from_value(value) { self.total_time_mins = v; }
                }
                "hour_rate" => {
                    if let Ok(v) = serde_json::from_value(value) { self.hour_rate = v; }
                }
                "operating_cost" => {
                    if let Ok(v) = serde_json::from_value(value) { self.operating_cost = v; }
                }
                "status" => {
                    if let Ok(v) = serde_json::from_value(value) { self.status = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for JobCard {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "JobCard"
    }
}

impl backbone_core::PersistentEntity for JobCard {
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

impl backbone_orm::EntityRepoMeta for JobCard {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("work_order_id".to_string(), "uuid".to_string());
        m.insert("operation_id".to_string(), "uuid".to_string());
        m.insert("workstation_id".to_string(), "uuid".to_string());
        m.insert("status".to_string(), "job_card_status".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &[]
    }
    fn company_field() -> Option<&'static str> {
        Some("company_id")
    }
}

/// Builder for JobCard entity
///
/// Provides a fluent API for constructing JobCard instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct JobCardBuilder {
    company_id: Option<Uuid>,
    work_order_id: Option<Uuid>,
    operation_id: Option<Uuid>,
    workstation_id: Option<Uuid>,
    total_time_mins: Option<Decimal>,
    hour_rate: Option<Decimal>,
    operating_cost: Option<Decimal>,
    status: Option<JobCardStatus>,
}

impl JobCardBuilder {
    /// Set the company_id field (required)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the work_order_id field (required)
    pub fn work_order_id(mut self, value: Uuid) -> Self {
        self.work_order_id = Some(value);
        self
    }

    /// Set the operation_id field (required)
    pub fn operation_id(mut self, value: Uuid) -> Self {
        self.operation_id = Some(value);
        self
    }

    /// Set the workstation_id field (required)
    pub fn workstation_id(mut self, value: Uuid) -> Self {
        self.workstation_id = Some(value);
        self
    }

    /// Set the total_time_mins field (default: `Decimal::from(0)`)
    pub fn total_time_mins(mut self, value: Decimal) -> Self {
        self.total_time_mins = Some(value);
        self
    }

    /// Set the hour_rate field (default: `Decimal::from(0)`)
    pub fn hour_rate(mut self, value: Decimal) -> Self {
        self.hour_rate = Some(value);
        self
    }

    /// Set the operating_cost field (default: `Decimal::from(0)`)
    pub fn operating_cost(mut self, value: Decimal) -> Self {
        self.operating_cost = Some(value);
        self
    }

    /// Set the status field (default: `JobCardStatus::default()`)
    pub fn status(mut self, value: JobCardStatus) -> Self {
        self.status = Some(value);
        self
    }

    /// Build the JobCard entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<JobCard, String> {
        let company_id = self.company_id.ok_or_else(|| "company_id is required".to_string())?;
        let work_order_id = self.work_order_id.ok_or_else(|| "work_order_id is required".to_string())?;
        let operation_id = self.operation_id.ok_or_else(|| "operation_id is required".to_string())?;
        let workstation_id = self.workstation_id.ok_or_else(|| "workstation_id is required".to_string())?;

        Ok(JobCard {
            id: Uuid::new_v4(),
            company_id,
            work_order_id,
            operation_id,
            workstation_id,
            total_time_mins: self.total_time_mins.unwrap_or(Decimal::from(0)),
            hour_rate: self.hour_rate.unwrap_or(Decimal::from(0)),
            operating_cost: self.operating_cost.unwrap_or(Decimal::from(0)),
            status: self.status.unwrap_or(JobCardStatus::default()),
            metadata: AuditMetadata::default(),
        })
    }
}
