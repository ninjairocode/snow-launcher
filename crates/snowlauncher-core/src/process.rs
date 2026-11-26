//! Processes and how they relate to Wine prefixes.
//!
//! Snow Launcher must only ever touch processes that belong to its own
//! prefix. A process belongs to a prefix when its `WINEPREFIX` environment
//! variable points to it. Processes are never identified by name, so a
//! Battle.net running under Lutris or Steam is never affected.

use std::path::{Path, PathBuf};

/// A Linux process id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pid(pub u32);

/// A snapshot of one running process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: Pid,
    /// Short process name (`/proc/<pid>/comm`). Only for display and logs,
    /// never for deciding which processes to touch. Unreliable for Wine
    /// programs: Chromium-based ones such as Battle.net rename it to
    /// `CrBrowserMain`, `CrRendererMain` etc.
    pub name: String,
    /// Command line (`/proc/<pid>/cmdline`), decoded lossily, without empty
    /// entries. For Wine programs the first entry is the Windows path of the
    /// executable, e.g. `C:\Program Files (x86)\Battle.net\Battle.net.exe`.
    pub cmdline: Vec<String>,
    /// Value of `WINEPREFIX`, if the process has one and it could be read.
    pub wineprefix: Option<PathBuf>,
}

impl ProcessInfo {
    /// Best human-readable name: the executable file name from the command
    /// line (e.g. `Battle.net.exe`), or `comm` if there is no command line.
    pub fn display_name(&self) -> &str {
        match self.cmdline.first() {
            Some(argv0) => file_name(argv0),
            None => &self.name,
        }
    }

    /// Title of the game this process is, if it is a known game.
    pub fn game_title(&self) -> Option<&'static str> {
        if ProcessRole::of(self) == ProcessRole::Game {
            known_game_title(self.display_name())
        } else {
            None
        }
    }
}

/// What a process is, as far as its command line tells.
///
/// Only for display and status. Which processes Snow Launcher may touch is
/// always decided by prefix (see [`processes_in_prefix`]), never by role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessRole {
    /// The main Battle.net client process.
    BattleNet,
    /// Chromium child processes of the client (GPU, renderer, utility).
    BattleNetHelper,
    Agent,
    /// Wine/Proton infrastructure that runs in every prefix.
    WineService,
    /// Linux-side launcher or runtime (umu-run, pressure-vessel, Lutris
    /// wrapper...). These inherit `WINEPREFIX` but are not Wine programs.
    Launcher,
    /// A known game (see [`KNOWN_GAMES`]).
    Game,
    /// Any other Windows program. Not assumed to be a game: a false
    /// positive would, for example, keep the computer from sleeping.
    Other,
}

// TODO(v0.4): these lists will come from the quirks repository.
const BATTLENET_EXE: &str = "Battle.net.exe";
const BATTLENET_HELPER_NAMES: &[&str] = &["Battle.net Helper.exe"];
const AGENT_NAMES: &[&str] = &["Agent.exe"];
const WINE_SERVICE_NAMES: &[&str] = &[
    "wineserver",
    "services.exe",
    "winedevice.exe",
    "plugplay.exe",
    "svchost.exe",
    "rpcss.exe",
    "lsass.exe",
    "explorer.exe",
    "tabtip.exe",
    "conhost.exe",
    "start.exe",
    "rundll32.exe",
    // Proton/umu helpers.
    "steam.exe",
    "umu.exe",
    // Wine's accessibility bridge (Proton).
    "xalia.exe",
];

