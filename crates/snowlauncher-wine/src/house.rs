//! Carrying out a house plan (`snowlauncher_core::house`), and undoing it.
//!
//! Every step is **verified by its result** (the prefix opens, the registry
//! says Windows 11, the DPI values are there), not by exit codes. If a step
//! fails, what this run created is removed again. Deleting is fenced in:
//! only inside Snow Launcher's prefixes folder, never through symlinks, and
//! never the games (project.md, section 9.2).

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use directories::BaseDirs;
use snowlauncher_core::house::{
    DPI_REGISTRY_FILES, DPI_REGISTRY_KEYS, HOUSE_ENV, HOUSE_RECORD_SCHEMA, HouseInputs,
    HouseRecord, HouseStep, RemoveStep, WINDOWS_PRODUCT_FILE, WINDOWS_PRODUCT_NAME,
    WINDOWS_VERSION,
};
use snowlauncher_core::registry;

use crate::prefix::WinePrefix;

/// Runs Windows programs inside the house's prefix. Faked in tests.
pub trait WineRunner {
    /// Runs `args` (program first) and waits for the prefix to settle.
    /// `Ok(false)` if it exited with an error.
    fn run(&mut self, args: &[String]) -> io::Result<bool>;
}

/// The real thing: `umu-run` with the house's prefix and runtime.
pub struct UmuRunner {
    pub umu_run: PathBuf,
    pub prefix: PathBuf,
    pub runtime: PathBuf,
    /// Output of the Wine tools goes here, for diagnosis.
    pub log: PathBuf,
}

impl UmuRunner {
    /// `umu-run <args>` in the house, output appended to the log.
    fn command(&self, args: &[impl AsRef<std::ffi::OsStr>]) -> io::Result<Command> {
        if let Some(dir) = self.log.parent() {
            fs::create_dir_all(dir)?;
        }
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log)?;
        let mut command = Command::new(&self.umu_run);
        command
            .args(args)
            .env("WINEPREFIX", &self.prefix)
            .env("GAMEID", "umu-default")
            .env("PROTONPATH", &self.runtime)
            .envs(HOUSE_ENV.iter().copied())
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        Ok(command)
    }

    /// Starts a program in the house without waiting for it. Proton only
    /// returns once everything in the prefix has exited, which for an
    /// installer that opens Battle.net means "when Battle.net closes".
    ///
    /// It gets its own process group, so closing the terminal `snowctl`
    /// ran in does not take Battle.net down with it.
    pub fn spawn(&self, args: &[impl AsRef<std::ffi::OsStr>]) -> io::Result<std::process::Child> {
        use std::os::unix::process::CommandExt;
        self.command(args)?.process_group(0).spawn()
    }

    /// Runs a program alongside what is already running in the house and
    /// waits for it (`PROTON_VERB=runinprefix`; the default verb would wait
    /// for the whole prefix). E.g. `Battle.net.exe` again, which brings the
    /// open window back, even from the tray (experiment E9).
    pub fn run_alongside(&self, args: &[impl AsRef<std::ffi::OsStr>]) -> io::Result<bool> {
        let mut command = self.command(args)?;
        command.env("PROTON_VERB", "runinprefix");
        Ok(command.status()?.success())
    }
}

impl WineRunner for UmuRunner {
    fn run(&mut self, args: &[String]) -> io::Result<bool> {
        Ok(self.command(args)?.status()?.success())
    }
}

