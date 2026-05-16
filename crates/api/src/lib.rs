use nsh_core::AppConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HealthResponse {
    pub success: bool,
    pub status: String,
    pub version: String,
}

pub fn health() -> HealthResponse {
    HealthResponse {
        success: true,
        status: "running".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

pub fn default_config() -> AppConfig {
    AppConfig::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_is_successful() {
        let response = health();
        assert!(response.success);
        assert_eq!(response.status, "running");
    }
}
