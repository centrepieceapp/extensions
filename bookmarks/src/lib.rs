//! Browser bookmarks, under the `bb` prefix: pick a browser, then a profile,
//! then a bookmark, which opens in that browser and that profile.
//!
//! The browsers listed are the ones installed — whose bundle is in
//! `/Applications` or `~/Applications` — out of those [`BROWSERS`] knows how to
//! read. A browser with one profile skips straight to its bookmarks.
//!
//! Bookmarks are shown folder by folder, the folders first; picking one opens
//! it, and Backspace steps back out. Typing searches everything inside the
//! folder that is open, however deep ([`folders`]).
//!
//! Three families keep bookmarks three ways, each in its own module:
//! Chromium's a JSON file per profile ([`chromium`]), Firefox's an SQLite
//! database per profile, read through the system's `sqlite3` ([`firefox`]),
//! and Safari's one property list for every profile ([`safari`]), which macOS
//! only lets Centrepiece read with Full Disk Access.

mod chromium;
mod firefox;
mod folders;
mod safari;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use centrepiece_extension::{
    Extension, Icon, Item, Response, Screen, TaskId, TaskResult, Tasks, export_extension, home_dir,
    host,
};

/// Opens a URL in a given application, for Safari, and for any browser whose
/// own binary will not start.
const OPEN: &str = "/usr/bin/open";

/// Row ids on each screen.
const BROWSER: &str = "browser:";
const PROFILE: &str = "profile:";
const FOLDER: &str = "folder:";

/// From the extension's `assets/`.
const FOLDER_ICON: &str = "folder.svg";

/// Tailwind's amber-200 and rose-300, as `0xRRGGBBAA`.
const FOLDER_TINT: u32 = 0xFEE6_85FF;
const BOOKMARK_TINT: u32 = 0xFFA1_ADFF;
const FULL_DISK_ACCESS: &str = "full-disk-access";

/// System Settings, at the Full Disk Access list.
const FULL_DISK_ACCESS_SETTINGS: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Chromium,
    Firefox,
    Safari,
}

/// A browser this extension knows how to read.
struct Browser {
    id: &'static str,
    name: &'static str,
    /// The bundle's file name in an Applications folder.
    app: &'static str,
    /// The executable in the bundle's `Contents/MacOS`.
    binary: &'static str,
    /// Where it keeps its profiles, relative to the home folder.
    data: &'static str,
    family: Family,
}

const fn browser(
    id: &'static str,
    name: &'static str,
    app: &'static str,
    binary: &'static str,
    data: &'static str,
    family: Family,
) -> Browser {
    Browser {
        id,
        name,
        app,
        binary,
        data,
        family,
    }
}

/// Every browser looked for, in the order they are listed. The manifest has
/// to grant each one's bundle, data folder and binary.
const BROWSERS: &[Browser] = &[
    browser(
        "safari",
        "Safari",
        "Safari.app",
        "Safari",
        "Library/Safari",
        Family::Safari,
    ),
    browser(
        "chrome",
        "Google Chrome",
        "Google Chrome.app",
        "Google Chrome",
        "Library/Application Support/Google/Chrome",
        Family::Chromium,
    ),
    browser(
        "chrome-beta",
        "Google Chrome Beta",
        "Google Chrome Beta.app",
        "Google Chrome Beta",
        "Library/Application Support/Google/Chrome Beta",
        Family::Chromium,
    ),
    browser(
        "chrome-canary",
        "Google Chrome Canary",
        "Google Chrome Canary.app",
        "Google Chrome Canary",
        "Library/Application Support/Google/Chrome Canary",
        Family::Chromium,
    ),
    browser(
        "chromium",
        "Chromium",
        "Chromium.app",
        "Chromium",
        "Library/Application Support/Chromium",
        Family::Chromium,
    ),
    browser(
        "brave",
        "Brave",
        "Brave Browser.app",
        "Brave Browser",
        "Library/Application Support/BraveSoftware/Brave-Browser",
        Family::Chromium,
    ),
    browser(
        "edge",
        "Microsoft Edge",
        "Microsoft Edge.app",
        "Microsoft Edge",
        "Library/Application Support/Microsoft Edge",
        Family::Chromium,
    ),
    browser(
        "vivaldi",
        "Vivaldi",
        "Vivaldi.app",
        "Vivaldi",
        "Library/Application Support/Vivaldi",
        Family::Chromium,
    ),
    browser(
        "opera",
        "Opera",
        "Opera.app",
        "Opera",
        "Library/Application Support/com.operasoftware.Opera",
        Family::Chromium,
    ),
    browser(
        "helium",
        "Helium",
        "Helium.app",
        "Helium",
        "Library/Application Support/net.imput.helium",
        Family::Chromium,
    ),
    browser(
        "firefox",
        "Firefox",
        "Firefox.app",
        "firefox",
        "Library/Application Support/Firefox",
        Family::Firefox,
    ),
    browser(
        "zen",
        "Zen",
        "Zen.app",
        "zen",
        "Library/Application Support/zen",
        Family::Firefox,
    ),
    browser(
        "librewolf",
        "LibreWolf",
        "LibreWolf.app",
        "librewolf",
        "Library/Application Support/librewolf",
        Family::Firefox,
    ),
];

