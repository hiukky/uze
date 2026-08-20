use serde::Serialize;

#[derive(Clone, Debug, Default)]
pub struct RuntimeSupport {
    pub native_acp: bool,
    pub reliable_acp_adapter: Option<String>,
    pub minimal_adapter: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "path")]
pub enum RuntimeIntegration {
    NativeAcp,
    ReliableAcpAdapter { adapter: String },
    MinimalExplicitAdapter { adapter: String },
    Unavailable { rationale: String },
}

pub fn select_runtime_integration(support: &RuntimeSupport) -> RuntimeIntegration {
    if support.native_acp {
        RuntimeIntegration::NativeAcp
    } else if let Some(adapter) = &support.reliable_acp_adapter {
        RuntimeIntegration::ReliableAcpAdapter {
            adapter: adapter.clone(),
        }
    } else if let Some(adapter) = &support.minimal_adapter {
        RuntimeIntegration::MinimalExplicitAdapter {
            adapter: adapter.clone(),
        }
    } else {
        RuntimeIntegration::Unavailable {
            rationale: "No native ACP support or verified adapter was supplied; UZE will not fabricate runtime integration.".to_owned(),
        }
    }
}

pub fn unverified_runtime_support() -> RuntimeSupport {
    RuntimeSupport::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_acp_wins_over_all_fallbacks() {
        let support = RuntimeSupport {
            native_acp: true,
            reliable_acp_adapter: Some("official-adapter".to_owned()),
            minimal_adapter: Some("minimal".to_owned()),
        };
        assert_eq!(
            select_runtime_integration(&support),
            RuntimeIntegration::NativeAcp
        );
    }
}
