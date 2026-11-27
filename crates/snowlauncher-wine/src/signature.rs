//! Authenticode signature verification of Windows executables, used to
//! make sure the Battle.net installer is Blizzard's, unmodified, before
//! Snow Launcher runs it (project.md, section 5).
//!
//! Four checks, all required:
//! 1. **The file is what was signed**: the Authenticode hash of the
//!    executable equals the one inside the signature.
//! 2. **The signature is valid**: the signed attributes (which carry that
//!    hash) are signed with the signer certificate's key.
//! 3. **The certificate is trusted**: its chain reaches an authority in the
//!    system's trust store, and it is meant for code signing.
//! 4. **The publisher is the expected one** (see [`verify_publisher`]).
//!
//! The `authenticode` crate finds the signature and computes the file's
//! hash; the rest is parsed here with `cms`/`x509-cert`, so malformed input
//! is an error, never a crash. RSA verification uses `ring`.
//!
//! Not checked: certificate validity dates (a signed file stays valid after
//! its certificate expires, thanks to the timestamp countersignature, which
//! is not verified here) and revocation.

use std::fs;
use std::path::{Path, PathBuf};

use authenticode::{AttributeCertificateIterator, PeTrait, authenticode_digest};
use cms::cert::CertificateChoices;
use cms::content_info::ContentInfo;
use cms::signed_data::{SignedData, SignerIdentifier};
use const_oid::ObjectIdentifier;
use const_oid::db::{rfc4519, rfc5280, rfc5912, rfc6268};
use der::asn1::{Ia5StringRef, OctetString, PrintableStringRef, Utf8StringRef};
use der::{Decode, Encode, SliceReader};
use object::read::pe::{PeFile32, PeFile64};
use ring::signature::{
    RSA_PKCS1_2048_8192_SHA256, RSA_PKCS1_2048_8192_SHA384, RSA_PKCS1_2048_8192_SHA512,
    UnparsedPublicKey, VerificationAlgorithm,
};
use sha2::{Digest, Sha256, Sha384, Sha512};
use x509_cert::Certificate;
use x509_cert::ext::pkix::ExtendedKeyUsage;
use x509_cert::name::Name;

/// Longest certificate chain followed (signer, intermediates, root).
const MAX_CHAIN: usize = 6;

/// System trust stores, best first: the code-signing bundle (Fedora,
/// Arch: p11-kit) before the TLS-only ones (Debian, Ubuntu).
pub const TRUST_STORES: &[&str] = &[
    "/etc/pki/ca-trust/extracted/pem/objsign-ca-bundle.pem",
    "/etc/ca-certificates/extracted/objsign-ca-bundle.pem",
    "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem",
    "/etc/ssl/certs/ca-certificates.crt",
];

#[derive(Debug, thiserror::Error)]
pub enum SignatureError {
    #[error("not a Windows executable ({0})")]
    NotExecutable(String),
    #[error("the file is not signed")]
    NotSigned,
    #[error("malformed signature: {0}")]
    Malformed(String),
    #[error("unsupported signature: {0}")]
    Unsupported(String),
    #[error("the file was modified after it was signed")]
    FileModified,
    #[error("the signature is not valid")]
    SignatureInvalid,
    #[error("the signing certificate is not meant for code signing")]
    NotForCodeSigning,
    #[error("the certificate chain does not reach a trusted authority")]
    Untrusted,
    #[error("signed by {found:?}, not by {expected}")]
    WrongPublisher {
        found: Option<String>,
        expected: String,
    },
    #[error("could not read {}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("no system trust store found (looked for {})", TRUST_STORES.join(", "))]
    NoTrustStore,
}

/// Who signed a verified executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Publisher {
    /// `O=` of the signing certificate.
    pub organization: Option<String>,
    /// `CN=` of the signing certificate.
    pub common_name: Option<String>,
    /// Subjects from the signer up to the trusted root.
    pub chain: Vec<String>,
}

