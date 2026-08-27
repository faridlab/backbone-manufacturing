use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use rust_decimal::Decimal;
use super::AuditMetadata;

/// Strongly-typed ID for BomByproduct
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BomByproductId(pub Uuid);

impl BomByproductId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for BomByproductId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for BomByproductId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for BomByproductId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<BomByproductId> for Uuid {
    fn from(id: BomByproductId) -> Self { id.0 }
}

impl AsRef<Uuid> for BomByproductId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for BomByproductId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct BomByproduct {
    pub id: Uuid,
    pub company_id: Option<Uuid>,
    pub bom_id: Uuid,
    pub item_id: Uuid,
    pub product_category_id: Option<Uuid>,
    pub quantity: Decimal,
    pub cost_share: Decimal,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl BomByproduct {
    /// Create a builder for BomByproduct
    pub fn builder() -> BomByproductBuilder {
        <BomByproductBuilder as Default>::default()
    }

    /// Create a new BomByproduct with required fields
    pub fn new(bom_id: Uuid, item_id: Uuid, quantity: Decimal, cost_share: Decimal) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id: None,
            bom_id,
            item_id,
            product_category_id: None,
            quantity,
            cost_share,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> BomByproductId {
        BomByproductId(self.id)
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

    /// Set the company_id field (chainable)
    pub fn with_company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the product_category_id field (chainable)
    pub fn with_product_category_id(mut self, value: Uuid) -> Self {
        self.product_category_id = Some(value);
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
                "bom_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.bom_id = v; }
                }
                "item_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.item_id = v; }
                }
                "product_category_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.product_category_id = v; }
                }
                "quantity" => {
                    if let Ok(v) = serde_json::from_value(value) { self.quantity = v; }
                }
                "cost_share" => {
                    if let Ok(v) = serde_json::from_value(value) { self.cost_share = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for BomByproduct {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "BomByproduct"
    }
}

impl backbone_core::PersistentEntity for BomByproduct {
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

impl backbone_orm::EntityRepoMeta for BomByproduct {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("bom_id".to_string(), "uuid".to_string());
        m.insert("item_id".to_string(), "uuid".to_string());
        m.insert("product_category_id".to_string(), "uuid".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &[]
    }
    fn company_field() -> Option<&'static str> {
        Some("company_id")
    }
}

/// Builder for BomByproduct entity
///
/// Provides a fluent API for constructing BomByproduct instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct BomByproductBuilder {
    company_id: Option<Uuid>,
    bom_id: Option<Uuid>,
    item_id: Option<Uuid>,
    product_category_id: Option<Uuid>,
    quantity: Option<Decimal>,
    cost_share: Option<Decimal>,
}

impl BomByproductBuilder {
    /// Set the company_id field (optional)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the bom_id field (required)
    pub fn bom_id(mut self, value: Uuid) -> Self {
        self.bom_id = Some(value);
        self
    }

    /// Set the item_id field (required)
    pub fn item_id(mut self, value: Uuid) -> Self {
        self.item_id = Some(value);
        self
    }

    /// Set the product_category_id field (optional)
    pub fn product_category_id(mut self, value: Uuid) -> Self {
        self.product_category_id = Some(value);
        self
    }

    /// Set the quantity field (required)
    pub fn quantity(mut self, value: Decimal) -> Self {
        self.quantity = Some(value);
        self
    }

    /// Set the cost_share field (default: `Decimal::from(0)`)
    pub fn cost_share(mut self, value: Decimal) -> Self {
        self.cost_share = Some(value);
        self
    }

    /// Build the BomByproduct entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<BomByproduct, String> {
        let bom_id = self.bom_id.ok_or_else(|| "bom_id is required".to_string())?;
        let item_id = self.item_id.ok_or_else(|| "item_id is required".to_string())?;
        let quantity = self.quantity.ok_or_else(|| "quantity is required".to_string())?;

        Ok(BomByproduct {
            id: Uuid::new_v4(),
            company_id: self.company_id,
            bom_id,
            item_id,
            product_category_id: self.product_category_id,
            quantity,
            cost_share: self.cost_share.unwrap_or(Decimal::from(0)),
            metadata: AuditMetadata::default(),
        })
    }
}
