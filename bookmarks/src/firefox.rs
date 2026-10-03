//! Firefox and the browsers built on it: Zen, LibreWolf.
//!
//! Profiles are listed in `profiles.ini`, and each keeps its bookmarks in
//! `places.sqlite`. There is no SQLite in the sandbox, so the system's
//! `sqlite3` reads it, and answers in JSON. The browser holds the database
//! locked while it runs, so it is opened immutable: what the browser has not
//! yet written back from its journal is missed until it does.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use centrepiece_extension::{TaskId, TaskResult, host};
use serde::Deserialize;

use crate::{Bookmark, Profile};

const SQLITE: &str = "/usr/bin/sqlite3";
const PROFILES: &str = "profiles.ini";
const PLACES: &str = "places.sqlite";

/// Every bookmark under the four roots a user sees, with the folders it sits
/// in, separated by [`SEPARATOR`]; in the order the browser shows them, the
/// toolbar's first. `place` sorts by position at every level of the tree.
const QUERY: &str = "
WITH RECURSIVE folders(id, path, place) AS (
    SELECT id, CASE guid
        WHEN 'toolbar_____' THEN 'Bookmarks Toolbar'
        WHEN 'menu________' THEN 'Bookmarks Menu'
        WHEN 'unfiled_____' THEN 'Other Bookmarks'
        ELSE 'Mobile Bookmarks' END,
        CASE guid
        WHEN 'toolbar_____' THEN '0'
        WHEN 'menu________' THEN '1'
        WHEN 'unfiled_____' THEN '2'
        ELSE '3' END
    FROM moz_bookmarks
    WHERE guid IN ('toolbar_____', 'menu________', 'unfiled_____', 'mobile______')
    UNION ALL
    SELECT b.id, f.path || char(31) || COALESCE(b.title, ''),
        f.place || '.' || printf('%06d', b.position)
    FROM moz_bookmarks b JOIN folders f ON b.parent = f.id
    WHERE b.type = 2
)
SELECT COALESCE(b.title, '') AS title, p.url AS url, f.path AS folder
FROM moz_bookmarks b
JOIN moz_places p ON p.id = b.fk
JOIN folders f ON f.id = b.parent
WHERE b.type = 1 AND p.url NOT LIKE 'place:%'
ORDER BY f.place || '.' || printf('%06d', b.position);
";

/// Between the folder names in a row's `folder`: no title has one.
const SEPARATOR: char = '\u{1f}';

/// One section of an ini file: its name, and its keys.
type Section = (String, HashMap<String, String>);

fn parse_ini(text: &str) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    for line in text.lines().map(str::trim) {
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            sections.push((name.to_string(), HashMap::new()));
        } else if let (Some((key, value)), Some((_, keys))) =
            (line.split_once('='), sections.last_mut())
        {
            keys.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    sections
}

/// The profiles in `profiles.ini` that hold a bookmark database, the one this
/// install starts with first, then by name.
pub fn profiles(data: &Path) -> Vec<Profile> {
    let Ok(text) = std::fs::read_to_string(data.join(PROFILES)) else {
        return Vec::new();
    };
    from_ini(data, &text)
        .into_iter()
        // A profile kept elsewhere cannot be checked from the sandbox; sqlite3
        // will say if it is not there.
        .filter(|profile| !profile.dir.starts_with(data) || profile.dir.join(PLACES).is_file())
        .collect()
}

fn from_ini(data: &Path, text: &str) -> Vec<Profile> {
    let sections = parse_ini(text);
    // The install's own default wins over the `Default=1` of older versions.
    let default = sections
        .iter()
        .find(|(name, keys)| name.starts_with("Install") && keys.contains_key("Default"))
        .map(|(_, keys)| keys["Default"].clone());

    let mut profiles: Vec<(bool, Profile)> = sections
        .iter()
        .filter(|(name, _)| name.starts_with("Profile"))
        .filter_map(|(_, keys)| {
            let path = keys.get("Path")?;
            let name = keys.get("Name").cloned().unwrap_or_else(|| path.clone());
            let dir = if keys
                .get("IsRelative")
                .is_some_and(|relative| relative == "1")
            {
                data.join(path)
            } else {
                PathBuf::from(path)
            };
            let is_default = match &default {
                Some(default) => default == path,
                None => keys.get("Default").is_some_and(|flag| flag == "1"),
            };
            Some((
                is_default,
                Profile {
                    subtitle: Some(if is_default {
                        "Default profile".to_string()
                    } else {
                        path.rsplit('/').next().unwrap_or(path).to_string()
                    }),
                    launch: vec!["-P".to_string(), name.clone()],
                    avatar: None,
                    name,
                    dir,
                },
            ))
        })
        .collect();
    profiles.sort_by(|(a_default, a), (b_default, b)| {
        b_default.cmp(a_default).then_with(|| a.name.cmp(&b.name))
    });
    profiles.into_iter().map(|(_, profile)| profile).collect()
}

