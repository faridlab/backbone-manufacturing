use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "work_order_status", rename_all = "snake_case")]
pub enum WorkOrderStatus {
    Draft,
    Released,
    InProcess,
    Completed,
    Stopped,
}

impl std::fmt::Display for WorkOrderStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Draft => write!(f, "draft"),
            Self::Released => write!(f, "released"),
            Self::InProcess => write!(f, "in_process"),
            Self::Completed => write!(f, "completed"),
            Self::Stopped => write!(f, "stopped"),
        }
    }
}

impl FromStr for WorkOrderStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "draft" => Ok(Self::Draft),
            "released" => Ok(Self::Released),
            "in_process" => Ok(Self::InProcess),
            "completed" => Ok(Self::Completed),
            "stopped" => Ok(Self::Stopped),
            _ => Err(format!("Unknown WorkOrderStatus variant: {}", s)),
        }
    }
}

impl Default for WorkOrderStatus {
    fn default() -> Self {
        Self::Draft
    }
}
