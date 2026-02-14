//! SSH configuration constants with multiple security levels.
//!
//! This module contains comprehensive lists of all supported SSH algorithms,
//! ciphers, MAC algorithms, and compression methods. These configurations
//! support both modern secure defaults and legacy compatibility.

use russh::keys::{Algorithm, EcdsaCurve, HashAlg};
use russh::{cipher, compression, kex, mac};

/// Legacy-compatible key exchange algorithms in order of preference.
///
/// Includes modern algorithms like Curve25519 as well as legacy Diffie-Hellman
/// variants for compatibility with older devices.
pub const LEGACY_KEX_ORDER: &[kex::Name] = &[
    kex::CURVE25519,
    kex::CURVE25519_PRE_RFC_8731,
    kex::DH_GEX_SHA1,
    kex::DH_GEX_SHA256,
    kex::DH_G1_SHA1,
    kex::DH_G14_SHA1,
    kex::DH_G14_SHA256,
    kex::DH_G15_SHA512,
    kex::DH_G16_SHA512,
    kex::DH_G17_SHA512,
    kex::DH_G18_SHA512,
    kex::ECDH_SHA2_NISTP256,
    kex::ECDH_SHA2_NISTP384,
    kex::ECDH_SHA2_NISTP521,
    kex::NONE,
];

/// Legacy-compatible cipher algorithms.
///
/// Includes modern ciphers like AES-GCM and ChaCha20-Poly1305, as well as
/// legacy CBC mode ciphers for compatibility with older devices.
pub static LEGACY_CIPHERS: &[cipher::Name] = &[
    cipher::CLEAR,
    cipher::NONE,
    cipher::AES_128_CTR,
    cipher::AES_192_CTR,
    cipher::AES_256_CTR,
    cipher::AES_256_GCM,
    cipher::AES_128_CBC,
    cipher::AES_192_CBC,
    cipher::AES_256_CBC,
    cipher::CHACHA20_POLY1305,
];

/// Legacy-compatible MAC (Message Authentication Code) algorithms.
///
/// Includes both standard HMAC variants and ETM (Encrypt-then-MAC) variants
/// for enhanced security.
pub const LEGACY_MAC_ALGORITHMS: &[mac::Name] = &[
    mac::NONE,
    mac::HMAC_SHA1,
    mac::HMAC_SHA256,
    mac::HMAC_SHA512,
    mac::HMAC_SHA1_ETM,
    mac::HMAC_SHA256_ETM,
    mac::HMAC_SHA512_ETM,
];

/// Compression algorithms shared by all security levels.
///
/// Includes ZLIB compression variants as well as no compression for
/// maximum compatibility.
pub const DEFAULT_COMPRESSION_ALGORITHMS: &[compression::Name] = &[
    compression::NONE,
    compression::ZLIB,
    compression::ZLIB_LEGACY,
];

/// Legacy-compatible host key algorithms.
///
/// Includes modern algorithms like Ed25519 and ECDSA, as well as legacy
/// RSA and DSA for compatibility with older devices.
pub const LEGACY_KEY_TYPES: &[Algorithm] = &[
    Algorithm::Dsa,
    Algorithm::Ecdsa {
        curve: EcdsaCurve::NistP256,
    },
    Algorithm::Ecdsa {
        curve: EcdsaCurve::NistP384,
    },
    Algorithm::Ecdsa {
        curve: EcdsaCurve::NistP521,
    },
    Algorithm::Ed25519,
    Algorithm::Rsa { hash: None },
    Algorithm::Rsa {
        hash: Some(HashAlg::Sha256),
    },
    Algorithm::Rsa {
        hash: Some(HashAlg::Sha512),
    },
    Algorithm::SkEcdsaSha2NistP256,
    Algorithm::SkEd25519,
];

/// Balanced key exchange algorithms (keeps broad compatibility while removing weak entries).
pub const BALANCED_KEX_ORDER: &[kex::Name] = &[
    kex::CURVE25519,
    kex::CURVE25519_PRE_RFC_8731,
    kex::ECDH_SHA2_NISTP256,
    kex::ECDH_SHA2_NISTP384,
    kex::ECDH_SHA2_NISTP521,
    kex::DH_G14_SHA256,
    kex::DH_G15_SHA512,
    kex::DH_G16_SHA512,
    kex::DH_G17_SHA512,
    kex::DH_G18_SHA512,
];

/// Balanced cipher algorithms.
pub static BALANCED_CIPHERS: &[cipher::Name] = &[
    cipher::AES_128_CTR,
    cipher::AES_192_CTR,
    cipher::AES_256_CTR,
    cipher::AES_256_GCM,
    cipher::CHACHA20_POLY1305,
];

/// Balanced MAC algorithms.
pub const BALANCED_MAC_ALGORITHMS: &[mac::Name] = &[
    mac::HMAC_SHA256,
    mac::HMAC_SHA512,
    mac::HMAC_SHA256_ETM,
    mac::HMAC_SHA512_ETM,
];

/// Balanced host key algorithms.
pub const BALANCED_KEY_TYPES: &[Algorithm] = &[
    Algorithm::Ecdsa {
        curve: EcdsaCurve::NistP256,
    },
    Algorithm::Ecdsa {
        curve: EcdsaCurve::NistP384,
    },
    Algorithm::Ecdsa {
        curve: EcdsaCurve::NistP521,
    },
    Algorithm::Ed25519,
    Algorithm::Rsa {
        hash: Some(HashAlg::Sha256),
    },
    Algorithm::Rsa {
        hash: Some(HashAlg::Sha512),
    },
];

/// Strict secure key exchange algorithms.
pub const SECURE_KEX_ORDER: &[kex::Name] = &[
    kex::CURVE25519,
    kex::CURVE25519_PRE_RFC_8731,
    kex::ECDH_SHA2_NISTP256,
    kex::ECDH_SHA2_NISTP384,
    kex::ECDH_SHA2_NISTP521,
];

/// Strict secure cipher algorithms.
pub static SECURE_CIPHERS: &[cipher::Name] = &[
    cipher::AES_256_GCM,
    cipher::CHACHA20_POLY1305,
    cipher::AES_256_CTR,
];

/// Strict secure MAC algorithms.
pub const SECURE_MAC_ALGORITHMS: &[mac::Name] =
    &[mac::HMAC_SHA512_ETM, mac::HMAC_SHA256_ETM, mac::HMAC_SHA512];

/// Strict secure host key algorithms.
pub const SECURE_KEY_TYPES: &[Algorithm] = &[
    Algorithm::Ed25519,
    Algorithm::Ecdsa {
        curve: EcdsaCurve::NistP256,
    },
    Algorithm::Ecdsa {
        curve: EcdsaCurve::NistP384,
    },
    Algorithm::Ecdsa {
        curve: EcdsaCurve::NistP521,
    },
    Algorithm::Rsa {
        hash: Some(HashAlg::Sha512),
    },
    Algorithm::Rsa {
        hash: Some(HashAlg::Sha256),
    },
];
