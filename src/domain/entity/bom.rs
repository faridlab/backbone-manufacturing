use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use rust_decimal::Decimal;

use super::BomType;
use super::BomStatus;
use super::AuditMetadata;

/// Strongly-typed ID for Bom
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BomId(pub Uuid);

impl BomId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for BomId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for BomId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for BomId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<BomId> for Uuid {
    fn from(id: BomId) -> Self { id.0 }
}

impl AsRef<Uuid> for BomId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for BomId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Bom {
    pub id: Uuid,
    pub company_id: Option<Uuid>,
    pub item_id: Uuid,
    pub bom_code: String,
    pub version: i32,
    pub bom_type: BomType,
    pub quantity: Decimal,
    pub uom: Option<String>,
    pub currency: String,
    pub raw_material_cost: Decimal,
    pub operating_cost: Decimal,
    pub total_cost: Decimal,
    pub status: BomStatus,
    pub is_default: bool,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl Bom {
    /// Create a builder for Bom
    pub fn builder() -> BomBuilder {
        <BomBuilder as Default>::default()
    }

    /// Create a new Bom with required fields
    pub fn new(item_id: Uuid, bom_code: String, version: i32, bom_type: BomType, quantity: Decimal, currency: String, raw_material_cost: Decimal, operating_cost: Decimal, total_cost: Decimal, status: BomStatus, is_default: bool) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id: None,
            item_id,
            bom_code,
            version,
            bom_type,
            quantity,
            uom: None,
            currency,
            raw_material_cost,
            operating_cost,
            total_cost,
            status,
            is_default,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> BomId {
        BomId(self.id)
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
    pub fn status(&self) -> &BomStatus {
        &self.status
    }


    // ==========================================================
    // Fluent Setters (with_* for optional fields)
    // ==========================================================

    /// Set the company_id field (chainable)
    pub fn with_company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the uom field (chainable)
    pub fn with_uom(mut self, value: String) -> Self {
        self.uom = Some(value);
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
                "item_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.item_id = v; }
                }
                "bom_code" => {
                    if let Ok(v) = serde_json::from_value(value) { self.bom_code = v; }
                }
                "version" => {
                    if let Ok(v) = serde_json::from_value(value) { self.version = v; }
                }
                "bom_type" => {
                    if let Ok(v) = serde_json::from_value(value) { self.bom_type = v; }
                }
                "quantity" => {
                    if let Ok(v) = serde_json::from_value(value) { self.quantity = v; }
                }
                "uom" => {
                    if let Ok(v) = serde_json::from_value(value) { self.uom = v; }
                }
                "currency" => {
                    if let Ok(v) = serde_json::from_value(value) { self.currency = v; }
                }
                "raw_material_cost" => {
                    if let Ok(v) = serde_json::from_value(value) { self.raw_material_cost = v; }
                }
                "operating_cost" => {
                    if let Ok(v) = serde_json::from_value(value) { self.operating_cost = v; }
                }
                "total_cost" => {
                    if let Ok(v) = serde_json::from_value(value) { self.total_cost = v; }
                }
                "status" => {
                    if let Ok(v) = serde_json::from_value(value) { self.status = v; }
                }
                "is_default" => {
                    if let Ok(v) = serde_json::from_value(value) { self.is_default = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for Bom {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "Bom"
    }
}

impl backbone_core::PersistentEntity for Bom {
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

impl backbone_orm::EntityRepoMeta for Bom {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("item_id".to_string(), "uuid".to_string());
        m.insert("bom_type".to_string(), "bom_type".to_string());
        m.insert("status".to_string(), "bom_status".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["bom_code", "currency"]
    }
    fn company_field() -> Option<&'static str> {
        Some("company_id")
    }
}

/// Builder for Bom entity
///
/// Provides a fluent API for constructing Bom instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct BomBuilder {
    company_id: Option<Uuid>,
    item_id: Option<Uuid>,
    bom_code: Option<String>,
    version: Option<i32>,
    bom_type: Option<BomType>,
    quantity: Option<Decimal>,
    uom: Option<String>,
    currency: Option<String>,
    raw_material_cost: Option<Decimal>,
    operating_cost: Option<Decimal>,
    total_cost: Option<Decimal>,
    status: Option<BomStatus>,
    is_default: Option<bool>,
}

impl BomBuilder {
    /// Set the company_id field (optional)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the item_id field (required)
    pub fn item_id(mut self, value: Uuid) -> Self {
        self.item_id = Some(value);
        self
    }

    /// Set the bom_code field (required)
    pub fn bom_code(mut self, value: String) -> Self {
        self.bom_code = Some(value);
        self
    }

    /// Set the version field (default: `1`)
    pub fn version(mut self, value: i32) -> Self {
        self.version = Some(value);
        self
    }

    /// Set the bom_type field (default: `BomType::default()`)
    pub fn bom_type(mut self, value: BomType) -> Self {
        self.bom_type = Some(value);
        self
    }

    /// Set the quantity field (default: `Decimal::from(1)`)
    pub fn quantity(mut self, value: Decimal) -> Self {
        self.quantity = Some(value);
        self
    }

    /// Set the uom field (optional)
    pub fn uom(mut self, value: String) -> Self {
        self.uom = Some(value);
        self
    }

    /// Set the currency field (default: `"IDR".to_string()`)
    pub fn currency(mut self, value: String) -> Self {
        self.currency = Some(value);
        self
    }

    /// Set the raw_material_cost field (default: `Decimal::from(0)`)
    pub fn raw_material_cost(mut self, value: Decimal) -> Self {
        self.raw_material_cost = Some(value);
        self
    }

    /// Set the operating_cost field (default: `Decimal::from(0)`)
    pub fn operating_cost(mut self, value: Decimal) -> Self {
        self.operating_cost = Some(value);
        self
    }

    /// Set the total_cost field (default: `Decimal::from(0)`)
    pub fn total_cost(mut self, value: Decimal) -> Self {
        self.total_cost = Some(value);
        self
    }

    /// Set the status field (default: `BomStatus::default()`)
    pub fn status(mut self, value: BomStatus) -> Self {
        self.status = Some(value);
        self
    }

    /// Set the is_default field (default: `false`)
    pub fn is_default(mut self, value: bool) -> Self {
        self.is_default = Some(value);
        self
    }

    /// Build the Bom entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<Bom, String> {
        let item_id = self.item_id.ok_or_else(|| "item_id is required".to_string())?;
        let bom_code = self.bom_code.ok_or_else(|| "bom_code is required".to_string())?;

        Ok(Bom {
            id: Uuid::new_v4(),
            company_id: self.company_id,
            item_id,
            bom_code,
            version: self.version.unwrap_or(1),
            bom_type: self.bom_type.unwrap_or_default(),
            quantity: self.quantity.unwrap_or(Decimal::from(1)),
            uom: self.uom,
            currency: self.currency.unwrap_or("IDR".to_string()),
            raw_material_cost: self.raw_material_cost.unwrap_or(Decimal::from(0)),
            operating_cost: self.operating_cost.unwrap_or(Decimal::from(0)),
            total_cost: self.total_cost.unwrap_or(Decimal::from(0)),
            status: self.status.unwrap_or_default(),
            is_default: self.is_default.unwrap_or(false),
            metadata: AuditMetadata::default(),
        })
    }
}
