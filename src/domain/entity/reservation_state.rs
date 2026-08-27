use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "reservation_state", rename_all = "snake_case")]
pub enum ReservationState {
    Waiting,
    Confirmed,
    Assigned,
}

impl std::fmt::Display for ReservationState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Waiting => write!(f, "waiting"),
            Self::Confirmed => write!(f, "confirmed"),
            Self::Assigned => write!(f, "assigned"),
        }
    }
}

impl FromStr for ReservationState {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "waiting" => Ok(Self::Waiting),
            "confirmed" => Ok(Self::Confirmed),
            "assigned" => Ok(Self::Assigned),
            _ => Err(format!("Unknown ReservationState variant: {}", s)),
        }
    }
}

impl Default for ReservationState {
    fn default() -> Self {
        Self::Confirmed
    }
}
