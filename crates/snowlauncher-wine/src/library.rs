//! The games in the house's library, read from their own files. Read-only.

use std::fs;
use std::path::{Path, PathBuf};

use snowlauncher_core::library::{
    BuildInfo, ProductCode, launch_code, parse_build_info, product_codes,
};

use crate::battlenet_logs::client_logs;
use crate::prefix::WinePrefix;

/// How many recent Battle.net sessions are searched for product codes.
const LOGS_SEARCHED: usize = 5;

/// A game folder in the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledGame {
    /// Folder name, e.g. `World of Warcraft`.
    pub name: String,
    pub path: PathBuf,
    pub build: BuildInfo,
    /// For `--exec="launch <code>"`, if Battle.net's log revealed it.
    pub launch_code: Option<String>,
}

/// Every folder of `library` with a readable `.build.info`, by name.
pub fn installed_games(library: &Path, codes: &[ProductCode]) -> Vec<InstalledGame> {
    let Ok(entries) = fs::read_dir(library) else {
        return Vec::new();
    };
    let mut games: Vec<InstalledGame> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let text = fs::read_to_string(path.join(".build.info")).ok()?;
            let build = parse_build_info(&text)?;
            Some(InstalledGame {
                name: entry.file_name().to_string_lossy().into_owned(),
                launch_code: launch_code(&build.product, codes),
                path,
                build,
            })
        })
        .collect();
    games.sort_by(|a, b| a.name.cmp(&b.name));
    games
}

/// Product codes named in the house's recent Battle.net logs. Empty if
/// Battle.net never ran there.
pub fn known_product_codes(prefix: &Path) -> Vec<ProductCode> {
    let Ok(prefix) = WinePrefix::open(prefix) else {
        return Vec::new();
    };
    let mut codes: Vec<ProductCode> = Vec::new();
    for log in client_logs(&prefix)
        .unwrap_or_default()
        .into_iter()
        .take(LOGS_SEARCHED)
    {
        let Ok(bytes) = fs::read(&log) else { continue };
        for code in product_codes(&String::from_utf8_lossy(&bytes)) {
            if !codes.contains(&code) {
                codes.push(code);
            }
        }
    }
    codes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_game_folders_with_their_codes() {
        let tmp = tempfile::tempdir().unwrap();
        let wow = tmp.path().join("World of Warcraft");
        fs::create_dir_all(&wow).unwrap();
        fs::write(
            wow.join(".build.info"),
            "Active!DEC:1|Version!STRING:0|Product!STRING:0\n1|1.60.1.71139|wowf\n",
        )
        .unwrap();
        // Not a game: no .build.info.
        fs::create_dir_all(tmp.path().join("Screenshots")).unwrap();

        let codes = vec![ProductCode {
            code: "WoWF".to_owned(),
            flavor: "wowf".to_owned(),
        }];
        let games = installed_games(tmp.path(), &codes);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].name, "World of Warcraft");
        assert_eq!(games[0].build.product, "wowf");
        assert_eq!(games[0].launch_code.as_deref(), Some("WoWF"));

        // Without the log's help the code is unknown, not guessed.
        assert_eq!(installed_games(tmp.path(), &[])[0].launch_code, None);
        assert!(installed_games(&tmp.path().join("missing"), &[]).is_empty());
    }
}
