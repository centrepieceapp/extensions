//! Chrome, under the `bm` prefix: its bookmarks, and the URL on the clipboard
//! opened in a Chrome profile of your choosing.
//!
//! Both come from Chrome's own files, without asking Chrome anything. It keeps
//! its bookmarks in a JSON file per profile, which it rewrites on every change,
//! and its profiles in `Local State`, next to the profile directories. The
//! manifest lets this extension read those folders and nothing else.
//!
//! Copy a link, press the hotkey, and the first row offers to open it in
//! Chrome; `↩` lists the profiles, and one more key opens the page in that
//! profile. The same row leads the bookmark list while the link is there.
//!
//! Opening in a profile means running Chrome's own binary with
//! `--profile-directory`: `open -b` hands the URL over through Apple Events
//! and drops the arguments whenever Chrome is already running. The binary
//! forwards to the running Chrome instead, and exits as soon as it has.
//! Bookmarks go the same way, minus the profile, so they open in Chrome
//! rather than whatever the default browser is — unless Chrome will not start.
//!
//! These were two built-in extensions, `bm` and `url`; as one WebAssembly
//! extension they share one prefix, the way they share one browser.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use centrepiece_extension::{
    Extension, Icon, Item, Response, Screen, export_extension, home_dir, host,
};
use serde::Deserialize;

/// Chrome's binary, in the places the manifest lets the extension run it from,
/// in the order they are tried.
const CHROME: &[&str] = &[
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "~/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
];

/// Where browsers keep their profiles, relative to the home folder.
const SUPPORT: &str = "Library/Application Support";

/// Chrome's profiles, relative to [`SUPPORT`].
const PROFILES_DIR: &str = "Google/Chrome";
const LOCAL_STATE: &str = "Local State";

/// The Chromium-family browsers whose bookmarks we read, in preference order.
const BROWSER_DIRS: &[&str] = &[
    "Google/Chrome",
    "Google/Chrome Beta",
    "Google/Chrome Canary",
    "Chromium",
];

/// How long what was read from disk stays fresh. Chrome rewrites its files
/// on every change, and reading them again costs a few milliseconds.
const CACHE_TTL: Duration = Duration::from_secs(30);

/// The row that leads to the profile list.
const OPEN: &str = "open";
/// Profile rows are this followed by the profile's directory.
const PROFILE: &str = "profile:";

/// Centrepiece's own icons, by name.
mod icons {
    pub const BOOKMARK: &str = "bookmark";
    pub const LINK: &str = "link";
    pub const BROWSER: &str = "browser";
}

#[derive(Debug, Clone)]
struct Bookmark {
    title: String,
    url: String,
    /// Where the bookmark sits, e.g. `Bookmarks Bar / Rust`.
    folder: String,
}

/// One Chrome profile.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Profile {
    /// The directory under `Google/Chrome`, which is what Chrome is told.
    directory: String,
    /// What Chrome calls it in its own menu.
    name: String,
    /// The Google account it is signed in to, if any.
    account: Option<String>,
}

/// Something read from disk, and when.
struct Cached<T> {
    value: Vec<T>,
    read_at: Option<Instant>,
}

impl<T> Cached<T> {
    fn new() -> Self {
        Self {
            value: Vec::new(),
            read_at: None,
        }
    }

    /// The value, read again first when it is older than [`CACHE_TTL`].
    fn fresh(&mut self, read: impl FnOnce() -> Vec<T>) -> &[T] {
        if self
            .read_at
            .is_none_or(|read_at| read_at.elapsed() > CACHE_TTL)
        {
            self.value = read();
            self.read_at = Some(Instant::now());
        }
        &self.value
    }
}

struct Chrome {
    support: Option<PathBuf>,
    /// Whatever was on the clipboard at the last summon, if it was a URL.
    url: Option<String>,
    profiles: Cached<Profile>,
    bookmarks: Cached<Bookmark>,
    query: String,
}

impl Chrome {
    fn profiles(&mut self) -> &[Profile] {
        let support = self.support.clone();
        self.profiles
            .fresh(|| support.map(|dir| read_profiles(&dir)).unwrap_or_default())
    }

    fn bookmarks(&mut self) -> &[Bookmark] {
        let support = self.support.clone();
        self.bookmarks
            .fresh(|| support.map(|dir| read_bookmarks(&dir)).unwrap_or_default())
    }