/// A browser found on this Mac.
struct Installed {
    browser: &'static Browser,
    /// The bundle, e.g. `/Applications/Google Chrome.app`.
    bundle: PathBuf,
    /// Its data folder.
    data: PathBuf,
}

impl Installed {
    fn executable(&self) -> String {
        self.bundle
            .join("Contents/MacOS")
            .join(self.browser.binary)
            .to_string_lossy()
            .into_owned()
    }

    fn profiles(&self) -> Vec<Profile> {
        match self.browser.family {
            Family::Chromium => chromium::profiles(&self.data),
            Family::Firefox => firefox::profiles(&self.data),
            Family::Safari => vec![safari::profile(&self.data)],
        }
    }

    /// Opens `url` in `profile`, in this browser if at all possible.
    fn open(&self, profile: &Profile, url: &str) -> Result<(), String> {
        let mut args = profile.launch.clone();
        args.push(url.to_string());
        let own = match self.browser.family {
            // Safari takes no arguments; `open` is the way in.
            Family::Safari => Err(String::new()),
            _ => host::run(&self.executable(), &args),
        };
        own.or_else(|error| {
            if !error.is_empty() {
                host::debug(format!("{} would not start: {error}", self.browser.name));
            }
            let bundle = self.bundle.to_string_lossy().into_owned();
            host::run(OPEN, &["-a".to_string(), bundle, url.to_string()])
        })
    }
}

/// One profile of a browser.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Profile {
    /// What the browser calls it.
    name: String,
    /// The account it is signed in to, or its folder.
    subtitle: Option<String>,
    /// The folder its bookmarks are in.
    dir: PathBuf,
    /// The arguments that tell the browser's binary to use it.
    launch: Vec<String>,
    /// Its picture, or its colour; the browser's icon when it has neither.
    avatar: Option<Icon>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Bookmark {
    title: String,
    url: String,
    /// The folders it sits in, outermost first: `["Bookmarks Bar", "Rust"]`.
    folder: Vec<String>,
}

/// Drops repeated URLs from one screen, keeping the first: one row per URL
/// keeps row ids unique, and a bookmark filed twice is still one page.
fn dedupe<'a>(bookmarks: impl IntoIterator<Item = &'a Bookmark>) -> Vec<&'a Bookmark> {
    let mut seen = HashSet::new();
    bookmarks
        .into_iter()
        .filter(|bookmark| seen.insert(bookmark.url.as_str()))
        .collect()
}

/// Where the user is, one entry per screen pushed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Browsers,
    Profiles,
    /// A profile's bookmarks, at the folder they open in.
    Bookmarks,
    /// A folder opened from there.
    Folder,
}

struct Bookmarks {
    home: Option<PathBuf>,
    browsers: Vec<Installed>,
    /// Index into `browsers` of the one chosen.
    browser: Option<usize>,
    profiles: Vec<Profile>,
    profile: Option<Profile>,
    /// The profile's bookmarks, once read.
    bookmarks: Option<Vec<Bookmark>>,
    /// The folder open, as a path from the top.
    folder: Vec<String>,
    query: String,
    views: Vec<View>,
    /// Firefox bookmarks being read.
    tasks: Tasks<()>,
}

