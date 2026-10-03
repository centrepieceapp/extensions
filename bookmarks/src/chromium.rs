//! Chrome and the browsers built on Chromium: Brave, Edge, Vivaldi, Opera…
//!
//! Each keeps its profiles in `Local State` and a folder per profile beside
//! it, with the bookmarks in that folder's `Bookmarks`, a JSON file rewritten
//! on every change. Opera keeps one profile in the data folder itself.

use std::collections::HashMap;
use std::path::Path;

use centrepiece_extension::{Icon, host};
use serde::Deserialize;

use crate::{Bookmark, Profile};

const LOCAL_STATE: &str = "Local State";
const BOOKMARKS: &str = "Bookmarks";

#[derive(Deserialize)]
struct LocalState {
    #[serde(default)]
    profile: Option<ProfileSection>,
}

#[derive(Deserialize)]
struct ProfileSection {
    #[serde(default)]
    info_cache: HashMap<String, ProfileEntry>,
}

#[derive(Deserialize)]
struct ProfileEntry {
    #[serde(default)]
    name: String,
    #[serde(default)]
    user_name: String,
    /// The Google account's picture, in the profile's folder.
    #[serde(default)]
    gaia_picture_file_name: Option<String>,
    /// `false` when the user chose another avatar over the account picture.
    #[serde(default)]
    use_gaia_picture: Option<bool>,
    /// The profile's colour, as a signed `0xAARRGGBB`.
    #[serde(default)]
    profile_highlight_color: Option<i64>,
}

/// Every profile the browser knows about whose folder is there, `Default`
/// first, then in the order they were made.
pub fn profiles(data: &Path) -> Vec<Profile> {
    let entries = std::fs::read_to_string(data.join(LOCAL_STATE))
        .ok()
        .and_then(
            |contents| match serde_json::from_str::<LocalState>(&contents) {
                Ok(state) => state.profile,
                Err(error) => {
                    host::warn(format!("could not parse {}: {error}", data.display()));
                    None
                }
            },
        )
        .map(|section| section.info_cache)
        .unwrap_or_default();

    let mut profiles: Vec<Profile> = entries
        .into_iter()
        .filter(|(directory, _)| data.join(directory).is_dir())
        .map(|(directory, entry)| profile(data, directory, entry))
        .collect();
    profiles.sort_by_key(rank);

    // Opera: the data folder is the profile.
    if profiles.is_empty() && data.join(BOOKMARKS).is_file() {
        profiles.push(Profile {
            name: "Default".to_string(),
            subtitle: None,
            dir: data.to_path_buf(),
            launch: Vec::new(),
            avatar: None,
        });
    }
    profiles
}

fn profile(data: &Path, directory: String, entry: ProfileEntry) -> Profile {
    let avatar = avatar(&data.join(&directory), &entry);
    Profile {
        avatar,
        name: if entry.name.is_empty() {
            directory.clone()
        } else {
            entry.name
        },
        subtitle: Some(if entry.user_name.is_empty() {
            directory.clone()
        } else {
            entry.user_name
        }),
        dir: data.join(&directory),
        launch: vec![format!("--profile-directory={directory}")],
    }
}

/// What the browser shows for a profile: the account's picture when it uses
/// one and it has been downloaded, or else the profile's colour. The built-in
/// avatars are drawn from inside the browser's binary, out of reach.
fn avatar(dir: &Path, entry: &ProfileEntry) -> Option<Icon> {
    let picture = entry
        .gaia_picture_file_name
        .as_ref()
        .filter(|_| entry.use_gaia_picture != Some(false))
        .map(|name| dir.join(name))
        .filter(|picture| picture.is_file());
    if let Some(picture) = picture {
        return Some(Icon::File(picture.to_string_lossy().into_owned()));
    }
    entry.profile_highlight_color.map(|argb| {
        let argb = argb as u32;
        // ARGB to RGBA.
        Icon::Color(argb.rotate_left(8))
    })
}

/// `Default` sorts first, then `Profile N` by number, then anything else by
/// name — so the digit keys stay put between visits.
fn rank(profile: &Profile) -> (u8, u32, String) {
    let directory = profile
        .dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    if directory == "Default" {
        return (0, 0, String::new());
    }
    match directory
        .strip_prefix("Profile ")
        .and_then(|number| number.parse().ok())
    {
        Some(number) => (1, number, String::new()),
        None => (2, 0, directory),
    }
}

#[derive(Deserialize)]
struct BookmarkFile {
    roots: HashMap<String, Node>,
}

#[derive(Deserialize)]
struct Node {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    children: Vec<Node>,
}