/// Checks 1–3 on the bytes of an executable. Check 4 is
/// [`verify_publisher`].
pub fn verify_executable(bytes: &[u8], roots: &[Certificate]) -> Result<Publisher, SignatureError> {
    if let Ok(pe) = PeFile32::parse(bytes) {
        return verify_pe(&pe, roots);
    }
    match PeFile64::parse(bytes) {
        Ok(pe) => verify_pe(&pe, roots),
        Err(error) => Err(SignatureError::NotExecutable(error.to_string())),
    }
}

/// Checks that `publisher` is `expected` (the certificate's organization).
pub fn verify_publisher(publisher: &Publisher, expected: &str) -> Result<(), SignatureError> {
    if publisher.organization.as_deref() == Some(expected) {
        Ok(())
    } else {
        Err(SignatureError::WrongPublisher {
            found: publisher.organization.clone(),
            expected: expected.to_owned(),
        })
    }
}

/// Loads the first system trust store found.
pub fn system_roots() -> Result<Vec<Certificate>, SignatureError> {
    let path = TRUST_STORES
        .iter()
        .map(Path::new)
        .find(|path| path.is_file())
        .ok_or(SignatureError::NoTrustStore)?;
    let pem = fs::read(path).map_err(|source| SignatureError::Io {
        path: path.to_owned(),
        source,
    })?;
    Certificate::load_pem_chain(&pem)
        .map_err(|error| SignatureError::Malformed(format!("{}: {error}", path.display())))
}

fn malformed(what: impl std::fmt::Display) -> SignatureError {
    SignatureError::Malformed(what.to_string())
}

fn verify_pe(pe: &dyn PeTrait, roots: &[Certificate]) -> Result<Publisher, SignatureError> {
    let attribute_certificate = AttributeCertificateIterator::new(pe)
        .map_err(|error| malformed(format!("{error:?}")))?
        .ok_or(SignatureError::NotSigned)?
        .next()
        .ok_or(SignatureError::NotSigned)?
        .map_err(|error| malformed(format!("{error:?}")))?;
    let signature = attribute_certificate
        .get_authenticode_signature()
        .map_err(|error| malformed(format!("{error:?}")))?;

    // 1. The file is what was signed.
    let file_hash = hash_for_digest(&signature.digest_algorithm().oid)?;
    let actual = file_hash
        .authenticode(pe)
        .ok_or_else(|| malformed("could not hash the executable"))?;
    if actual != signature.digest() {
        return Err(SignatureError::FileModified);
    }

    // Parse the signature ourselves (no panics on missing parts).
    let mut reader = SliceReader::new(attribute_certificate.data).map_err(malformed)?;
    let content_info = ContentInfo::decode(&mut reader).map_err(malformed)?;
    let signed_data: SignedData = content_info.content.decode_as().map_err(malformed)?;
    let certificates: Vec<&Certificate> = signed_data
        .certificates
        .as_ref()
        .ok_or_else(|| malformed("no certificates in the signature"))?
        .0
        .iter()
        .filter_map(|choice| match choice {
            CertificateChoices::Certificate(certificate) => Some(certificate),
            CertificateChoices::Other(_) => None,
        })
        .collect();
    let signer_info = signed_data
        .signer_infos
        .0
        .iter()
        .next()
        .ok_or_else(|| malformed("no signer"))?;

    // 2a. The signed attributes carry the hash of the signed content.
    let signer_hash = hash_for_digest(&signer_info.digest_alg.oid)?;
    let signed_attrs = signer_info
        .signed_attrs
        .as_ref()
        .ok_or_else(|| malformed("no signed attributes"))?;
    let message_digest = signed_attrs
        .iter()
        .find(|attr| attr.oid == rfc6268::ID_MESSAGE_DIGEST)
        .and_then(|attr| attr.values.iter().next())
        .ok_or_else(|| malformed("no message digest attribute"))?
        .decode_as::<OctetString>()
        .map_err(malformed)?;
    // Authenticode hashes the content without its outer tag and length.
    let content = signed_data
        .encap_content_info
        .econtent
        .as_ref()
        .ok_or_else(|| malformed("no signed content"))?
        .value();
    if signer_hash.digest(content) != message_digest.as_bytes() {
        return Err(SignatureError::SignatureInvalid);
    }

    // 2b. The signed attributes are signed with the signer's key.
    let signer = match &signer_info.sid {
        SignerIdentifier::IssuerAndSerialNumber(id) => certificates
            .iter()
            .copied()
            .find(|cert| {
                cert.tbs_certificate.issuer == id.issuer
                    && cert.tbs_certificate.serial_number == id.serial_number
            })
            .ok_or_else(|| malformed("the signer certificate is missing"))?,
        SignerIdentifier::SubjectKeyIdentifier(_) => {
            return Err(SignatureError::Unsupported(
                "signer identified by key id".to_owned(),
            ));
        }
    };
    let message = signed_attrs.to_der().map_err(malformed)?;
    let signature_hash = match signer_info.signature_algorithm.oid {
        // Plain RSA: the hash is the signer's digest algorithm.
        rfc5912::RSA_ENCRYPTION => signer_hash,
        other => hash_for_signature(&other)?,
    };
    if !verify_rsa(
        signer,
        signature_hash,
        &message,
        signer_info.signature.as_bytes(),
    )? {
        return Err(SignatureError::SignatureInvalid);
    }

    // 3. The certificate is meant for code signing and is trusted.
    if !is_code_signing(signer)? {
        return Err(SignatureError::NotForCodeSigning);
    }
    let chain = trusted_chain(signer, &certificates, roots)?;

    Ok(Publisher {
        organization: name_attribute(&signer.tbs_certificate.subject, rfc4519::O),
        common_name: name_attribute(&signer.tbs_certificate.subject, rfc4519::CN),
        chain,
    })
}