impl Bookmarks {
    fn installed(&self) -> Vec<Installed> {
        let Some(home) = &self.home else {
            return Vec::new();
        };
        BROWSERS
            .iter()
            .filter_map(|browser| {
                let bundle = [
                    PathBuf::from("/Applications").join(browser.app),
                    home.join("Applications").join(browser.app),
                ]
                .into_iter()
                .find(|bundle| exists(bundle))?;
                Some(Installed {
                    browser,
                    bundle,
                    data: home.join(browser.data),
                })
            })
            .collect()
    }

    fn browsers_screen(&self) -> Screen {
        let items = self
            .browsers
            .iter()
            .map(|installed| {
                Item::new(
                    format!("{BROWSER}{}", installed.browser.id),
                    installed.browser.name,
                )
                .icon(Icon::File(installed.bundle.to_string_lossy().into_owned()))
            })
            .collect();
        Screen::search(items)
            .host_filtered()
            .placeholder("Choose a browser")
            .status("No browser with bookmarks Centrepiece can read is installed")
    }

    fn current(&self) -> Option<&Installed> {
        self.browsers.get(self.browser?)
    }

    fn profiles_screen(&self, installed: &Installed) -> Screen {
        let items =
            self.profiles
                .iter()
                .enumerate()
                .map(|(index, profile)| {
                    let icon = profile.avatar.clone().unwrap_or_else(|| {
                        Icon::File(installed.bundle.to_string_lossy().into_owned())
                    });
                    let mut item =
                        Item::new(format!("{PROFILE}{index}"), profile.name.clone()).icon(icon);
                    if let Some(subtitle) = &profile.subtitle {
                        item = item.subtitle(subtitle.clone());
                    }
                    if let Some(key) = char::from_digit(index as u32 + 1, 10) {
                        item = item.key(key);
                    }
                    item
                })
                .collect();
        Screen::menu(format!("{} profile", installed.browser.name), items)
    }

    fn breadcrumb(&self) -> String {
        let browser = self
            .current()
            .map_or("", |installed| installed.browser.name);
        match &self.profile {
            Some(profile) if self.profiles.len() > 1 => format!("{browser} · {}", profile.name),
            _ => browser.to_string(),
        }
    }

    /// Where the user is: the profile, and the folder open in it.
    fn location(&self) -> String {
        match self.folder.last() {
            Some(folder) => format!("{} › {folder}", self.breadcrumb()),
            None => self.breadcrumb(),
        }
    }

    /// The folder open: its folders, then its bookmarks — or, with something
    /// typed, every bookmark inside it that matches, however deep.
    fn folder_screen(&self) -> Screen {
        let bookmarks = self.bookmarks.as_deref().unwrap_or_default();
        let query = self.query.trim();
        let screen = Screen::search(Vec::new())
            .title(self.location())
            .placeholder("Search bookmarks");

        if !query.is_empty() {
            let found = host::rank(
                query,
                dedupe(folders::under(bookmarks, &self.folder)),
                |bookmark| bookmark.url.clone(),
                |bookmark| {
                    vec![
                        bookmark.title.clone(),
                        bookmark.url.clone(),
                        bookmark.folder.join(" "),
                    ]
                },
            );
            let depth = self.folder.len();
            let items = found
                .into_iter()
                .take(50)
                .map(|bookmark| {
                    // Where it is from here; nothing for this folder's own.
                    let item = bookmark_item(bookmark);
                    match &bookmark.folder[depth..] {
                        [] => item,
                        inner => item.detail(inner.join(" › ")),
                    }
                })
                .collect();
            return Screen {
                items,
                ..screen.status(format!("No bookmark matches {query:?}"))
            };
        }

        let (folders, own) = folders::level(bookmarks, &self.folder);
        let mut items: Vec<Item> = folders
            .into_iter()
            .map(|folder| {
                let count = match folder.count {
                    1 => "1 bookmark".to_string(),
                    count => format!("{count} bookmarks"),
                };
                Item::new(format!("{FOLDER}{}", folder.name), folder.name)
                    .subtitle(count)
                    .icon(Icon::asset(FOLDER_ICON))
                    .tint(FOLDER_TINT)
            })
            .collect();
        items.extend(dedupe(own).into_iter().map(bookmark_item));
        let status = if self.folder.is_empty() {
            "No bookmarks in this profile"
        } else {
            "This folder is empty"
        };
        Screen {
            items,
            ..screen.status(status)
        }
    }