/// Where the house lives, as callers need it (canonical prefix).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HouseRecordView {
    pub prefix: PathBuf,
    pub library: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum OpenHouseError {
    #[error("could not determine the state directory")]
    NoStateDir,
    #[error("there is no house yet")]
    NoHouse,
    #[error("the house's prefix {} is missing", .0.display())]
    PrefixMissing(PathBuf),
    #[error("umu-run not found: install umu-launcher (or Lutris, which bundles it)")]
    NoUmuRun,
    #[error(transparent)]
    Record(#[from] HouseError),
}

/// Reads the house's record and prepares a runner for it, with the
/// house's environment. Shared by `snowctl` and the GUI.
pub fn open_house() -> Result<(HouseRecordView, UmuRunner), OpenHouseError> {
    let record_file = record_path().ok_or(OpenHouseError::NoStateDir)?;
    let record = load_record(&record_file)?.ok_or(OpenHouseError::NoHouse)?;
    let prefix = fs::canonicalize(&record.prefix)
        .map_err(|_| OpenHouseError::PrefixMissing(record.prefix.clone()))?;
    let runner = UmuRunner {
        umu_run: crate::runtime::find_umu_run().ok_or(OpenHouseError::NoUmuRun)?,
        prefix: prefix.clone(),
        runtime: record.runtime_path.clone(),
        log: open_log_path().ok_or(OpenHouseError::NoStateDir)?,
    };
    Ok((
        HouseRecordView {
            prefix,
            library: record.library,
        },
        runner,
    ))
}

/// What [`run_battlenet`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattlenetRun {
    /// Battle.net was closed and is starting.
    Started,
    /// Battle.net was open; the request went to it (window back, or a
    /// game launched), as validated in experiment E9.
    PassedToRunning,
}

/// The house's main Battle.net process, if running.
pub fn battlenet_pid(prefix: &Path) -> Result<Option<u32>, HouseError> {
    use snowlauncher_core::process::{ProcessRole, ProcessTable, processes_in_prefix};
    let processes = crate::procfs::ProcfsProcessTable::new()
        .processes()
        .map_err(|error| HouseError::Step {
            step: "list processes".to_owned(),
            problem: error.to_string(),
        })?;
    Ok(processes_in_prefix(&processes, prefix)
        .find(|process| ProcessRole::of(process) == ProcessRole::BattleNet)
        .map(|process| process.pid.0))
}

/// Runs `Battle.net.exe <args>` in the house: starts it if closed, or
/// hands the request to the running one. No arguments opens (or brings
/// back) the window; `--exec=launch <code>` opens a game (E5).
pub fn run_battlenet(runner: &UmuRunner, args: &[&str]) -> Result<BattlenetRun, HouseError> {
    let exe = runner.prefix.join(snowlauncher_core::house::BATTLENET_EXE);
    if !exe.is_file() {
        return Err(HouseError::Step {
            step: "open Battle.net".to_owned(),
            problem: "it is not installed in the house".to_owned(),
        });
    }
    let mut command: Vec<&std::ffi::OsStr> = vec![exe.as_os_str()];
    command.extend(args.iter().map(std::ffi::OsStr::new));
    let io_problem = |error: io::Error| HouseError::Step {
        step: "open Battle.net".to_owned(),
        problem: error.to_string(),
    };
    if battlenet_pid(&runner.prefix)?.is_some() {
        runner.run_alongside(&command).map_err(io_problem)?;
        Ok(BattlenetRun::PassedToRunning)
    } else {
        // Battle.net keeps running after we return. A thread waits for it
        // so that, once it exits, it does not linger as a zombie in a
        // long-running caller (the GUI, snowd).
        let mut child = runner.spawn(&command).map_err(io_problem)?;
        std::thread::spawn(move || child.wait());
        Ok(BattlenetRun::Started)
    }
}

/// `--exec=launch <code>`, the argument that opens a game (E5).
pub fn launch_argument(code: &str) -> String {
    format!("--exec=launch {code}")
}

/// Total size of the files under `path`, without following symlinks.
/// Unreadable entries are skipped: this is for progress, not accounting.
pub fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => dir_size(&entry.path()),
            Ok(kind) if kind.is_file() => entry.metadata().map_or(0, |meta| meta.len()),
            _ => 0,
        })
        .sum()
}

#[derive(Debug, thiserror::Error)]
pub enum HouseError {
    #[error("{step}: {problem}")]
    Step { step: String, problem: String },

