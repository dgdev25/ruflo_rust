#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportOutcome {
    LocalFallback {
        reason: String,
        activation_error: bool,
    },
    SlimActivated {
        endpoint: String,
    },
}

pub trait SlimTransportAdapter {
    fn is_available(&self) -> Result<bool, String>;
    fn activate(&self, endpoint: &str) -> Result<(), String>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoSlimTransportAdapter;

impl SlimTransportAdapter for NoSlimTransportAdapter {
    fn is_available(&self) -> Result<bool, String> {
        Ok(false)
    }
    fn activate(&self, _endpoint: &str) -> Result<(), String> {
        Err("unavailable adapter cannot activate".into())
    }
}

pub fn activate_slim(
    endpoint: Option<&str>,
    adapter: &dyn SlimTransportAdapter,
) -> TransportOutcome {
    let Some(endpoint) = endpoint.filter(|value| !value.is_empty()) else {
        return TransportOutcome::LocalFallback {
            reason: "RUFLO_AGNTCY_SLIM_ENDPOINT is not set".into(),
            activation_error: false,
        };
    };
    match adapter.is_available() {
        Ok(true) => match adapter.activate(endpoint) {
            Ok(()) => TransportOutcome::SlimActivated {
                endpoint: endpoint.into(),
            },
            Err(reason) => TransportOutcome::LocalFallback {
                reason,
                activation_error: true,
            },
        },
        Ok(false) => TransportOutcome::LocalFallback {
            reason: "optional \"@agntcy/slim-bindings\" runtime package is not installed".into(),
            activation_error: false,
        },
        Err(reason) => TransportOutcome::LocalFallback {
            reason,
            activation_error: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Adapter {
        available: Result<bool, &'static str>,
        activation: Result<(), &'static str>,
    }
    impl SlimTransportAdapter for Adapter {
        fn is_available(&self) -> Result<bool, String> {
            self.available.map_err(str::to_owned)
        }
        fn activate(&self, _endpoint: &str) -> Result<(), String> {
            self.activation.map_err(str::to_owned)
        }
    }

    #[test]
    fn absent_empty_and_unavailable_endpoints_fall_back_locally() {
        let adapter = Adapter {
            available: Ok(true),
            activation: Ok(()),
        };
        for endpoint in [None, Some("")] {
            assert!(matches!(
                activate_slim(endpoint, &adapter),
                TransportOutcome::LocalFallback {
                    activation_error: false,
                    ..
                }
            ));
        }
        let unavailable = Adapter {
            available: Ok(false),
            activation: Ok(()),
        };
        assert!(matches!(
            activate_slim(Some("https://slim"), &unavailable),
            TransportOutcome::LocalFallback {
                activation_error: false,
                ..
            }
        ));
    }

    #[test]
    fn adapter_success_and_failure_are_typed_without_trimming_endpoint() {
        let success = Adapter {
            available: Ok(true),
            activation: Ok(()),
        };
        assert_eq!(
            activate_slim(Some("  endpoint  "), &success),
            TransportOutcome::SlimActivated {
                endpoint: "  endpoint  ".into()
            }
        );
        let failed = Adapter {
            available: Ok(true),
            activation: Err("connection refused"),
        };
        assert_eq!(
            activate_slim(Some("endpoint"), &failed),
            TransportOutcome::LocalFallback {
                reason: "connection refused".into(),
                activation_error: true
            }
        );
    }
}