/// Known game executables and their titles.
///
/// Only processes listed here count as games. Executable names are
/// compared case-insensitively.
// TODO(E5): confirm each name on a real install; WoW retail/Classic were
// the priority.
pub const KNOWN_GAMES: &[(&str, &str)] = &[
    ("Wow.exe", "World of Warcraft"),
    ("WowT.exe", "World of Warcraft (PTR)"),
    ("WowB.exe", "World of Warcraft (Beta)"),
    ("WowClassic.exe", "World of Warcraft Classic"),
    ("WowClassicT.exe", "World of Warcraft Classic (PTR)"),
    ("WowClassicB.exe", "World of Warcraft Classic (Beta)"),
    ("Diablo IV.exe", "Diablo IV"),
    ("Diablo III64.exe", "Diablo III"),
    ("D2R.exe", "Diablo II: Resurrected"),
    ("Overwatch.exe", "Overwatch 2"),
    ("Hearthstone.exe", "Hearthstone"),
    ("HeroesOfTheStorm_x64.exe", "Heroes of the Storm"),
    ("SC2_x64.exe", "StarCraft II"),
    ("StarCraft.exe", "StarCraft: Remastered"),
    ("Warcraft III.exe", "Warcraft III: Reforged"),
];

fn known_game_title(exe: &str) -> Option<&'static str> {
    KNOWN_GAMES
        .iter()
        .find(|(name, _)| exe.eq_ignore_ascii_case(name))
        .map(|(_, title)| *title)
}

/// Chromium marks its child processes with `--type=gpu-process` etc.
const CHROMIUM_CHILD_FLAG: &str = "--type=";

/// The kernel keeps only the first 15 bytes of a process name in `comm`.
const COMM_MAX_LEN: usize = 15;

impl ProcessRole {
    /// Classifies a process, preferring its command line over `comm`.
    pub fn of(process: &ProcessInfo) -> Self {
        match process.cmdline.split_first() {
            Some((argv0, args)) => Self::from_command(argv0, args),
            None => Self::from_comm(&process.name),
        }
    }

    fn from_command(argv0: &str, args: &[String]) -> Self {
        // Wine uses `\` (sometimes `/`); Linux programs use `/`.
        let exe = file_name(argv0);
        if exe.eq_ignore_ascii_case(BATTLENET_EXE)
            && args.iter().any(|arg| arg.starts_with(CHROMIUM_CHILD_FLAG))
        {
            Self::BattleNetHelper
        } else if let Some(role) = Self::from_known_name(|name| exe.eq_ignore_ascii_case(name)) {
            role
        } else if is_windows_path(argv0) {
            Self::Other
        } else {
            Self::Launcher
        }
    }

    /// Fallback when the command line is not available.
    fn from_comm(comm: &str) -> Self {
        Self::from_known_name(|name| comm_matches(comm, name)).unwrap_or(Self::Other)
    }

    fn from_known_name(matches: impl Fn(&str) -> bool) -> Option<Self> {
        let any = |names: &[&str]| names.iter().any(|name| matches(name));
        if matches(BATTLENET_EXE) {
            Some(Self::BattleNet)
        } else if any(BATTLENET_HELPER_NAMES) {
            Some(Self::BattleNetHelper)
        } else if any(AGENT_NAMES) {
            Some(Self::Agent)
        } else if any(WINE_SERVICE_NAMES) {
            Some(Self::WineService)
        } else if KNOWN_GAMES.iter().any(|(name, _)| matches(name)) {
            Some(Self::Game)
        } else {
            None
        }
    }
}

/// Last component of a Windows (`\` or `/`) or Linux path.
fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// `C:\...`, `c:/...` or `\\?\...`: how Wine shows Windows executables.
fn is_windows_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    let drive = matches!(bytes, [letter, b':', ..] if letter.is_ascii_alphabetic());
    drive || path.starts_with(r"\\")
}

/// Whether `comm` is `full_name` as the kernel would store it (truncated).
fn comm_matches(comm: &str, full_name: &str) -> bool {
    let full = full_name.as_bytes();
    full.get(..COMM_MAX_LEN).unwrap_or(full) == comm.as_bytes()
}

/// Source of running processes. The real implementation reads `/proc`;
/// tests use fixed lists.
pub trait ProcessTable {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Lists the processes visible right now.
    fn processes(&self) -> Result<Vec<ProcessInfo>, Self::Error>;
}