    #[error("could not {what} {}", path.display())]
    Io {
        what: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("refusing to delete {}: {reason}", path.display())]
    Refused { path: PathBuf, reason: &'static str },
}

fn io_err<'a>(what: &'static str, path: &'a Path) -> impl FnOnce(io::Error) -> HouseError + 'a {
    move |source| HouseError::Io {
        what,
        path: path.to_owned(),
        source,
    }
}

/// Carries out `steps`, calling `on_step` before each one. On failure,
/// removes what this run created and returns the error.
pub fn create(
    steps: &[HouseStep],
    inputs: &HouseInputs,
    runtime_path: &Path,
    prefixes_root: &Path,
    runner: &mut impl WineRunner,
    mut on_step: impl FnMut(&HouseStep),
) -> Result<HouseRecord, HouseError> {
    let mut created_prefix = false;
    let mut created_library = false;
    let result = steps.iter().try_for_each(|step| {
        on_step(step);
        run_step(
            step,
            inputs,
            runner,
            &mut created_prefix,
            &mut created_library,
        )
    });
    if let Err(error) = result {
        if created_prefix {
            // Best effort; the original error is what matters.
            let _ = remove_prefix(&inputs.prefix, prefixes_root);
        }
        if created_library {
            let _ = fs::remove_dir(&inputs.library);
        }
        return Err(error);
    }
    Ok(HouseRecord {
        schema: HOUSE_RECORD_SCHEMA,
        prefix: inputs.prefix.clone(),
        runtime_name: inputs.runtime_name.clone(),
        runtime_path: runtime_path.to_owned(),
        library: inputs.library.clone(),
        library_created: created_library,
        dpi: inputs.dpi.dpi,
        created_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs()),
    })
}

fn run_step(
    step: &HouseStep,
    inputs: &HouseInputs,
    runner: &mut impl WineRunner,
    created_prefix: &mut bool,
    created_library: &mut bool,
) -> Result<(), HouseError> {
    let fail = |problem: String| HouseError::Step {
        step: step.to_string(),
        problem,
    };
    let prefix = &inputs.prefix;
    match step {
        HouseStep::CreatePrefix { .. } => {
            if let Some(parent) = prefix.parent() {
                fs::create_dir_all(parent).map_err(io_err("create", parent))?;
            }
            *created_prefix = true;
            let ran = run(runner, &["wineboot", "-u"]).map_err(fail)?;
            WinePrefix::open(prefix).map_err(|error| {
                fail(format!(
                    "the prefix is not usable ({error}); wineboot {ran}"
                ))
            })?;
        }
        HouseStep::SetWindowsVersion(version) => {
            let ran = run(runner, &["winecfg", "-v", version]).map_err(fail)?;
            if *version == WINDOWS_VERSION {
                let (file, section, name) = WINDOWS_PRODUCT_FILE;
                let found = read_value(prefix, file, section, name);
                if found.as_deref().and_then(registry::string) != Some(WINDOWS_PRODUCT_NAME) {
                    return Err(fail(format!(
                        "the registry does not say {WINDOWS_PRODUCT_NAME} (found {found:?}); winecfg {ran}"
                    )));
                }
            }
        }
        HouseStep::SetDpi { dpi } => {
            let reg_file = prefix.join("snow-launcher-dpi.reg");
            fs::write(&reg_file, dpi_reg_file(*dpi)).map_err(io_err("write", &reg_file))?;
            let windows_path = windows_path(&reg_file)
                .ok_or_else(|| fail("the prefix path is not valid UTF-8".to_owned()))?;
            let ran = run(runner, &["regedit", "/S", &windows_path]);
            let _ = fs::remove_file(&reg_file);
            let ran = ran.map_err(fail)?;
            for (file, section, name) in DPI_REGISTRY_FILES {
                let found = read_value(prefix, file, section, name);
                if found.as_deref().and_then(registry::dword) != Some(*dpi) {
                    return Err(fail(format!(
                        "{name} in {file} is {found:?}, not {dpi}; regedit {ran}"
                    )));
                }
            }
        }
        HouseStep::CreateLibrary { path } => {
            fs::create_dir_all(path).map_err(io_err("create", path))?;
            *created_library = true;
        }
        HouseStep::MapDrive { letter, target } => {
            let link = prefix
                .join("dosdevices")
                .join(format!("{}:", letter.to_ascii_lowercase()));
            symlink(target, &link).map_err(io_err("create the drive link", &link))?;
        }
    }
    Ok(())
}

