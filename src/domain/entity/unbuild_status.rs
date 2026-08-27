use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "unbuild_status", rename_all = "snake_case")]
pub enum UnbuildStatus {
    Draft,
    Done,
}

impl std::fmt::Display for UnbuildStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Draft => write!(f, "draft"),
            Self::Done => write!(f, "done"),
        }
    }
}

impl FromStr for UnbuildStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "draft" => Ok(Self::Draft),
            "done" => Ok(Self::Done),
            _ => Err(format!("Unknown UnbuildStatus variant: {}", s)),
        }
    }
}

impl Default for UnbuildStatus {
    fn default() -> Self {
        Self::Draft
    }
}
