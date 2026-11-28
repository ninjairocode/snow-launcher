//! The Battle.net installer: is a file really Blizzard's installer,
//! unmodified (`snowctl check-installer`)? Which one to use, keeping the
//! last verified copy, and opening the official download page.

use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use snowlauncher_core::house::BATTLENET_PUBLISHER;
use snowlauncher_core::installer::OFFICIAL_DOWNLOAD_PAGE;
use snowlauncher_wine::installer_cache::{
    ChooseError, choose_installer, downloaded_installers, downloads_dir, kept_installer,
    verify_installer,
};
use snowlauncher_wine::signature::{Publisher, SignatureError};

/// Verifies `path` and returns the report, or an error saying why it must
/// not be run.
pub fn check(path: &Path) -> Result<String> {
    match verify_installer(path) {
        Ok(publisher) => Ok(render_ok(path, &publisher)),
        Err(error) => bail!("{}: do not run it: {error}{}", path.display(), hint(&error)),
    }
}

/// The installer to run (see [`choose_installer`]) and its report.
pub fn choose(named: Option<&Path>) -> Result<(PathBuf, String)> {
    match choose_installer(named) {
        Ok((path, publisher)) => {
            let report = render_ok(&path, &publisher);
            Ok((path, report))
        }
        Err(ChooseError::Refused { path, source }) => {
            bail!(
                "{}: do not run it: {source}{}",
                path.display(),
                hint(&source)
            )
        }
        Err(other) => Err(other.into()),
    }
}

/// `snowctl house installer status`.
pub fn status(out: &mut impl io::Write) -> Result<()> {
    let verdict = |path: &Path| match check(path) {
        Ok(_) => "ok, signed by Blizzard".to_owned(),
        Err(error) => format!("refused: {error}"),
    };
    match kept_installer() {
        Some(path) => writeln!(out, "Kept copy: {} ({})", path.display(), verdict(&path))?,
        None => writeln!(out, "Kept copy: none yet")?,
    }
    let downloads = downloads_dir()
        .map(|dir| downloaded_installers(&dir))
        .unwrap_or_default();
    if downloads.is_empty() {
        writeln!(out, "In Downloads: none")?;
    }
    for path in &downloads {
        writeln!(out, "In Downloads: {} ({})", path.display(), verdict(path))?;
    }
    writeln!(out, "Official download page: {OFFICIAL_DOWNLOAD_PAGE}")?;
    Ok(())
}

/// `snowctl house installer download`: the browser downloads it, not Snow.
pub fn open_download_page(out: &mut impl io::Write) -> Result<()> {
    let status = std::process::Command::new("xdg-open")
        .arg(OFFICIAL_DOWNLOAD_PAGE)
        .status()
        .context("could not run xdg-open")?;
    anyhow::ensure!(status.success(), "xdg-open failed ({status})");
    writeln!(
        out,
        "Opened {OFFICIAL_DOWNLOAD_PAGE} in the browser. Once it is downloaded, run \
         `snowctl house install-battlenet`: Snow finds it in Downloads and checks it."
    )?;
    Ok(())
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
    use std::fs;

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
