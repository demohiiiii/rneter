use super::*;

/// Security level used for SSH algorithm selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum SecurityLevel {
    /// Strict modern algorithms (default).
    Secure,
    /// Good security with broader compatibility.
    Balanced,
    /// Maximum compatibility with legacy devices.
    LegacyCompatible,
}

/// Connection security options for SSH establishment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionSecurityOptions {
    /// SSH algorithm policy.
    pub level: SecurityLevel,
    /// Server host key verification method.
    pub server_check: ServerCheckMethod,
}

impl Default for ConnectionSecurityOptions {
    fn default() -> Self {
        Self::secure_default()
    }
}

impl ConnectionSecurityOptions {
    /// Secure-by-default profile (recommended).
    pub fn secure_default() -> Self {
        Self {
            level: SecurityLevel::Secure,
            server_check: ServerCheckMethod::DefaultKnownHostsFile,
        }
    }

    /// Balanced profile for mixed environments.
    pub fn balanced() -> Self {
        Self {
            level: SecurityLevel::Balanced,
            server_check: ServerCheckMethod::DefaultKnownHostsFile,
        }
    }

    /// Legacy compatibility profile for older devices.
    pub fn legacy_compatible() -> Self {
        Self {
            level: SecurityLevel::LegacyCompatible,
            server_check: ServerCheckMethod::NoCheck,
        }
    }

    pub(super) fn preferred(&self) -> Preferred {
        match self.level {
            SecurityLevel::Secure => Preferred {
                kex: Cow::Borrowed(config::SECURE_KEX_ORDER),
                key: Cow::Borrowed(config::SECURE_KEY_TYPES),
                cipher: Cow::Borrowed(config::SECURE_CIPHERS),
                mac: Cow::Borrowed(config::SECURE_MAC_ALGORITHMS),
                compression: Cow::Borrowed(config::DEFAULT_COMPRESSION_ALGORITHMS),
            },
            SecurityLevel::Balanced => Preferred {
                kex: Cow::Borrowed(config::BALANCED_KEX_ORDER),
                key: Cow::Borrowed(config::BALANCED_KEY_TYPES),
                cipher: Cow::Borrowed(config::BALANCED_CIPHERS),
                mac: Cow::Borrowed(config::BALANCED_MAC_ALGORITHMS),
                compression: Cow::Borrowed(config::DEFAULT_COMPRESSION_ALGORITHMS),
            },
            SecurityLevel::LegacyCompatible => Preferred {
                kex: Cow::Borrowed(config::LEGACY_KEX_ORDER),
                key: Cow::Borrowed(config::LEGACY_KEY_TYPES),
                cipher: Cow::Borrowed(config::LEGACY_CIPHERS),
                mac: Cow::Borrowed(config::LEGACY_MAC_ALGORITHMS),
                compression: Cow::Borrowed(config::DEFAULT_COMPRESSION_ALGORITHMS),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ConnectionSecurityOptions, SecurityLevel};
    use async_ssh2_tokio::ServerCheckMethod;
    use russh::{cipher, kex, mac};

    #[test]
    fn default_security_options_are_secure() {
        let options = ConnectionSecurityOptions::default();
        assert_eq!(options.level, SecurityLevel::Secure);
        assert!(matches!(
            options.server_check,
            ServerCheckMethod::DefaultKnownHostsFile
        ));
    }

    #[test]
    fn legacy_profile_uses_no_host_check() {
        let options = ConnectionSecurityOptions::legacy_compatible();
        assert_eq!(options.level, SecurityLevel::LegacyCompatible);
        assert!(matches!(options.server_check, ServerCheckMethod::NoCheck));
    }

    #[test]
    fn secure_profile_excludes_weak_algorithms() {
        let preferred = ConnectionSecurityOptions::secure_default().preferred();

        assert!(preferred.kex.iter().all(|alg| *alg != kex::NONE));
        assert!(preferred.cipher.iter().all(|alg| *alg != cipher::NONE));
        assert!(preferred.cipher.iter().all(|alg| *alg != cipher::CLEAR));
        assert!(preferred.mac.iter().all(|alg| *alg != mac::NONE));
    }

    #[test]
    fn legacy_profile_keeps_broad_compatibility_algorithms() {
        let preferred = ConnectionSecurityOptions::legacy_compatible().preferred();

        assert!(preferred.kex.contains(&kex::DH_G1_SHA1));
        assert!(preferred.cipher.contains(&cipher::NONE));
        assert!(preferred.mac.contains(&mac::NONE));
    }
}
