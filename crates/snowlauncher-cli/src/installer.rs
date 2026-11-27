//! `snowctl check-installer`: is this really Blizzard's Battle.net
//! installer, unmodified? Read-only.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use snowlauncher_core::house::BATTLENET_PUBLISHER;
use snowlauncher_wine::signature::{
    Publisher, SignatureError, system_roots, verify_executable, verify_publisher,
};

/// Verifies `path` and returns the report, or an error saying why it must
/// not be run.
pub fn check(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("could not read {}", path.display()))?;
    let roots = system_roots()?;
    match verify_executable(&bytes, &roots)
        .and_then(|publisher| verify_publisher(&publisher, BATTLENET_PUBLISHER).map(|()| publisher))
    {
        Ok(publisher) => Ok(render_ok(path, &publisher)),
        Err(error) => bail!("{}: do not run it: {error}{}", path.display(), hint(&error)),
    }
}

fn hint(error: &SignatureError) -> &'static str {
    match error {
        SignatureError::FileModified | SignatureError::SignatureInvalid => {
            " (the download may be corrupted or tampered with; download it again from Blizzard's site)"
        }
        SignatureError::NotSigned | SignatureError::WrongPublisher { .. } => {
            " (this is not the official Battle.net installer)"
        }
        _ => "",
    }
}

fn render_ok(path: &Path, publisher: &Publisher) -> String {
    let mut out = String::new();
    // Writing to a String cannot fail, so the results are ignored.
    let _ = writeln!(out, "Battle.net installer: {}", path.display());
    let _ = writeln!(out, "  ok  the file is exactly what was signed");
    let _ = writeln!(out, "  ok  the signature is valid");
    let root = publisher.chain.last().map_or("?", String::as_str);
    let _ = writeln!(
        out,
        "  ok  the certificate chain reaches a trusted authority: {root}"
    );
    let _ = writeln!(out, "  ok  signed by {BATTLENET_PUBLISHER}");
    let _ = writeln!(
        out,
        "Safe to run. (Not checked: certificate dates and revocation.)"
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_names_publisher_and_root() {
        let publisher = Publisher {
            organization: Some(BATTLENET_PUBLISHER.to_owned()),
            common_name: Some(BATTLENET_PUBLISHER.to_owned()),
            chain: vec![
                "CN=signer".to_owned(),
                "CN=DigiCert Trusted Root G4".to_owned(),
            ],
        };
        let out = render_ok(Path::new("/x/Battle.net-Setup.exe"), &publisher);
        assert!(out.contains("trusted authority: CN=DigiCert Trusted Root G4"));
        assert!(out.contains("signed by Blizzard Entertainment, Inc."));
    }

    #[test]
    fn a_text_file_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let fake = tmp.path().join("Battle.net-Setup.exe");
        fs::write(&fake, "not an installer").unwrap();
        let error = check(&fake).unwrap_err().to_string();
        assert!(error.contains("do not run it"), "{error}");
    }
}