/// Follows issuers from `signer` through the embedded certificates until a
/// trusted root signs one of them. Every link's signature is checked.
fn trusted_chain(
    signer: &Certificate,
    embedded: &[&Certificate],
    roots: &[Certificate],
) -> Result<Vec<String>, SignatureError> {
    let mut chain = vec![signer.tbs_certificate.subject.to_string()];
    let mut current = signer;
    for _ in 0..MAX_CHAIN {
        let issuer = &current.tbs_certificate.issuer;
        for root in roots
            .iter()
            .filter(|root| &root.tbs_certificate.subject == issuer)
        {
            if signed_by(current, root)? {
                chain.push(root.tbs_certificate.subject.to_string());
                return Ok(chain);
            }
        }
        let mut next = None;
        for candidate in embedded.iter().copied() {
            let distinct = candidate.tbs_certificate.subject != current.tbs_certificate.subject;
            if distinct
                && &candidate.tbs_certificate.subject == issuer
                && signed_by(current, candidate)?
            {
                next = Some(candidate);
                break;
            }
        }
        let Some(next) = next else {
            return Err(SignatureError::Untrusted);
        };
        chain.push(next.tbs_certificate.subject.to_string());
        current = next;
    }
    Err(SignatureError::Untrusted)
}

/// Whether `certificate` was signed by `issuer`'s key.
fn signed_by(certificate: &Certificate, issuer: &Certificate) -> Result<bool, SignatureError> {
    let hash = hash_for_signature(&certificate.signature_algorithm.oid)?;
    let message = certificate.tbs_certificate.to_der().map_err(malformed)?;
    let signature = certificate
        .signature
        .as_bytes()
        .ok_or_else(|| malformed("certificate signature with unused bits"))?;
    verify_rsa(issuer, hash, &message, signature)
}

fn verify_rsa(
    key_owner: &Certificate,
    hash: Hash,
    message: &[u8],
    signature: &[u8],
) -> Result<bool, SignatureError> {
    let spki = &key_owner.tbs_certificate.subject_public_key_info;
    if spki.algorithm.oid != rfc5912::RSA_ENCRYPTION {
        return Err(SignatureError::Unsupported(format!(
            "public key algorithm {}",
            spki.algorithm.oid
        )));
    }
    let key = spki
        .subject_public_key
        .as_bytes()
        .ok_or_else(|| malformed("public key with unused bits"))?;
    Ok(UnparsedPublicKey::new(hash.rsa(), key)
        .verify(message, signature)
        .is_ok())
}