/// Returns the processes whose `WINEPREFIX` is exactly `prefix`.
///
/// Matching is conservative: relative `WINEPREFIX` values never match,
/// and a subfolder or a sibling with a similar name is not the same prefix.
/// Paths are compared component by component, so `/a/b/` equals `/a/b`.
/// Symlinks are not resolved here (that is IO); callers should pass
/// canonical paths on both sides.
pub fn processes_in_prefix<'a>(
    processes: &'a [ProcessInfo],
    prefix: &'a Path,
) -> impl Iterator<Item = &'a ProcessInfo> {
    processes.iter().filter(move |process| {
        process
            .wineprefix
            .as_deref()
            .is_some_and(|wineprefix| wineprefix.is_absolute() && wineprefix == prefix)
    })
}

/// Titles of the known games running in `prefix`, sorted and without
/// duplicates (a game may run as several processes).
pub fn running_games(processes: &[ProcessInfo], prefix: &Path) -> Vec<&'static str> {
    let mut titles: Vec<&'static str> = processes_in_prefix(processes, prefix)
        .filter_map(ProcessInfo::game_title)
        .collect();
    titles.sort_unstable();
    titles.dedup();
    titles
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(pid: u32, wineprefix: Option<&str>) -> ProcessInfo {
        ProcessInfo {
            pid: Pid(pid),
            name: format!("proc{pid}"),
            cmdline: Vec::new(),
            wineprefix: wineprefix.map(PathBuf::from),
        }
    }

    fn matching_pids(processes: &[ProcessInfo], prefix: &str) -> Vec<u32> {
        processes_in_prefix(processes, Path::new(prefix))
            .map(|p| p.pid.0)
            .collect()
    }

    const PREFIX: &str = "/home/user/.local/share/snow-launcher/prefixes/battlenet";

    #[test]
    fn matches_exact_prefix() {
        let processes = [process(1, Some(PREFIX))];
        assert_eq!(matching_pids(&processes, PREFIX), [1]);
    }

    #[test]
    fn ignores_trailing_slash() {
        let processes = [process(1, Some(&format!("{PREFIX}/")))];
        assert_eq!(matching_pids(&processes, PREFIX), [1]);
    }

    #[test]
    fn ignores_processes_without_wineprefix() {
        let processes = [process(1, None)];
        assert!(matching_pids(&processes, PREFIX).is_empty());
    }

    #[test]
    fn ignores_other_launchers() {
        let processes = [
            process(1, Some("/home/user/Games/battlenet")), // Lutris
            process(
                2,
                Some("/home/user/.steam/steam/steamapps/compatdata/123/pfx"),
            ),
            process(3, Some(PREFIX)),
        ];
        assert_eq!(matching_pids(&processes, PREFIX), [3]);
    }

    #[test]
    fn similar_names_and_subfolders_do_not_match() {
        let processes = [
            process(1, Some(&format!("{PREFIX}-old"))),
            process(2, Some(&format!("{PREFIX}/drive_c"))),
        ];
        assert!(matching_pids(&processes, PREFIX).is_empty());
    }

    /// Role of a process given only its command line (as Wine shows it).
    fn role(cmdline: &[&str]) -> ProcessRole {
        ProcessRole::of(&ProcessInfo {
            pid: Pid(1),
            name: "CrBrowserMain".to_owned(),
            cmdline: cmdline.iter().map(|arg| (*arg).to_owned()).collect(),
            wineprefix: None,
        })
    }

    // Command lines below were captured from a real Lutris + GE-Proton prefix.

    #[test]
    fn battlenet_main_process_is_found_despite_chromium_comm() {
        let bnet = r"C:\Program Files (x86)\Battle.net\Battle.net.exe";
        assert_eq!(role(&[bnet]), ProcessRole::BattleNet);
        assert_eq!(
            role(&[bnet, "--type=gpu-process", "--no-sandbox"]),
            ProcessRole::BattleNetHelper
        );
    }

    #[test]
    fn agent_with_versioned_folder_and_forward_slashes() {
        let agent = "C:/ProgramData/Battle.net/Agent/Agent.9886/Agent.exe";
        assert_eq!(role(&[agent, "--session=1"]), ProcessRole::Agent);
    }

    #[test]
    fn wine_services_are_recognized() {
        assert_eq!(
            role(&[r"C:\windows\system32\winedevice.exe"]),
            ProcessRole::WineService
        );
        assert_eq!(
            role(&[r"c:\windows\system32\umu.exe", "/x"]),
            ProcessRole::WineService
        );
        assert_eq!(
            role(&[r"\\?\X:\proton\files\share\xalia\xalia.exe"]),
            ProcessRole::WineService
        );
        assert_eq!(
            role(&["/opt/GE-Proton/files/bin/wineserver"]),
            ProcessRole::WineService
        );
    }

    #[test]
    fn linux_side_launchers_are_not_windows_programs() {
        assert_eq!(
            role(&["python3", "/home/u/umu/umu-run"]),
            ProcessRole::Launcher
        );
        assert_eq!(
            role(&["/usr/bin/python3", "/usr/share/lutris/bin/lutris-wrapper"]),
            ProcessRole::Launcher
        );
    }

    #[test]
    fn known_games_are_recognized_with_title() {
        let wow = ProcessInfo {
            pid: Pid(1),
            name: "Wow.exe".to_owned(),
            cmdline: vec![r"G:\World of Warcraft\_retail_\Wow.exe".to_owned()],
            wineprefix: None,
        };
        assert_eq!(ProcessRole::of(&wow), ProcessRole::Game);
        assert_eq!(wow.game_title(), Some("World of Warcraft"));
        assert_eq!(
            role(&[r"G:\World of Warcraft\_classic_\WOWCLASSIC.EXE"]),
            ProcessRole::Game
        );
    }

    #[test]
    fn running_games_only_counts_our_prefix_and_dedups() {
        let game = |pid, exe: &str, prefix: &str| ProcessInfo {
            pid: Pid(pid),
            name: String::new(),
            cmdline: vec![exe.to_owned()],
            wineprefix: Some(PathBuf::from(prefix)),
        };
        let processes = [
            game(1, r"G:\World of Warcraft\_retail_\Wow.exe", PREFIX),
            game(2, r"G:\World of Warcraft\_retail_\Wow.exe", PREFIX),
            game(
                3,
                r"C:\Games\Diablo IV\Diablo IV.exe",
                "/home/user/Games/lutris",
            ),
            game(
                4,
                r"C:\Program Files (x86)\Battle.net\Battle.net.exe",
                PREFIX,
            ),
        ];
        assert_eq!(
            running_games(&processes, Path::new(PREFIX)),
            ["World of Warcraft"]
        );
    }

    #[test]
    fn unknown_windows_program_is_not_a_game() {
        let tool = ProcessInfo {
            pid: Pid(1),
            name: "BlizzardError.e".to_owned(),
            cmdline: vec![r"C:\Program Files (x86)\Battle.net\BlizzardError.exe".to_owned()],
            wineprefix: None,
        };
        assert_eq!(ProcessRole::of(&tool), ProcessRole::Other);
        assert_eq!(tool.game_title(), None);
    }

    #[test]
    fn falls_back_to_comm_without_cmdline() {
        let by_comm = |comm: &str| {
            ProcessRole::of(&ProcessInfo {
                pid: Pid(1),
                name: comm.to_owned(),
                cmdline: Vec::new(),
                wineprefix: None,
            })
        };
        assert_eq!(by_comm("Agent.exe"), ProcessRole::Agent);
        // "Battle.net Helper.exe" is stored by the kernel as its first 15 bytes.
        assert_eq!(by_comm("Battle.net Help"), ProcessRole::BattleNetHelper);
        // A prefix shorter than 15 bytes is not a truncation.
        assert_eq!(by_comm("Battle.net"), ProcessRole::Other);
    }

    #[test]
    fn relative_wineprefix_never_matches() {
        let processes = [process(1, Some("battlenet"))];
        assert!(matching_pids(&processes, "battlenet").is_empty());
    }
}