    /// The profile's bookmarks are in: open them where they start.
    fn loaded(&mut self, bookmarks: Vec<Bookmark>) -> Screen {
        self.folder = folders::start(&bookmarks);
        self.bookmarks = Some(bookmarks);
        self.folder_screen()
    }

    /// One row, leading to the setting that lets Centrepiece read Safari's
    /// bookmarks.
    fn no_access_screen(&self) -> Screen {
        let item = Item::new(FULL_DISK_ACCESS, "Open Full Disk Access settings")
            .subtitle("Centrepiece needs it to read Safari's bookmarks; turn it on, then restart Centrepiece")
            .icon(Icon::builtin("settings"));
        Screen::search(vec![item])
            .host_filtered()
            .title(self.breadcrumb())
    }

    fn failed_screen(&self, why: String) -> Screen {
        Screen::search(Vec::new())
            .title(self.breadcrumb())
            .status(why)
    }

    /// Opens the bookmarks of `profile`: at once, or for the Firefox family,
    /// once `sqlite3` has read them.
    fn enter_profile(&mut self, profile: Profile) -> Response {
        let Some(installed) = self.current() else {
            return Response::None;
        };
        let family = installed.browser.family;
        let name = installed.browser.name;
        self.profile = Some(profile.clone());
        self.bookmarks = None;
        self.folder.clear();
        self.query.clear();
        self.views.push(View::Bookmarks);
        let screen = match family {
            Family::Chromium => match chromium::bookmarks(&profile.dir) {
                Ok(bookmarks) => self.loaded(bookmarks),
                Err(why) => self.failed_screen(why),
            },
            Family::Safari => match safari::bookmarks(&profile.dir) {
                Ok(bookmarks) => self.loaded(bookmarks),
                Err(safari::Unreadable::NoAccess) => self.no_access_screen(),
                Err(safari::Unreadable::Broken(why)) => self.failed_screen(why),
            },
            Family::Firefox => {
                self.tasks.clear();
                self.tasks.insert(firefox::read(&profile.dir), ());
                Screen::search(Vec::new())
                    .title(self.breadcrumb())
                    .placeholder("Search bookmarks")
                    .status(format!("Reading {name} bookmarks…"))
                    .loading(true)
            }
        };
        Response::Push(screen)
    }

    fn reset(&mut self) {
        self.browser = None;
        self.profiles.clear();
        self.profile = None;
        self.bookmarks = None;
        self.folder.clear();
        self.query.clear();
        self.views.clear();
        self.tasks.clear();
    }
}

impl Extension for Bookmarks {
    fn new() -> Self {
        Self {
            home: home_dir(),
            browsers: Vec::new(),
            browser: None,
            profiles: Vec::new(),
            profile: None,
            bookmarks: None,
            folder: Vec::new(),
            query: String::new(),
            views: Vec::new(),
            tasks: Tasks::new(),
        }
    }

    fn activate(&mut self) -> Response {
        self.reset();
        self.browsers = self.installed();
        self.views.push(View::Browsers);
        Response::Replace(self.browsers_screen())
    }

    fn search(&mut self, query: &str) -> Response {
        self.query = query.to_string();
        let in_bookmarks = matches!(self.views.last(), Some(View::Bookmarks | View::Folder));
        if in_bookmarks && self.bookmarks.is_some() {
            Response::Replace(self.folder_screen())
        } else {
            Response::None
        }
    }