/// The bookmarks in the profile folder `dir`, bookmarks bar first.
pub fn bookmarks(dir: &Path) -> Result<Vec<Bookmark>, String> {
    let file = dir.join(BOOKMARKS);
    let contents = match std::fs::read_to_string(&file) {
        Ok(contents) => contents,
        // A profile that never saved a bookmark has no file.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("Could not read {}: {error}", file.display())),
    };
    let parsed: BookmarkFile = serde_json::from_str(&contents)
        .map_err(|error| format!("Could not parse {}: {error}", file.display()))?;
    Ok(from_file(parsed))
}

fn from_file(parsed: BookmarkFile) -> Vec<Bookmark> {
    // `roots` is a map, so walk it in a fixed order.
    let mut roots: Vec<(&String, &Node)> = parsed.roots.iter().collect();
    roots.sort_by_key(|(name, _)| match name.as_str() {
        "bookmark_bar" => 0,
        "other" => 1,
        _ => 2,
    });
    let mut found = Vec::new();
    for (_, root) in roots {
        flatten(root, &[], &mut found);
    }
    found
}

fn flatten(node: &Node, path: &[String], found: &mut Vec<Bookmark>) {
    if node.kind == "url" {
        if let Some(url) = &node.url {
            found.push(Bookmark {
                title: if node.name.is_empty() {
                    url.clone()
                } else {
                    node.name.clone()
                },
                url: url.clone(),
                folder: path.to_vec(),
            });
        }
        return;
    }
    let mut path = path.to_vec();
    if !node.name.is_empty() {
        path.push(display_name(&node.name));
    }
    for child in &node.children {
        flatten(child, &path, found);
    }
}

/// The built-in roots, named the same whatever the browser's spelling.
fn display_name(name: &str) -> String {
    match name {
        "Bookmarks bar" | "Bookmarks Bar" => "Bookmarks Bar".to_string(),
        "Other bookmarks" | "Other Bookmarks" => "Other Bookmarks".to_string(),
        "Mobile bookmarks" | "Mobile Bookmarks" => "Mobile Bookmarks".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_are_named_and_in_a_fixed_order() {
        let state: LocalState = serde_json::from_str(
            r#"{"profile":{"info_cache":{
                "Profile 10":{"name":"Ten"},
                "Profile 2":{"name":"Work","user_name":"me@work.com"},
                "Default":{"name":"Personal"}
            }}}"#,
        )
        .unwrap();
        let data = Path::new("/data");
        let mut profiles: Vec<Profile> = state
            .profile
            .unwrap()
            .info_cache
            .into_iter()
            .map(|(directory, entry)| profile(data, directory, entry))
            .collect();
        profiles.sort_by_key(rank);
        let names: Vec<&str> = profiles.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Personal", "Work", "Ten"]);
        assert_eq!(profiles[1].subtitle.as_deref(), Some("me@work.com"));
        assert_eq!(profiles[0].subtitle.as_deref(), Some("Default"));
        assert_eq!(profiles[1].launch, ["--profile-directory=Profile 2"]);
    }

    #[test]
    fn a_profile_without_a_picture_shows_its_colour() {
        let entry: ProfileEntry = serde_json::from_str(
            r#"{"gaia_picture_file_name":"Google Profile Picture.png",
                "profile_highlight_color":-9761}"#,
        )
        .unwrap();
        // No such folder, so no picture: the colour, as 0xRRGGBBAA.
        assert_eq!(
            avatar(Path::new("/nowhere"), &entry),
            Some(Icon::Color(0xFFD9_DFFF))
        );
        let plain: ProfileEntry = serde_json::from_str("{}").unwrap();
        assert_eq!(avatar(Path::new("/nowhere"), &plain), None);
    }

    #[test]
    fn bookmarks_carry_their_folder_path() {
        let parsed: BookmarkFile = serde_json::from_str(
            r#"{"roots":{
                "other":{"type":"folder","name":"Other bookmarks","children":[
                    {"type":"url","name":"","url":"https://example.com"}]},
                "bookmark_bar":{"type":"folder","name":"Bookmarks bar","children":[
                    {"type":"url","name":"Rust","url":"https://rust-lang.org"},
                    {"type":"folder","name":"Work","children":[
                        {"type":"url","name":"Repo","url":"https://github.com"}]}]}
            }}"#,
        )
        .unwrap();
        let found = from_file(parsed);
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].folder, ["Bookmarks Bar"]);
        assert_eq!(found[1].folder, ["Bookmarks Bar", "Work"]);
        assert_eq!(found[2].title, "https://example.com");
        assert_eq!(found[2].folder, ["Other Bookmarks"]);
    }
}