    /// The row offering the copied URL: only with a URL copied, and a Chrome
    /// on this Mac with profiles to open it in.
    fn offer(&mut self) -> Option<Item> {
        let url = self.url.clone()?;
        if self.profiles().is_empty() {
            return None;
        }
        Some(
            Item::new(OPEN, "Open clipboard URL in Chrome profile")
                .subtitle(url)
                .icon(Icon::builtin(icons::LINK)),
        )
    }

    /// The bookmarks matching the query, led by the offer before anything is
    /// typed.
    fn results(&mut self) -> Screen {
        let query = self.query.clone();
        let mut items: Vec<Item> = Vec::new();
        if query.trim().is_empty() {
            items.extend(self.offer());
        }

        let bookmarks = self.bookmarks().to_vec();
        let empty = bookmarks.is_empty();
        let matches = host::rank(
            &query,
            bookmarks,
            |bookmark| bookmark.url.clone(),
            |bookmark| {
                vec![
                    bookmark.title.clone(),
                    bookmark.url.clone(),
                    bookmark.folder.clone(),
                ]
            },
        );
        items.extend(matches.into_iter().take(50).map(|bookmark| {
            Item::new(bookmark.url.clone(), bookmark.title)
                .subtitle(bookmark.url)
                .detail(bookmark.folder)
                .icon(Icon::builtin(icons::BOOKMARK))
        }));

        let status = if empty {
            "No Chrome bookmarks found on this Mac".to_string()
        } else {
            format!("No bookmark matches {query:?}")
        };
        Screen::search(items)
            .placeholder("Search Chrome bookmarks")
            .status(status)
    }

    fn menu(&mut self) -> Screen {
        let items: Vec<Item> = self
            .profiles()
            .iter()
            .enumerate()
            .map(|(index, profile)| {
                let mut item = Item::new(
                    format!("{PROFILE}{}", profile.directory),
                    profile.name.clone(),
                )
                .subtitle(
                    profile
                        .account
                        .clone()
                        .unwrap_or_else(|| profile.directory.clone()),
                )
                .icon(Icon::builtin(icons::BROWSER));
                if let Some(key) = char::from_digit(index as u32 + 1, 10) {
                    item = item.key(key);
                }
                item
            })
            .collect();

        Screen::menu("Chrome profile", items).status("No Chrome profiles found on this Mac")
    }
}

impl Extension for Chrome {
    fn new() -> Self {
        Self {
            support: home_dir().map(|home| home.join(SUPPORT)),
            url: None,
            profiles: Cached::new(),
            bookmarks: Cached::new(),
            query: String::new(),
        }
    }

    fn summoned(&mut self) {
        self.url = host::clipboard_text().and_then(|copied| single_url(&copied));
        host::debug(format!("clipboard url: {:?}", self.url));
        let offers: Vec<Item> = self.offer().into_iter().collect();
        host::set_offers(&offers);
    }

    fn activate(&mut self) -> Response {
        self.query.clear();
        Response::Replace(self.results())
    }

    fn search(&mut self, query: &str) -> Response {
        self.query = query.to_string();
        Response::Replace(self.results())
    }

    fn select(&mut self, id: &str) -> Response {
        if id == OPEN {
            return Response::Push(self.menu());
        }

        if let Some(directory) = id.strip_prefix(PROFILE) {
            let Some(url) = self.url.clone() else {
                return Response::Error("Nothing to open: the clipboard is not a URL".into());
            };
            let args = [format!("--profile-directory={directory}"), url];
            return match run_chrome(&args) {
                Ok(()) => Response::Dismiss,
                Err(error) => Response::Error(format!("Could not start Chrome: {error}")),
            };
        }

        // A bookmark, by its URL.
        host::record_pick(id);
        if let Err(error) = run_chrome(&[id.to_string()]) {
            // Better the default browser than nothing at all.
            host::debug(format!("opening {id} in the default browser: {error}"));
            host::open_url(id);
        }
        Response::Dismiss
    }

    fn dismissed(&mut self) {
        self.url = None;
        self.query.clear();
    }
}

export_extension!(Chrome);

