//! WDR DevEx placeholder crate (foundation only).
//!
//! Serves as the minimal runnable unit for the B0 foundation gate
//! (`just unit`, `just lint`, `just build`). No product features.

/// Host banner used by the DevEx self-test.
pub fn host_banner() -> String {
    "wdr-dev-ready".to_string()
}

/// Reads the `WDR_DEV_ENV` variable, falling back to `"unset"` when absent.
pub fn dev_env_name() -> String {
    match std::env::var_os("WDR_DEV_ENV") {
        Some(v) => v.to_string_lossy().into_owned(),
        None => "unset".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_banner_is_ready() {
        assert_eq!(host_banner(), "wdr-dev-ready");
    }

    #[test]
    fn reads_env_var_with_fallback() {
        std::env::set_var("WDR_DEV_ENV", "ci");
        let name = dev_env_name();
        std::env::remove_var("WDR_DEV_ENV");
        assert_eq!(name, "ci");
        assert_eq!(dev_env_name(), "unset");
    }
}