fn is_code_signing(certificate: &Certificate) -> Result<bool, SignatureError> {
    let Some(extensions) = &certificate.tbs_certificate.extensions else {
        return Ok(false);
    };
    let Some(eku) = extensions
        .iter()
        .find(|ext| ext.extn_id == rfc5280::ID_CE_EXT_KEY_USAGE)
    else {
        return Ok(false);
    };
    let usages = ExtendedKeyUsage::from_der(eku.extn_value.as_bytes()).map_err(malformed)?;
    Ok(usages.0.contains(&rfc5280::ID_KP_CODE_SIGNING))
}

/// First value of `oid` in a name (`O=`, `CN=`...), as text.
fn name_attribute(name: &Name, oid: ObjectIdentifier) -> Option<String> {
    name.0
        .iter()
        .flat_map(|rdn| rdn.0.iter())
        .find(|atv| atv.oid == oid)
        .and_then(|atv| {
            let value = &atv.value;
            Utf8StringRef::try_from(value)
                .map(|s| s.as_str().to_owned())
                .or_else(|_| PrintableStringRef::try_from(value).map(|s| s.as_str().to_owned()))
                .or_else(|_| Ia5StringRef::try_from(value).map(|s| s.as_str().to_owned()))
                .ok()
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hash {
    Sha256,
    Sha384,
    Sha512,
}

impl Hash {
    fn digest(self, data: &[u8]) -> Vec<u8> {
        match self {
            Self::Sha256 => Sha256::digest(data).to_vec(),
            Self::Sha384 => Sha384::digest(data).to_vec(),
            Self::Sha512 => Sha512::digest(data).to_vec(),
        }
    }

    /// Authenticode hash of an executable.
    fn authenticode(self, pe: &dyn PeTrait) -> Option<Vec<u8>> {
        fn run<D: Digest + digest_update::Update>(
            pe: &dyn PeTrait,
            mut hasher: D,
        ) -> Option<Vec<u8>> {
            authenticode_digest(pe, &mut hasher).ok()?;
            Some(hasher.finalize().to_vec())
        }
        match self {
            Self::Sha256 => run(pe, Sha256::new()),
            Self::Sha384 => run(pe, Sha384::new()),
            Self::Sha512 => run(pe, Sha512::new()),
        }
    }

    fn rsa(self) -> &'static dyn VerificationAlgorithm {
        match self {
            Self::Sha256 => &RSA_PKCS1_2048_8192_SHA256,
            Self::Sha384 => &RSA_PKCS1_2048_8192_SHA384,
            Self::Sha512 => &RSA_PKCS1_2048_8192_SHA512,
        }
    }
}

/// The `Update` trait `authenticode_digest` expects, from the same
/// `digest` crate `sha2` implements.
mod digest_update {
    pub use sha2::digest::Update;
}

fn hash_for_digest(oid: &ObjectIdentifier) -> Result<Hash, SignatureError> {
    match *oid {
        rfc5912::ID_SHA_256 => Ok(Hash::Sha256),
        rfc5912::ID_SHA_384 => Ok(Hash::Sha384),
        rfc5912::ID_SHA_512 => Ok(Hash::Sha512),
        other => Err(SignatureError::Unsupported(format!(
            "digest algorithm {other}"
        ))),
    }
}

fn hash_for_signature(oid: &ObjectIdentifier) -> Result<Hash, SignatureError> {
    match *oid {
        rfc5912::SHA_256_WITH_RSA_ENCRYPTION => Ok(Hash::Sha256),
        rfc5912::SHA_384_WITH_RSA_ENCRYPTION => Ok(Hash::Sha384),
        rfc5912::SHA_512_WITH_RSA_ENCRYPTION => Ok(Hash::Sha512),
        other => Err(SignatureError::Unsupported(format!(
            "signature algorithm {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_files_that_are_not_executables() {
        assert!(matches!(
            verify_executable(b"definitely not a PE file", &[]),
            Err(SignatureError::NotExecutable(_))
        ));
    }

    #[test]
    fn knows_the_usual_algorithms_and_refuses_sha1() {
        assert_eq!(hash_for_digest(&rfc5912::ID_SHA_256).unwrap(), Hash::Sha256);
        assert_eq!(
            hash_for_signature(&rfc5912::SHA_384_WITH_RSA_ENCRYPTION).unwrap(),
            Hash::Sha384
        );
        assert!(matches!(
            hash_for_digest(&rfc5912::ID_SHA_1),
            Err(SignatureError::Unsupported(_))
        ));
    }

    #[test]
    fn publisher_must_match_exactly() {
        let publisher = Publisher {
            organization: Some("Blizzard Entertainment, Inc.".to_owned()),
            common_name: None,
            chain: Vec::new(),
        };
        assert!(verify_publisher(&publisher, "Blizzard Entertainment, Inc.").is_ok());
        assert!(verify_publisher(&publisher, "Blizzard Entertainment").is_err());
    }

    /// The real installer cannot be shipped in the repository (it is
    /// Blizzard's). Point this at a downloaded copy:
    /// `SNOW_TEST_INSTALLER=~/Downloads/Battle.net-Setup.exe cargo test -- --ignored`
    fn real_installer() -> Vec<u8> {
        let path = std::env::var("SNOW_TEST_INSTALLER")
            .expect("set SNOW_TEST_INSTALLER to a Battle.net-Setup.exe");
        fs::read(path).unwrap()
    }

    #[test]
    #[ignore = "needs SNOW_TEST_INSTALLER and the system trust store"]
    fn real_installer_is_blizzards() {
        let publisher = verify_executable(&real_installer(), &system_roots().unwrap()).unwrap();
        verify_publisher(&publisher, "Blizzard Entertainment, Inc.").unwrap();
        assert!(publisher.chain.len() >= 2, "{:?}", publisher.chain);
    }

    #[test]
    #[ignore = "needs SNOW_TEST_INSTALLER and the system trust store"]
    fn a_single_changed_byte_is_caught() {
        let mut bytes = real_installer();
        let middle = bytes.len() / 3;
        bytes[middle] ^= 0x01;
        assert!(matches!(
            verify_executable(&bytes, &system_roots().unwrap()),
            Err(SignatureError::FileModified)
        ));
    }

    #[test]
    #[ignore = "needs SNOW_TEST_INSTALLER"]
    fn without_trusted_roots_it_is_untrusted() {
        assert!(matches!(
            verify_executable(&real_installer(), &[]),
            Err(SignatureError::Untrusted)
        ));
    }

    #[test]
    #[ignore = "needs SNOW_TEST_INSTALLER and the system trust store"]
    fn a_forged_signature_is_caught() {
        let mut bytes = real_installer();
        // Find the signer's RSA signature inside the file and change it.
        let signature = {
            let pe = PeFile32::parse(bytes.as_slice()).unwrap();
            let certificate = AttributeCertificateIterator::new(&pe)
                .unwrap()
                .unwrap()
                .next()
                .unwrap()
                .unwrap();
            let mut reader = SliceReader::new(certificate.data).unwrap();
            let content_info = ContentInfo::decode(&mut reader).unwrap();
            let signed_data: SignedData = content_info.content.decode_as().unwrap();
            let signer = signed_data.signer_infos.0.iter().next().unwrap();
            signer.signature.as_bytes().to_vec()
        };
        let at = bytes
            .windows(signature.len())
            .position(|window| window == signature.as_slice())
            .unwrap();
        bytes[at + 10] ^= 0x01;
        assert!(matches!(
            verify_executable(&bytes, &system_roots().unwrap()),
            Err(SignatureError::SignatureInvalid)
        ));
    }
}