/// Starts reading the bookmarks of the profile folder `dir`; the result goes
/// to [`parse`].
pub fn read(dir: &Path) -> TaskId {
    let uri = format!(
        "file:{}?immutable=1",
        escape(&dir.join(PLACES).to_string_lossy())
    );
    host::exec(SQLITE, &["-readonly", "-json", &uri, QUERY])
}

/// A path as it goes in an SQLite URI.
fn escape(path: &str) -> String {
    let mut escaped = String::with_capacity(path.len());
    for c in path.chars() {
        match c {
            '%' | '?' | '#' | ' ' => escaped.push_str(&format!("%{:02X}", c as u32)),
            _ => escaped.push(c),
        }
    }
    escaped
}

#[derive(Deserialize)]
struct Row {
    title: String,
    url: String,
    folder: String,
}

/// What [`read`] found.
pub fn parse(result: TaskResult) -> Result<Vec<Bookmark>, String> {
    let output = match result {
        TaskResult::Output(Ok(output)) => output,
        TaskResult::Output(Err(error)) => return Err(format!("Could not run sqlite3: {error}")),
        _ => return Err("Unexpected answer from sqlite3".to_string()),
    };
    if !output.is_success() {
        return Err(format!(
            "Could not read the bookmarks: {}",
            output.stderr().trim()
        ));
    }
    // No rows prints nothing at all, not `[]`.
    if output.stdout().trim().is_empty() {
        return Ok(Vec::new());
    }
    let rows: Vec<Row> = output
        .json()
        .map_err(|error| format!("Could not parse what sqlite3 printed: {error}"))?;
    Ok(rows
        .into_iter()
        .map(|row| Bookmark {
            title: if row.title.is_empty() {
                row.url.clone()
            } else {
                row.title
            },
            url: row.url,
            folder: row.folder.split(SEPARATOR).map(str::to_string).collect(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const INI: &str = "
[Profile1]
Name=default
IsRelative=1
Path=Profiles/abc.default
Default=1

[Profile0]
Name=default-release
IsRelative=1
Path=Profiles/xyz.default-release

[Profile2]
Name=elsewhere
IsRelative=0
Path=/Volumes/Data/ff

[General]
StartWithLastProfile=1

[Install2656FF1E876E9973]
Default=Profiles/xyz.default-release
Locked=1
";

    #[test]
    fn the_installs_default_profile_comes_first() {
        let profiles = from_ini(Path::new("/ff"), INI);
        let names: Vec<&str> = profiles.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["default-release", "default", "elsewhere"]);
        assert_eq!(
            profiles[0].dir,
            Path::new("/ff/Profiles/xyz.default-release")
        );
        assert_eq!(profiles[0].subtitle.as_deref(), Some("Default profile"));
        assert_eq!(profiles[0].launch, ["-P", "default-release"]);
        assert_eq!(profiles[2].dir, Path::new("/Volumes/Data/ff"));
    }

    #[test]
    fn without_an_install_section_the_flag_decides() {
        let ini = INI.split("[Install").next().unwrap();
        let profiles = from_ini(Path::new("/ff"), ini);
        assert_eq!(profiles[0].name, "default");
    }

    #[test]
    fn paths_are_escaped_for_a_uri() {
        assert_eq!(
            escape("/Users/me/Library/Application Support/Firefox/a?b#c%d/places.sqlite"),
            "/Users/me/Library/Application%20Support/Firefox/a%3Fb%23c%25d/places.sqlite"
        );
        assert_eq!(escape("/Users/Zoë/x"), "/Users/Zoë/x");
    }
}
