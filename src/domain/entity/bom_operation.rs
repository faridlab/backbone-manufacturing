use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use rust_decimal::Decimal;
use super::AuditMetadata;

/// Strongly-typed ID for BomOperation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BomOperationId(pub Uuid);

impl BomOperationId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for BomOperationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for BomOperationId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for BomOperationId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<BomOperationId> for Uuid {
    fn from(id: BomOperationId) -> Self { id.0 }
}

impl AsRef<Uuid> for BomOperationId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for BomOperationId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct BomOperation {
    pub id: Uuid,
    pub company_id: Uuid,
    pub bom_id: Uuid,
    pub operation_id: Uuid,
    pub workstation_id: Uuid,
    pub time_in_mins: Decimal,
    pub hour_rate: Decimal,
    pub operating_cost: Decimal,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl BomOperation {
    /// Create a builder for BomOperation
    pub fn builder() -> BomOperationBuilder {
        BomOperationBuilder::default()
    }

    /// Create a new BomOperation with required fields
    pub fn new(company_id: Uuid, bom_id: Uuid, operation_id: Uuid, workstation_id: Uuid, time_in_mins: Decimal, hour_rate: Decimal, operating_cost: Decimal) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id,
            bom_id,
            operation_id,
            workstation_id,
            time_in_mins,
            hour_rate,
            operating_cost,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> BomOperationId {
        BomOperationId(self.id)
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
    // Partial Update
    // ==========================================================

    /// Apply partial updates from a map of field name to JSON value
    pub fn apply_patch(&mut self, fields: std::collections::HashMap<String, serde_json::Value>) {
        for (key, value) in fields {
            match key.as_str() {
                "company_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.company_id = v; }
                }
                "bom_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.bom_id = v; }
                }
                "operation_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.operation_id = v; }
                }
                "workstation_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.workstation_id = v; }
                }
                "time_in_mins" => {
                    if let Ok(v) = serde_json::from_value(value) { self.time_in_mins = v; }
                }
                "hour_rate" => {
                    if let Ok(v) = serde_json::from_value(value) { self.hour_rate = v; }
                }
                "operating_cost" => {
                    if let Ok(v) = serde_json::from_value(value) { self.operating_cost = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for BomOperation {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "BomOperation"
    }
}

impl backbone_core::PersistentEntity for BomOperation {
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

impl backbone_orm::EntityRepoMeta for BomOperation {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("bom_id".to_string(), "uuid".to_string());
        m.insert("operation_id".to_string(), "uuid".to_string());
        m.insert("workstation_id".to_string(), "uuid".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &[]
    }
    fn company_field() -> Option<&'static str> {
        Some("company_id")
    }
}

/// Builder for BomOperation entity
///
/// Provides a fluent API for constructing BomOperation instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct BomOperationBuilder {
    company_id: Option<Uuid>,
    bom_id: Option<Uuid>,
    operation_id: Option<Uuid>,
    workstation_id: Option<Uuid>,
    time_in_mins: Option<Decimal>,
    hour_rate: Option<Decimal>,
    operating_cost: Option<Decimal>,
}

impl BomOperationBuilder {
    /// Set the company_id field (required)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the bom_id field (required)
    pub fn bom_id(mut self, value: Uuid) -> Self {
        self.bom_id = Some(value);
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

    /// Set the time_in_mins field (required)
    pub fn time_in_mins(mut self, value: Decimal) -> Self {
        self.time_in_mins = Some(value);
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

    /// Build the BomOperation entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<BomOperation, String> {
        let company_id = self.company_id.ok_or_else(|| "company_id is required".to_string())?;
        let bom_id = self.bom_id.ok_or_else(|| "bom_id is required".to_string())?;
        let operation_id = self.operation_id.ok_or_else(|| "operation_id is required".to_string())?;
        let workstation_id = self.workstation_id.ok_or_else(|| "workstation_id is required".to_string())?;
        let time_in_mins = self.time_in_mins.ok_or_else(|| "time_in_mins is required".to_string())?;

        Ok(BomOperation {
            id: Uuid::new_v4(),
            company_id,
            bom_id,
            operation_id,
            workstation_id,
            time_in_mins,
            hour_rate: self.hour_rate.unwrap_or(Decimal::from(0)),
            operating_cost: self.operating_cost.unwrap_or(Decimal::from(0)),
            metadata: AuditMetadata::default(),
        })
    }
}
