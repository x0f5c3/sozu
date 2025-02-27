use std::{collections::HashSet, fmt, str::FromStr};

use hex::FromHex;
use serde::de::{self, Visitor};
use sha2::{Digest, Sha256};
use x509_parser::{
    extensions::{GeneralName, ParsedExtension},
    oid_registry::{OID_X509_COMMON_NAME, OID_X509_EXT_SUBJECT_ALT_NAME},
    pem::{parse_x509_pem, Pem},
};

use crate::proto::command::TlsVersion;

// -----------------------------------------------------------------------------
// CertificateError

#[derive(thiserror::Error, Clone, Debug)]
pub enum CertificateError {
    #[error("Could not parse PEM certificate from bytes: {0}")]
    InvalidCertificate(String),
    #[error("failed to parse tls version '{0}'")]
    InvalidTlsVersion(String),
}

// -----------------------------------------------------------------------------
// parse

/// parse a pem file encoded as binary and convert it into the right structure
/// (a.k.a [`Pem`])
pub fn parse(certificate: &[u8]) -> Result<Pem, CertificateError> {
    let (_, pem) = parse_x509_pem(certificate)
        .map_err(|err| CertificateError::InvalidCertificate(err.to_string()))?;

    Ok(pem)
}

// -----------------------------------------------------------------------------
// get_cn_and_san_attributes

/// Retrieve from the [`Pem`] structure the common name (a.k.a `CN`) and the
/// subject alternate names (a.k.a `SAN`)
pub fn get_cn_and_san_attributes(pem: &Pem) -> Result<HashSet<String>, CertificateError> {
    let x509 = pem
        .parse_x509()
        .map_err(|err| CertificateError::InvalidCertificate(err.to_string()))?;

    let mut names: HashSet<String> = HashSet::new();
    for name in x509.subject().iter_by_oid(&OID_X509_COMMON_NAME) {
        names.insert(
            name.as_str()
                .map(String::from)
                .unwrap_or_else(|_| String::from_utf8_lossy(name.as_slice()).to_string()),
        );
    }

    for extension in x509.extensions() {
        if extension.oid == OID_X509_EXT_SUBJECT_ALT_NAME {
            if let ParsedExtension::SubjectAlternativeName(san) = extension.parsed_extension() {
                for name in &san.general_names {
                    if let GeneralName::DNSName(name) = name {
                        names.insert(name.to_string());
                    }
                }
            }
        }
    }
    Ok(names)
}

// -----------------------------------------------------------------------------
// TlsVersion

impl FromStr for TlsVersion {
    type Err = CertificateError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "SSL_V2" => Ok(TlsVersion::SslV2),
            "SSL_V3" => Ok(TlsVersion::SslV3),
            "TLSv1" => Ok(TlsVersion::TlsV10),
            "TLS_V11" => Ok(TlsVersion::TlsV11),
            "TLS_V12" => Ok(TlsVersion::TlsV12),
            "TLS_V13" => Ok(TlsVersion::TlsV13),
            _ => Err(CertificateError::InvalidTlsVersion(s.to_string())),
        }
    }
}

// -----------------------------------------------------------------------------
// Fingerprint

//FIXME: make fixed size depending on hash algorithm
/// A TLS certificates, encoded in bytes
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fingerprint(pub Vec<u8>);

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "CertificateFingerprint({})", hex::encode(&self.0))
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "{}", hex::encode(&self.0))
    }
}

impl serde::Serialize for Fingerprint {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&hex::encode(&self.0))
    }
}

struct FingerprintVisitor;

impl<'de> Visitor<'de> for FingerprintVisitor {
    type Value = Fingerprint;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("the certificate fingerprint must be in hexadecimal format")
    }

    fn visit_str<E>(self, value: &str) -> Result<Fingerprint, E>
    where
        E: de::Error,
    {
        FromHex::from_hex(value)
            .map_err(|e| E::custom(format!("could not deserialize hex: {e:?}")))
            .map(Fingerprint)
    }
}

impl<'de> serde::Deserialize<'de> for Fingerprint {
    fn deserialize<D>(deserializer: D) -> Result<Fingerprint, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        deserializer.deserialize_str(FingerprintVisitor {})
    }
}

/// Compute fingerprint from decoded pem as binary value
pub fn calculate_fingerprint_from_der(certificate: &[u8]) -> Vec<u8> {
    Sha256::digest(certificate).iter().cloned().collect()
}

/// Compute fingerprint from a certificate that is encoded in pem format
pub fn calculate_fingerprint(certificate: &[u8]) -> Result<Vec<u8>, CertificateError> {
    let parsed_certificate = parse(certificate)
        .map_err(|parse_error| CertificateError::InvalidCertificate(parse_error.to_string()))?;

    Ok(calculate_fingerprint_from_der(&parsed_certificate.contents))
}

pub fn split_certificate_chain(mut chain: String) -> Vec<String> {
    let mut v = Vec::new();

    let end = "-----END CERTIFICATE-----";
    loop {
        if let Some(sz) = chain.find(end) {
            let cert: String = chain.drain(..sz + end.len()).collect();
            v.push(cert.trim().to_string());
            continue;
        }

        break;
    }

    v
}
