use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "bom_type", rename_all = "snake_case")]
pub enum BomType {
    Normal,
    Kit,
    Subcontract,
}

impl std::fmt::Display for BomType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Normal => write!(f, "normal"),
            Self::Kit => write!(f, "kit"),
            Self::Subcontract => write!(f, "subcontract"),
        }
    }
}

impl FromStr for BomType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "normal" => Ok(Self::Normal),
            "kit" => Ok(Self::Kit),
            "subcontract" => Ok(Self::Subcontract),
            _ => Err(format!("Unknown BomType variant: {}", s)),
        }
    }
}

impl Default for BomType {
    fn default() -> Self {
        Self::Normal
    }
}