/// Hands `args` to Chrome's binary, wherever it is installed.
///
/// Returns once Chrome has been started, not once it has opened anything:
/// with Chrome already running the binary forwards the request and exits at
/// once, and without, it *is* Chrome.
fn run_chrome(args: &[String]) -> Result<(), String> {
    let mut failure = String::from("Chrome is not installed");
    for binary in CHROME {
        match host::run(binary, args) {
            Ok(()) => return Ok(()),
            Err(error) => failure = error,
        }
    }
    Err(failure)
}

/// `text` if it is exactly one web URL, with nothing around it.
///
/// A scheme and a host are required: `example.com` on its own could be a
/// word, and a line with two links in it is not something to open.
fn single_url(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() || text.chars().any(char::is_whitespace) {
        return None;
    }
    let lower = text.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if host.is_empty() {
        return None;
    }
    Some(text.to_string())
}

// --- Chrome's profiles
// --------------------------------------------------------

#[derive(Deserialize)]
struct LocalState {
    profile: ProfileSection,
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
}

/// Every profile Chrome knows about, `Default` first and the rest in the
/// order Chrome created them.
fn read_profiles(support: &Path) -> Vec<Profile> {
    let file = support.join(PROFILES_DIR).join(LOCAL_STATE);
    let Ok(contents) = std::fs::read_to_string(&file) else {
        return Vec::new();
    };
    match serde_json::from_str::<LocalState>(&contents) {
        Ok(parsed) => profiles(parsed),
        Err(error) => {
            host::warn(format!("could not parse {}: {error}", file.display()));
            Vec::new()
        }
    }
}

fn profiles(state: LocalState) -> Vec<Profile> {
    let mut profiles: Vec<Profile> = state
        .profile
        .info_cache
        .into_iter()
        .map(|(directory, entry)| Profile {
            name: if entry.name.is_empty() {
                directory.clone()
            } else {
                entry.name
            },
            account: Some(entry.user_name).filter(|account| !account.is_empty()),
            directory,
        })
        .collect();
    profiles.sort_by_key(|profile| profile_rank(&profile.directory));
    profiles
}

/// `Default` sorts first, then `Profile N` by number, then anything else by
/// name — so the digit keys stay put between summons.
fn profile_rank(directory: &str) -> (u8, u32, String) {
    if directory == "Default" {
        return (0, 0, String::new());
    }
    match directory
        .strip_prefix("Profile ")
        .and_then(|number| number.parse().ok())
    {
        Some(number) => (1, number, String::new()),
        None => (2, 0, directory.to_string()),
    }
}

// --- Chrome's bookmarks
// -------------------------------------------------------

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

/// Reads every Chromium-family profile on this Mac, de-duplicating by URL so a
/// bookmark synced across profiles appears once.
fn read_bookmarks(support: &Path) -> Vec<Bookmark> {
    let mut bookmarks = Vec::new();
    let mut seen = HashSet::new();

    for browser in BROWSER_DIRS {
        for file in profile_files(&support.join(browser)) {
            let Ok(contents) = std::fs::read_to_string(&file) else {
                continue;
            };
            let Ok(parsed) = serde_json::from_str::<BookmarkFile>(&contents) else {
                host::warn(format!("could not parse {}", file.display()));
                continue;
            };

            // `roots` is a map, so iterate in a fixed order for stable results.
            let mut roots: Vec<(&String, &Node)> = parsed.roots.iter().collect();
            roots.sort_by_key(|(name, _)| root_rank(name));

            for (_, root) in roots {
                flatten(root, "", &mut |bookmark: Bookmark| {
                    if seen.insert(bookmark.url.clone()) {
                        bookmarks.push(bookmark);
                    }
                });
            }
        }
    }

    bookmarks
}

/// Bookmarks-bar entries first: they are the ones the user reaches for.
fn root_rank(name: &str) -> u8 {
    match name {
        "bookmark_bar" => 0,
        "other" => 1,
        _ => 2,
    }
}

/// Every `<browser>/<profile>/Bookmarks` file that exists.
fn profile_files(browser_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(browser_dir) else {
        return Vec::new();
    };

    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path().join("Bookmarks"))
        .filter(|file| file.is_file())
        .collect();
    files.sort();
    files
}

