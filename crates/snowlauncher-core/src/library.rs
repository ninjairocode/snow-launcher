//! Games in the library, and how to launch them through Battle.net.
//! Pure: the text of the files comes from elsewhere.
//!
//! - Each installed game folder has a `.build.info`, a `|`-separated table
//!   whose `Product` column is the game's uid (`wowf`) and `Version` its
//!   version.
//! - `Battle.net.exe --exec="launch <code>"` needs the **product code**
//!   (`WoWF`), not the uid; with the uid it silently does nothing
//!   (experiment E5). The Battle.net log lists pairs such as
//!   `productCode=WoWF_wowf`, `productCode=WoWC_wow_classic` and
//!   `productCode=WoW_retail`: code, underscore, flavor.

/// The active build of an installed game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildInfo {
    /// The game's uid, e.g. `wowf`.
    pub product: String,
    pub version: Option<String>,
}

/// Reads a `.build.info`: header `Name!TYPE:n|...`, then one row per
/// build. Uses the row marked `Active` = 1, or the first one.
pub fn parse_build_info(text: &str) -> Option<BuildInfo> {
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let header: Vec<&str> = lines
        .next()?
        .split('|')
        .map(|column| column.split('!').next().unwrap_or(column))
        .collect();
    let column = |name: &str| header.iter().position(|&c| c == name);
    let (product, version, active) = (column("Product")?, column("Version"), column("Active"));
    let rows: Vec<Vec<&str>> = lines.map(|line| line.split('|').collect()).collect();
    let row = rows
        .iter()
        .find(|row| active.and_then(|i| row.get(i)) == Some(&"1"))
        .or_else(|| rows.first())?;
    let product = row.get(product)?.trim();
    if product.is_empty() {
        return None;
    }
    Some(BuildInfo {
        product: product.to_owned(),
        version: version
            .and_then(|i| row.get(i))
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty()),
    })
}

/// A launch code as named in the Battle.net log: code and flavor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductCode {
    pub code: String,
    pub flavor: String,
}

/// Every distinct `productCode=<code>_<flavor>` in a Battle.net log.
pub fn product_codes(log: &str) -> Vec<ProductCode> {
    let mut found: Vec<ProductCode> = Vec::new();
    for (start, _) in log.match_indices("productCode=") {
        let rest = &log[start + "productCode=".len()..];
        let token: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let Some((code, flavor)) = token.split_once('_') else {
            continue;
        };
        if code.is_empty() || flavor.is_empty() {
            continue;
        }
        let pair = ProductCode {
            code: code.to_owned(),
            flavor: flavor.to_owned(),
        };
        if !found.contains(&pair) {
            found.push(pair);
        }
    }
    found
}

/// The launch code for a game's uid: the pair whose flavor is the uid
/// (`WoWF_wowf` for `wowf`, validated in E5), else a `retail` pair whose
/// code is the uid in other case (`WoW_retail` for `wow`, to confirm).
pub fn launch_code(uid: &str, codes: &[ProductCode]) -> Option<String> {
    codes
        .iter()
        .find(|pair| pair.flavor == uid)
        .or_else(|| {
            codes
                .iter()
                .find(|pair| pair.flavor == "retail" && pair.code.eq_ignore_ascii_case(uid))
        })
        .map(|pair| pair.code.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shape of the maintainer's real WoW `.build.info` (keys shortened).
    const BUILD_INFO: &str = "Branch!STRING:0|Active!DEC:1|Build Key!HEX:16|CDN Key!HEX:16|Install Key!HEX:16|IM Size!DEC:4|CDN Path!STRING:0|CDN Hosts!STRING:0|CDN Servers!STRING:0|Tags!STRING:0|Armadillo!STRING:0|Last Activated!STRING:0|Version!STRING:0|KeyRing!HEX:16|Product!STRING:0\n\
us|1|7a16|bc77|||tpr/wow|level3.blizzard.com|http://x|Windows x86_64 US? enUS speech?:Windows x86_64 US? enUS text?|||1.60.1.71139||wowf\n";

    #[test]
    fn reads_product_and_version() {
        assert_eq!(
            parse_build_info(BUILD_INFO),
            Some(BuildInfo {
                product: "wowf".to_owned(),
                version: Some("1.60.1.71139".to_owned()),
            })
        );
    }

    #[test]
    fn prefers_the_active_build() {
        let text = "Active!DEC:1|Version!STRING:0|Product!STRING:0\n0|1.0|old\n1|2.0|wow\n";
        assert_eq!(parse_build_info(text).unwrap().product, "wow");
    }

    #[test]
    fn rejects_files_without_a_product() {
        assert_eq!(parse_build_info(""), None);
        assert_eq!(parse_build_info("Version!STRING:0\n1.0\n"), None);
        assert_eq!(parse_build_info("Product!STRING:0\n\n"), None);
    }

    // Real lines from a Battle.net log (O8 and the house).
    const LOG: &str = "D 2026-11-27 20:49:05.392537 [GameProvider] {Main} Tried to insert an unbound entity with no uid. productCode=WoWF_wowf\n\
D 2026-11-27 20:49:05.392600 [GameProvider] {Main} Tried to insert an unbound entity with no uid. productCode=WoWC_wow_classic\n\
D 2026-11-27 20:49:05.392700 [GameProvider] {Main} Tried to insert an unbound entity with no uid. productCode=WoW_retail\n\
D 2026-11-27 20:49:05.392800 [GameProvider] {Main} Tried to insert an unbound entity with no uid. productCode=WoWF_wowf\n";

    #[test]
    fn collects_distinct_codes() {
        let codes = product_codes(LOG);
        assert_eq!(codes.len(), 3);
        assert_eq!(
            codes[1],
            ProductCode {
                code: "WoWC".to_owned(),
                flavor: "wow_classic".to_owned()
            }
        );
    }

    #[test]
    fn finds_the_launch_code_for_a_uid() {
        let codes = product_codes(LOG);
        assert_eq!(launch_code("wowf", &codes).as_deref(), Some("WoWF")); // E5
        assert_eq!(launch_code("wow_classic", &codes).as_deref(), Some("WoWC"));
        assert_eq!(launch_code("wow", &codes).as_deref(), Some("WoW")); // to confirm
        assert_eq!(launch_code("d4", &codes), None);
    }
}