/// Runs a Wine tool; describes the outcome for error messages.
fn run(runner: &mut impl WineRunner, args: &[&str]) -> Result<&'static str, String> {
    let args: Vec<String> = args.iter().map(|&arg| arg.to_owned()).collect();
    match runner.run(&args) {
        Ok(true) => Ok("exited normally"),
        Ok(false) => Ok("exited with an error"),
        Err(error) => Err(format!("could not run {}: {error}", args.join(" "))),
    }
}

fn read_value(prefix: &Path, file: &str, section: &str, name: &str) -> Option<String> {
    let text = fs::read_to_string(prefix.join(file)).ok()?;
    registry::value(&text, section, name).map(str::to_owned)
}

/// A `.reg` file setting the three DPI values (REGEDIT4, CRLF).
fn dpi_reg_file(dpi: u32) -> String {
    let mut text = String::from("REGEDIT4\r\n");
    for key in DPI_REGISTRY_KEYS {
        text.push_str(&format!("\r\n[{key}]\r\n\"LogPixels\"=dword:{dpi:08x}\r\n"));
    }
    text
}

/// `/home/u/x.reg` → `Z:\home\u\x.reg` (Wine maps `Z:` to `/`).
fn windows_path(path: &Path) -> Option<String> {
    Some(format!("Z:{}", path.to_str()?.replace('/', "\\")))
}

/// Deletes a prefix folder, only if it is a real folder (not a symlink)
/// strictly inside `prefixes_root`. Symlinks inside it (like `g:` to the
/// library) are removed as links; what they point to is not touched.
pub fn remove_prefix(prefix: &Path, prefixes_root: &Path) -> Result<(), HouseError> {
    let refuse = |reason| HouseError::Refused {
        path: prefix.to_owned(),
        reason,
    };
    let meta = match fs::symlink_metadata(prefix) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(source) => return Err(io_err("inspect", prefix)(source)),
    };
    if !meta.is_dir() {
        return Err(refuse("it is not a real folder"));
    }
    let root =
        fs::canonicalize(prefixes_root).map_err(|_| refuse("the prefixes folder is missing"))?;
    let canonical = fs::canonicalize(prefix).map_err(io_err("inspect", prefix))?;
    if canonical == root || !canonical.starts_with(&root) {
        return Err(refuse("it is outside Snow Launcher's prefixes folder"));
    }
    fs::remove_dir_all(&canonical).map_err(io_err("delete", prefix))
}

/// Carries out a removal plan (`snowlauncher_core::house::plan_removal`).
pub fn remove(
    steps: &[RemoveStep],
    prefixes_root: &Path,
    record_path: &Path,
    mut on_step: impl FnMut(&RemoveStep),
) -> Result<(), HouseError> {
    for step in steps {
        on_step(step);
        match step {
            RemoveStep::DeletePrefix { path } => remove_prefix(path, prefixes_root)?,
            RemoveStep::DeleteEmptyLibrary { path } => match fs::remove_dir(path) {
                // `remove_dir` only removes empty folders: games are safe.
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(source) => return Err(io_err("delete", path)(source)),
            },
            RemoveStep::KeepLibrary { .. } => {}
            RemoveStep::ForgetRecord => match fs::remove_file(record_path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(source) => return Err(io_err("delete", record_path)(source)),
            },
        }
    }
    Ok(())
}