/// Walks the bookmark tree, reporting each URL with the folder path it sits in.
fn flatten(node: &Node, path: &str, found: &mut impl FnMut(Bookmark)) {
    match node.kind.as_str() {
        "url" => {
            if let Some(url) = &node.url {
                let title = if node.name.is_empty() {
                    url.clone()
                } else {
                    node.name.clone()
                };
                found(Bookmark {
                    title,
                    url: url.clone(),
                    folder: path.to_string(),
                });
            }
        }
        _ => {
            let path = match (path.is_empty(), node.name.is_empty()) {
                (_, true) => path.to_string(),
                (true, false) => display_name(&node.name),
                (false, false) => format!("{path} / {}", display_name(&node.name)),
            };
            for child in &node.children {
                flatten(child, &path, found);
            }
        }
    }
}

/// Chrome stores the built-in roots under machine names.
fn display_name(name: &str) -> String {
    match name {
        "Bookmarks bar" | "Bookmarks Bar" => "Bookmarks Bar".to_string(),
        "Other bookmarks" | "Other Bookmarks" => "Other Bookmarks".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lone_web_url_is_recognised() {
        assert_eq!(
            single_url("  https://example.com/path?q=1 \n"),
            Some("https://example.com/path?q=1".to_string())
        );
        assert_eq!(
            single_url("HTTP://Example.com"),
            Some("HTTP://Example.com".to_string())
        );
    }

    #[test]
    fn anything_else_is_not() {
        assert_eq!(single_url(""), None);
        assert_eq!(single_url("example.com"), None);
        assert_eq!(single_url("https://"), None);
        assert_eq!(single_url("https:///path"), None);
        assert_eq!(single_url("ftp://example.com"), None);
        assert_eq!(single_url("see https://example.com"), None);
        assert_eq!(single_url("https://a.com\nhttps://b.com"), None);
    }

    #[test]
    fn profiles_come_out_named_and_in_a_fixed_order() {
        let parsed: LocalState = serde_json::from_str(
            r#"{"profile":{"info_cache":{
                "Profile 10":{"name":"Ten","user_name":"ten@example.com"},
                "Profile 2":{"name":"Work"},
                "Default":{"name":"Personal","user_name":"me@example.com"},
                "Guest":{}
            }}}"#,
        )
        .unwrap();
        let profiles = profiles(parsed);
        let directories: Vec<&str> = profiles.iter().map(|p| p.directory.as_str()).collect();
        assert_eq!(directories, ["Default", "Profile 2", "Profile 10", "Guest"]);
        assert_eq!(profiles[0].account.as_deref(), Some("me@example.com"));
        assert_eq!(profiles[1].account, None);
        assert_eq!(profiles[3].name, "Guest");
    }

    #[test]
    fn a_local_state_without_profiles_is_empty() {
        let parsed: LocalState = serde_json::from_str(r#"{"profile":{}}"#).unwrap();
        assert!(profiles(parsed).is_empty());
    }

    const SAMPLE: &str = r#"{
        "roots": {
            "bookmark_bar": {
                "type": "folder",
                "name": "Bookmarks bar",
                "children": [
                    { "type": "url", "name": "Rust", "url": "https://rust-lang.org" },
                    {
                        "type": "folder",
                        "name": "Work",
                        "children": [
                            { "type": "url", "name": "Repo", "url": "https://github.com" }
                        ]
                    }
                ]
            }
        }
    }"#;

    #[test]
    fn folders_become_a_readable_path() {
        let parsed: BookmarkFile = serde_json::from_str(SAMPLE).unwrap();
        let mut found = Vec::new();
        flatten(&parsed.roots["bookmark_bar"], "", &mut |bookmark| {
            found.push(bookmark)
        });

        assert_eq!(found.len(), 2);
        assert_eq!(found[0].title, "Rust");
        assert_eq!(found[0].folder, "Bookmarks Bar");
        assert_eq!(found[1].folder, "Bookmarks Bar / Work");
    }

    #[test]
    fn a_nameless_bookmark_falls_back_to_its_url() {
        let parsed: BookmarkFile = serde_json::from_str(
            r#"{"roots":{"other":{"type":"folder","name":"","children":[
                {"type":"url","name":"","url":"https://example.com"}]}}}"#,
        )
        .unwrap();
        let mut found = Vec::new();
        flatten(&parsed.roots["other"], "", &mut |bookmark| {
            found.push(bookmark)
        });
        assert_eq!(found[0].title, "https://example.com");
    }
}
