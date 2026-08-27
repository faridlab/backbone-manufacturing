use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "loss_type", rename_all = "snake_case")]
pub enum LossType {
    Productive,
    Availability,
    Performance,
    Quality,
}

impl std::fmt::Display for LossType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Productive => write!(f, "productive"),
            Self::Availability => write!(f, "availability"),
            Self::Performance => write!(f, "performance"),
            Self::Quality => write!(f, "quality"),
        }
    }
}

impl FromStr for LossType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "productive" => Ok(Self::Productive),
            "availability" => Ok(Self::Availability),
            "performance" => Ok(Self::Performance),
            "quality" => Ok(Self::Quality),
            _ => Err(format!("Unknown LossType variant: {}", s)),
        }
    }
}

impl Default for LossType {
    fn default() -> Self {
        Self::Productive
    }
}