/// `$XDG_STATE_HOME/snow-launcher/house.toml`.
pub fn record_path() -> Option<PathBuf> {
    BaseDirs::new().and_then(|dirs| {
        dirs.state_dir()
            .map(|state| state.join("snow-launcher").join("house.toml"))
    })
}

/// Log of the Wine tools' output during create.
pub fn create_log_path() -> Option<PathBuf> {
    record_path().map(|path| path.with_file_name("house-create.log"))
}

/// Log of Battle.net's Wine output when opened by Snow Launcher.
pub fn open_log_path() -> Option<PathBuf> {
    record_path().map(|path| path.with_file_name("house-battlenet.log"))
}

pub fn load_record(path: &Path) -> Result<Option<HouseRecord>, HouseError> {
    match fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text)
            .map(Some)
            .map_err(|error| HouseError::Step {
                step: format!("read {}", path.display()),
                problem: error.to_string(),
            }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(io_err("read", path)(source)),
    }
}

/// Writes the record atomically: temporary file, flush to disk, rename.
pub fn save_record(path: &Path, record: &HouseRecord) -> Result<(), HouseError> {
    let text = toml::to_string(record).map_err(|error| HouseError::Step {
        step: format!("write {}", path.display()),
        problem: error.to_string(),
    })?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(io_err("create", dir))?;
    }
    write_atomic(path, text.as_bytes()).map_err(io_err("write", path))
}