    fn select(&mut self, id: &str) -> Response {
        if let Some(browser) = id.strip_prefix(BROWSER) {
            let Some(index) = self
                .browsers
                .iter()
                .position(|installed| installed.browser.id == browser)
            else {
                return Response::None;
            };
            self.browser = Some(index);
            self.profile = None;
            let installed = &self.browsers[index];
            self.profiles = installed.profiles();
            return match self.profiles.len() {
                0 => Response::Error(format!(
                    "{} has no profiles yet; open it once first",
                    installed.browser.name
                )),
                1 => self.enter_profile(self.profiles[0].clone()),
                _ => {
                    let screen = self.profiles_screen(installed);
                    self.views.push(View::Profiles);
                    Response::Push(screen)
                }
            };
        }

        if let Some(index) = id.strip_prefix(PROFILE) {
            let Some(profile) = index
                .parse::<usize>()
                .ok()
                .and_then(|index| self.profiles.get(index))
            else {
                return Response::None;
            };
            return self.enter_profile(profile.clone());
        }

        if let Some(name) = id.strip_prefix(FOLDER) {
            self.folder.push(name.to_string());
            self.query.clear();
            self.views.push(View::Folder);
            return Response::Push(self.folder_screen());
        }

        if id == FULL_DISK_ACCESS {
            host::open_url(FULL_DISK_ACCESS_SETTINGS);
            return Response::Dismiss;
        }

        // A bookmark, by its URL.
        host::record_pick(id);
        let (Some(installed), Some(profile)) = (self.current(), &self.profile) else {
            return Response::None;
        };
        if let Err(error) = installed.open(profile, id) {
            // Better the default browser than nothing at all.
            host::debug(format!("opening {id} in the default browser: {error}"));
            host::open_url(id);
        }
        Response::Dismiss
    }

    fn task_finished(&mut self, task: TaskId, result: TaskResult) -> Response {
        if self.tasks.take(task).is_none() || self.views.last() != Some(&View::Bookmarks) {
            return Response::None;
        }
        let screen = match firefox::parse(result) {
            Ok(bookmarks) => self.loaded(bookmarks),
            Err(why) => self.failed_screen(why),
        };
        Response::Replace(screen)
    }

    fn popped(&mut self) {
        // The screen underneath comes back as it was, query and all.
        match self.views.pop() {
            Some(View::Folder) => {
                self.folder.pop();
                self.query.clear();
            }
            Some(View::Bookmarks) => {
                self.profile = None;
                self.bookmarks = None;
                self.folder.clear();
                self.query.clear();
                self.tasks.clear();
            }
            _ => {}
        }
    }

    fn dismissed(&mut self) {
        self.reset();
    }
}

export_extension!(Bookmarks);

/// Whether something is at `path`, without following a symlink out of what
/// the sandbox can see: `/Applications/Safari.app` is one.
fn exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

fn bookmark_item(bookmark: &Bookmark) -> Item {
    Item::new(bookmark.url.clone(), bookmark.title.clone())
        .subtitle(bookmark.url.clone())
        .icon(Icon::builtin("bookmark"))
        .tint(BOOKMARK_TINT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bookmark(url: &str, folder: &str) -> Bookmark {
        Bookmark {
            title: url.to_string(),
            url: url.to_string(),
            folder: vec![folder.to_string()],
        }
    }

    #[test]
    fn a_url_filed_twice_is_listed_once() {
        let bookmarks = [
            bookmark("https://a.com", "Bar"),
            bookmark("https://b.com", "Bar"),
            bookmark("https://a.com", "Other"),
        ];
        let kept: Vec<&str> = dedupe(&bookmarks)
            .iter()
            .map(|bookmark| bookmark.folder[0].as_str())
            .collect();
        assert_eq!(kept, ["Bar", "Bar"]);
    }

    #[test]
    fn every_browser_has_a_unique_id() {
        let ids: HashSet<&str> = BROWSERS.iter().map(|browser| browser.id).collect();
        assert_eq!(ids.len(), BROWSERS.len());
    }

    /// The manifest has to grant what the code reaches for, or a browser
    /// silently goes missing.
    #[test]
    fn the_manifest_grants_every_browser() {
        let manifest = include_str!("../extension.toml");
        for browser in BROWSERS {
            let data = format!(
                "\"~/{}",
                browser
                    .data
                    .split('/')
                    .take(3)
                    .collect::<Vec<_>>()
                    .join("/")
            );
            assert!(manifest.contains(&data), "{} data: {data}", browser.id);
            assert!(
                manifest.contains(&format!("\"/Applications/{}\"", browser.app)),
                "{} bundle",
                browser.id
            );
            if browser.family != Family::Safari {
                assert!(
                    manifest.contains(&format!(
                        "\"/Applications/{}/Contents/MacOS/{}\"",
                        browser.app, browser.binary
                    )),
                    "{} binary",
                    browser.id
                );
            }
        }
    }
}
