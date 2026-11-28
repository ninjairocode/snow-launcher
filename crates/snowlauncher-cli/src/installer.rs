//! The Battle.net installer: is a file really Blizzard's installer,
//! unmodified (`snowctl check-installer`)? Which one to use, keeping the
//! last verified copy, and opening the official download page.

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use snowlauncher_core::house::BATTLENET_PUBLISHER;
use snowlauncher_core::installer::{OFFICIAL_DOWNLOAD_PAGE, installer_candidates};
use snowlauncher_wine::installer_cache::{
    downloaded_installers, downloads_dir, keep_verified, kept_installer, kept_installer_path,
};
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

/// The installer to run: the named one, or else the newest download that
/// passes the check, or else the kept copy. A verified download replaces
/// the kept copy. Returns the path and its check report.
pub fn choose(named: Option<&Path>) -> Result<(PathBuf, String)> {
    let downloads = downloads_dir()
        .map(|dir| downloaded_installers(&dir))
        .unwrap_or_default();
    choose_from(
        named,
        &downloads,
        kept_installer().as_deref(),
        kept_installer_path().as_deref(),
        check,
    )
}

/// [`choose`] with its inputs and the check given, for tests.
fn choose_from(
    named: Option<&Path>,
    downloads: &[PathBuf],
    kept: Option<&Path>,
    keep_to: Option<&Path>,
    check: impl Fn(&Path) -> Result<String>,
) -> Result<(PathBuf, String)> {
    let candidates = installer_candidates(named, downloads, kept);
    let mut problems = Vec::new();
    for candidate in &candidates {
        match check(candidate) {
            Ok(report) => {
                if let Some(keep_to) = keep_to {
                    // Keeping a copy is a convenience: never a reason to fail.
                    let _ = keep_verified(candidate, keep_to);
                }
                return Ok((candidate.clone(), report));
            }
            // A named file that fails stops here: nothing else is tried.
            Err(error) if named.is_some() => return Err(error),
            Err(error) => problems.push(format!("{error}")),
        }
    }
    if problems.is_empty() {
        bail!(
            "no Battle.net installer found: download it from {OFFICIAL_DOWNLOAD_PAGE} \
             (`snowctl house installer download` opens it) or pass --installer PATH"
        );
    }
    bail!(
        "no installer passed the check:\n  {}\nDownload it again from {OFFICIAL_DOWNLOAD_PAGE}",
        problems.join("\n  ")
    )
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

    /// Accepts files whose content is "good".
    fn fake_check(path: &Path) -> Result<String> {
        if fs::read_to_string(path).unwrap_or_default() == "good" {
            Ok(format!("ok {}", path.display()))
        } else {
            bail!("{}: refused", path.display())
        }
    }

    #[test]
    fn the_first_good_download_wins_and_is_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let bad = tmp.path().join("Battle.net-Setup (1).exe");
        let good = tmp.path().join("Battle.net-Setup.exe");
        fs::write(&bad, "tampered").unwrap();
        fs::write(&good, "good").unwrap();
        let keep = tmp.path().join("cache/Battle.net-Setup.exe");
        let downloads = [bad, good.clone()];

        let (chosen, _) = choose_from(None, &downloads, None, Some(&keep), fake_check).unwrap();
        assert_eq!(chosen, good);
        assert_eq!(fs::read_to_string(&keep).unwrap(), "good");

        // Later, with nothing in Downloads, the kept copy is used.
        let (chosen, _) = choose_from(None, &[], Some(&keep), Some(&keep), fake_check).unwrap();
        assert_eq!(chosen, keep);
    }

    #[test]
    fn a_named_file_that_fails_is_not_replaced_by_another() {
        let tmp = tempfile::tempdir().unwrap();
        let named = tmp.path().join("mine.exe");
        let good = tmp.path().join("Battle.net-Setup.exe");
        fs::write(&named, "tampered").unwrap();
        fs::write(&good, "good").unwrap();
        let keep = tmp.path().join("cache/Battle.net-Setup.exe");
        let error = choose_from(Some(&named), &[good], None, Some(&keep), fake_check)
            .unwrap_err()
            .to_string();
        assert!(error.contains("mine.exe: refused"), "{error}");
        assert!(!keep.exists(), "nothing kept from a refused file");
    }

    #[test]
    fn nothing_found_points_to_the_download_page() {
        let error = choose_from(None, &[], None, None, fake_check)
            .unwrap_err()
            .to_string();
        assert!(error.contains(OFFICIAL_DOWNLOAD_PAGE), "{error}");
    }
}