/// Writes a file atomically: temporary file next to it, flush to disk,
/// rename. Readers see the old content or the new one, never half.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut tmp_name = OsString::from(path.as_os_str());
    tmp_name.push(".tmp");
    let tmp = PathBuf::from(tmp_name);
    let write = || -> io::Result<()> {
        let mut file = File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&tmp, path)
    };
    write().inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use snowlauncher_core::display::{DpiChoice, DpiSource};
    use snowlauncher_core::house::{Existing, plan_house, plan_removal};

    /// Pretends to be Wine: each tool writes what the real one would.
    struct FakeWine {
        prefix: PathBuf,
        /// Tool that exits with an error and writes nothing.
        fail: Option<&'static str>,
        calls: Vec<String>,
    }

    impl FakeWine {
        fn append(&self, file: &str, text: &str) {
            let path = self.prefix.join(file);
            let mut old = fs::read_to_string(&path).unwrap_or_default();
            old.push_str(text);
            fs::write(path, old).unwrap();
        }
    }

    impl WineRunner for FakeWine {
        fn run(&mut self, args: &[String]) -> io::Result<bool> {
            let tool = args[0].as_str();
            self.calls.push(args.join(" "));
            if self.fail == Some(tool) {
                return Ok(false);
            }
            match tool {
                "wineboot" => {
                    fs::create_dir_all(self.prefix.join("drive_c"))?;
                    fs::create_dir_all(self.prefix.join("dosdevices"))?;
                    self.append("system.reg", "WINE REGISTRY Version 2\n");
                    self.append("user.reg", "WINE REGISTRY Version 2\n");
                }
                "winecfg" => self.append(
                    "system.reg",
                    "\n[Software\\\\Microsoft\\\\Windows NT\\\\CurrentVersion] 1\n\"ProductName\"=\"Microsoft Windows 11\"\n",
                ),
                "regedit" => {
                    // Like Wine: read the .reg file it was given.
                    let path = args[2].strip_prefix("Z:").unwrap().replace('\\', "/");
                    let reg = fs::read_to_string(path)?;
                    let value = reg.lines().find(|l| l.starts_with("\"LogPixels\"")).unwrap();
                    self.append("user.reg", &format!("\n[Control Panel\\\\Desktop] 1\n{value}\n\n[Software\\\\Wine\\\\Fonts] 1\n{value}\n"));
                    self.append("system.reg", &format!("\n[System\\\\ControlSet001\\\\Hardware Profiles\\\\Current\\\\Software\\\\Fonts] 1\n{value}\n"));
                }
                _ => {}
            }
            Ok(true)
        }
    }

    struct Setup {
        _tmp: tempfile::TempDir,
        root: PathBuf,
        inputs: HouseInputs,
    }

    fn setup() -> Setup {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("prefixes");
        let inputs = HouseInputs {
            prefix: root.join("battlenet"),
            runtime_name: "GE-Proton11-14-x86_64".to_owned(),
            library: tmp.path().join("Games/SnowLauncher"),
            dpi: DpiChoice {
                dpi: 192,
                source: DpiSource::Desktop,
            },
        };
        Setup {
            _tmp: tmp,
            root,
            inputs,
        }
    }

    fn steps(inputs: &HouseInputs) -> Vec<HouseStep> {
        let existing = Existing {
            prefix: false,
            library: false,
        };
        plan_house(inputs, existing).unwrap()
    }

    fn wine(inputs: &HouseInputs, fail: Option<&'static str>) -> FakeWine {
        FakeWine {
            prefix: inputs.prefix.clone(),
            fail,
            calls: Vec::new(),
        }
    }

    #[test]
    fn builds_and_verifies_a_house() {
        let s = setup();
        let mut wine = wine(&s.inputs, None);
        let mut seen = 0;
        let record = create(
            &steps(&s.inputs),
            &s.inputs,
            Path::new("/x/GE"),
            &s.root,
            &mut wine,
            |_| seen += 1,
        )
        .unwrap();

        assert_eq!(seen, 5);
        assert_eq!(
            wine.calls[..2],
            ["wineboot -u".to_owned(), "winecfg -v win11".to_owned()]
        );
        assert!(wine.calls[2].starts_with("regedit /S Z:\\"));
        assert!(record.library_created);
        assert_eq!(record.dpi, 192);
        let link = s.inputs.prefix.join("dosdevices/g:");
        assert_eq!(fs::read_link(link).unwrap(), s.inputs.library);
        // The temporary .reg file is gone.
        assert!(!s.inputs.prefix.join("snow-launcher-dpi.reg").exists());
    }

    #[test]
    fn a_failed_step_removes_what_was_created() {
        let s = setup();
        let mut wine = wine(&s.inputs, Some("regedit"));
        let error = create(
            &steps(&s.inputs),
            &s.inputs,
            Path::new("/x"),
            &s.root,
            &mut wine,
            |_| {},
        )
        .unwrap_err();
        assert!(error.to_string().contains("set the UI scale"), "{error}");
        assert!(!s.inputs.prefix.exists());
        // The library was not created yet at that point.
        assert!(!s.inputs.library.exists());
    }

    #[test]
    fn a_silent_failure_is_caught_by_verification() {
        // winecfg "succeeds" but changes nothing.
        struct Liar(FakeWine);
        impl WineRunner for Liar {
            fn run(&mut self, args: &[String]) -> io::Result<bool> {
                if args[0] == "winecfg" {
                    return Ok(true);
                }
                self.0.run(args)
            }
        }
        let s = setup();
        let mut liar = Liar(wine(&s.inputs, None));
        let error = create(
            &steps(&s.inputs),
            &s.inputs,
            Path::new("/x"),
            &s.root,
            &mut liar,
            |_| {},
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("Microsoft Windows 11"),
            "{error}"
        );
        assert!(!s.inputs.prefix.exists());
    }

    #[test]
    fn removal_keeps_the_games_behind_the_drive_link() {
        let s = setup();
        let mut wine = wine(&s.inputs, None);
        let record = create(
            &steps(&s.inputs),
            &s.inputs,
            Path::new("/x"),
            &s.root,
            &mut wine,
            |_| {},
        )
        .unwrap();
        // A game in the library, reachable as G: from the prefix.
        let game = s.inputs.library.join("World of Warcraft/Wow.exe");
        fs::create_dir_all(game.parent().unwrap()).unwrap();
        fs::write(&game, "game").unwrap();
        let record_file = s.root.join("house.toml");
        save_record(&record_file, &record).unwrap();
        assert_eq!(load_record(&record_file).unwrap(), Some(record.clone()));

        let library_empty = fs::read_dir(&record.library).unwrap().next().is_none();
        let steps = plan_removal(&record, false, library_empty).unwrap();
        remove(&steps, &s.root, &record_file, |_| {}).unwrap();

        assert!(!record.prefix.exists());
        assert_eq!(fs::read_to_string(&game).unwrap(), "game");
        assert!(load_record(&record_file).unwrap().is_none());
    }

    #[test]
    fn never_deletes_outside_the_prefixes_folder() {
        let s = setup();
        let outside = s.root.parent().unwrap().join("elsewhere");
        fs::create_dir_all(&outside).unwrap();
        fs::create_dir_all(&s.root).unwrap();
        assert!(matches!(
            remove_prefix(&outside, &s.root),
            Err(HouseError::Refused { .. })
        ));
        assert!(matches!(
            remove_prefix(&s.root, &s.root),
            Err(HouseError::Refused { .. })
        ));
        // A symlink inside the root pointing outside is refused too.
        let link = s.root.join("sneaky");
        symlink(&outside, &link).unwrap();
        assert!(matches!(
            remove_prefix(&link, &s.root),
            Err(HouseError::Refused { .. })
        ));
        assert!(outside.exists());
    }

    #[test]
    fn everything_in_the_house_runs_with_qt_high_dpi_scaling_off() {
        let tmp = tempfile::tempdir().unwrap();
        let runner = UmuRunner {
            umu_run: PathBuf::from("/usr/bin/umu-run"),
            prefix: PathBuf::from("/pfx"),
            runtime: PathBuf::from("/runtime"),
            log: tmp.path().join("log"),
        };
        let command = runner.command(&["Battle.net-Setup.exe"]).unwrap();
        let envs: Vec<(String, String)> = command
            .get_envs()
            .filter_map(|(key, value)| {
                Some((key.to_str()?.to_owned(), value?.to_str()?.to_owned()))
            })
            .collect();
        // Without it the installer freezes above 100% DPI (E11).
        assert!(envs.contains(&("QT_ENABLE_HIGHDPI_SCALING".to_owned(), "0".to_owned())));
        assert!(envs.contains(&("WINEPREFIX".to_owned(), "/pfx".to_owned())));
    }

    #[test]
    fn launch_argument_is_a_single_argument() {
        // One argv entry, as Battle.net expects `--exec="launch WoWF"`.
        assert_eq!(launch_argument("WoWF"), "--exec=launch WoWF");
    }

    #[test]
    fn running_battlenet_needs_it_installed() {
        let tmp = tempfile::tempdir().unwrap();
        let runner = UmuRunner {
            umu_run: PathBuf::from("/nonexistent/umu-run"),
            prefix: tmp.path().to_owned(),
            runtime: PathBuf::from("/runtime"),
            log: tmp.path().join("log"),
        };
        let error = run_battlenet(&runner, &[]).unwrap_err().to_string();
        assert!(error.contains("not installed"), "{error}");
    }

    #[test]
    fn dir_size_counts_files_and_skips_links() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("Battle.net");
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("a"), [0u8; 100]).unwrap();
        fs::write(dir.join("sub/b"), [0u8; 50]).unwrap();
        // A link to something big must not be counted (nor followed).
        fs::write(tmp.path().join("big"), [0u8; 10_000]).unwrap();
        symlink(tmp.path().join("big"), dir.join("link")).unwrap();
        assert_eq!(dir_size(&dir), 150);
        assert_eq!(dir_size(&tmp.path().join("missing")), 0);
    }

    #[test]
    fn dpi_reg_file_sets_the_three_values() {
        let text = dpi_reg_file(192);
        assert!(text.starts_with("REGEDIT4\r\n"));
        assert_eq!(text.matches("\"LogPixels\"=dword:000000c0").count(), 3);
        assert_eq!(
            windows_path(Path::new("/home/u/x.reg")).unwrap(),
            "Z:\\home\\u\\x.reg"
        );
    }
}
