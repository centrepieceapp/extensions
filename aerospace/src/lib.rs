//! Moving windows between AeroSpace workspaces and switching their layout,
//! under the `wm` prefix.
//!
//! Everything goes through the `aerospace` command line client, which the
//! manifest lets the extension run from where Homebrew puts it. Which of those
//! places holds it is found out once, at startup; on a Mac without it the
//! extension says so instead of listing anything.
//!
//! "The current window" is the awkward part: by the time a row is picked
//! Centrepiece is the application in front. So Centrepiece notes what was in
//! front at every summon, before the panel is presented, and the extension acts
//! on that.

use centrepiece_extension::{
    App, Extension, Icon, Item, Output, Response, Screen, TaskId, TaskResult, Tasks,
    export_extension, host,
};
use serde::Deserialize;

/// Where the client may be, in the order they are tried; the manifest lists
/// the same. Homebrew puts it in the first on Apple silicon, the second on
/// Intel.
const CLIENTS: &[&str] = &["/opt/homebrew/bin/aerospace", "/usr/local/bin/aerospace"];

/// Item ids.
const ACTION_MOVE: &str = "action:move";
const ACTION_LAYOUT: &str = "action:layout";
const WORKSPACE_ITEM: &str = "workspace:";
const LAYOUT_ITEM: &str = "layout:";

/// Centrepiece's own icons, by name.
mod icons {
    pub const LAYOUT_GRID: &str = "layout-grid";
    pub const MOVE_WINDOW: &str = "move-window";
    pub const SWITCH_LAYOUT: &str = "switch-layout";
    pub const TILES_HORIZONTAL: &str = "tiles-horizontal";
    pub const TILES_VERTICAL: &str = "tiles-vertical";
    pub const ACCORDION_HORIZONTAL: &str = "accordion-horizontal";
    pub const ACCORDION_VERTICAL: &str = "accordion-vertical";
}

/// One of the ways AeroSpace arranges the windows of a container.
struct Layout {
    /// What `aerospace` calls it.
    id: &'static str,
    name: &'static str,
    description: &'static str,
    icon: &'static str,
    key: char,
}

const LAYOUTS: &[Layout] = &[
    Layout {
        id: "h_tiles",
        name: "Horizontal tiles",
        description: "Windows side by side — i3's horizontal split",
        icon: icons::TILES_HORIZONTAL,
        key: 'h',
    },
    Layout {
        id: "v_tiles",
        name: "Vertical tiles",
        description: "Windows one above the other — i3's vertical split",
        icon: icons::TILES_VERTICAL,
        key: 'v',
    },
    Layout {
        id: "h_accordion",
        name: "Horizontal accordion",
        description: "One window in front, the rest at its sides — i3's tabbed layout",
        icon: icons::ACCORDION_HORIZONTAL,
        key: 'a',
    },
    Layout {
        id: "v_accordion",
        name: "Vertical accordion",
        description: "One window in front, the rest above and below — i3's stacked layout",
        icon: icons::ACCORDION_VERTICAL,
        key: 's',
    },
];

/// The submenus, both of which act on the window that was in front.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Menu {
    Move,
    Layout,
}

impl Menu {
    fn title(self, app: &str) -> String {
        match self {
            Menu::Move => format!("Move {app} to workspace"),
            Menu::Layout => "Switch layout".to_string(),
        }
    }
}

/// Where the `aerospace` client is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Client {
    /// Still being looked for.
    Looking,
    Found(&'static str),
    Missing,
}

/// What each task in flight was started for.
enum Pending {
    /// Whether the client is at `CLIENTS[index]`.
    Probe(usize),
    Focused,
    Workspaces,
    Windows,
    /// A move or a layout switch.
    Done,
}

/// A submenu waiting on AeroSpace's answers.
struct Loading {
    menu: Menu,
    workspaces: Option<Vec<Workspace>>,
    windows: Option<Vec<Window>>,
}

struct AeroSpace {
    client: Client,
    /// The application that was in front at the last summon.
    front: Option<App>,
    /// The window AeroSpace had focused at the last summon, if its answer
    /// came back. It says *which* of the application's windows to move.
    focused: Option<Window>,
    /// Whether the user is inside the extension, so a late answer has a screen
    /// to land on.
    entered: bool,
    /// The submenu showing, if any.
    menu: Option<Menu>,
    loading: Option<Loading>,
    /// The window the open submenu is about to act on.
    target: Option<Window>,
    tasks: Tasks<Pending>,
}

/// What the submenus are drawn from.
struct Overview {
    target: Window,
    workspaces: Vec<Workspace>,
    windows: Vec<Window>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Window {
    window_id: u64,
    app_pid: i32,
    app_name: String,
    workspace: String,
    /// The layout of the container the window sits in, or `floating`.
    #[serde(default)]
    window_layout: String,
}

#[derive(Debug, Clone, Deserialize)]
struct Workspace {
    #[serde(rename = "workspace")]
    name: String,
}

const WINDOW_FORMAT: &str = "%{window-id}%{app-pid}%{app-name}%{workspace}%{window-layout}";

impl AeroSpace {
    fn probe(&mut self, index: usize) {
        let task = host::exec(CLIENTS[index], &["--help"]);
        self.tasks.insert(task, Pending::Probe(index));
    }

    /// Runs the client, noting what for. `false` when there is none to run.
    fn exec(&mut self, arguments: &[&str], what: Pending) -> bool {
        let Client::Found(client) = self.client else {
            return false;
        };
        let task = host::exec(client, arguments);
        self.tasks.insert(task, what);
        true
    }

    /// Asked without waiting: the summon must not stall on another process.
    /// AeroSpace does not count the panel as a window, so in practice it
    /// still names the user's one even if the panel is up by the time it
    /// answers; an answer for any other application is ignored and the front
    /// application's first window moves instead.
    fn ask_focused(&mut self) {
        self.exec(
            &[
                "list-windows",
                "--focused",
                "--json",
                "--format",
                WINDOW_FORMAT,
            ],
            Pending::Focused,
        );
    }

    fn actions(&self) -> Vec<Item> {
        let Some(front) = &self.front else {
            return Vec::new();
        };
        // Known once AeroSpace has said which window was focused, a moment
        // after the summon.
        let layout = self
            .focused
            .as_ref()
            .filter(|window| window.app_pid == front.pid)
            .map(|window| {
                format!(
                    "Workspace {} · {}",
                    window.workspace,
                    layout_name(&window.window_layout)
                )
            })
            .unwrap_or_else(|| "Change how the workspace arranges its windows".to_string());

        vec![
            Item::new(ACTION_MOVE, Menu::Move.title(&front.name))
                .subtitle("Send the window you were in to another AeroSpace workspace")
                .icon(Icon::builtin(icons::MOVE_WINDOW)),
            Item::new(ACTION_LAYOUT, Menu::Layout.title(&front.name))
                .subtitle(layout)
                .icon(Icon::builtin(icons::SWITCH_LAYOUT)),
        ]
    }

    fn root(&self) -> Screen {
        let status = match self.client {
            Client::Looking => "Looking for AeroSpace…",
            Client::Missing => {
                "AeroSpace's command line client is not in /opt/homebrew/bin or /usr/local/bin"
            }
            Client::Found(_) => "No application was in front when Centrepiece came up",
        };
        let items = match self.client {
            Client::Found(_) => self.actions(),
            _ => Vec::new(),
        };
        Screen::search(items)
            .host_filtered()
            .placeholder("Search window actions")
            .status(status)
            .loading(self.client == Client::Looking)
    }

    /// The root again, if that is the screen showing.
    fn refresh_root(&self) -> Response {
        if self.entered && self.menu.is_none() {
            Response::Replace(self.root())
        } else {
            Response::None
        }
    }

    fn open(&mut self, menu: Menu) -> Response {
        let Some(front) = &self.front else {
            return Response::None;
        };
        let title = menu.title(&front.name);

        self.forget_menu();
        let asked = self.exec(
            &[
                "list-workspaces",
                "--all",
                "--json",
                "--format",
                "%{workspace}",
            ],
            Pending::Workspaces,
        ) && self.exec(
            &["list-windows", "--all", "--json", "--format", WINDOW_FORMAT],
            Pending::Windows,
        );
        if !asked {
            return Response::None;
        }

        self.menu = Some(menu);
        self.loading = Some(Loading {
            menu,
            workspaces: None,
            windows: None,
        });
        Response::Push(
            Screen::menu(title, Vec::new())
                .status("Asking AeroSpace…")
                .loading(true),
        )
    }

    /// Fills the submenu once both of AeroSpace's answers are in.
    fn loaded(&mut self, answer: Result<Answer, String>) -> Response {
        let (Some(loading), Some(front)) = (&mut self.loading, &self.front) else {
            return Response::None;
        };
        let menu = loading.menu;
        let title = menu.title(&front.name);

        match answer {
            Ok(Answer::Workspaces(workspaces)) => loading.workspaces = Some(workspaces),
            Ok(Answer::Windows(windows)) => loading.windows = Some(windows),
            Err(error) => {
                self.loading = None;
                return Response::Replace(Screen::menu(title, Vec::new()).status(error));
            }
        }
        let (Some(_), Some(_)) = (&loading.workspaces, &loading.windows) else {
            return Response::None;
        };
        let loading = self.loading.take().expect("matched above");
        let mut windows = loading.windows.expect("matched above");
        // The panel is not something a workspace "holds".
        windows.retain(|window| !window.app_name.eq_ignore_ascii_case("centrepiece"));

        let Some(target) = target(front, self.focused.clone(), &windows) else {
            let error = format!("AeroSpace is not managing a window of {}", front.name);
            return Response::Replace(Screen::menu(title, Vec::new()).status(error));
        };
        let overview = Overview {
            target,
            workspaces: loading.workspaces.expect("matched above"),
            windows,
        };
        let items = match menu {
            Menu::Move => workspace_items(&overview),
            Menu::Layout => layout_items(&overview.target.window_layout),
        };
        self.target = Some(overview.target);
        Response::Replace(Screen::menu(title, items))
    }

    /// Forgets the submenu, and any answer on its way to it.
    fn forget_menu(&mut self) {
        self.tasks
            .forget(|pending| matches!(pending, Pending::Workspaces | Pending::Windows));
        self.menu = None;
        self.loading = None;
        self.target = None;
    }
}

/// One of the lists a submenu is drawn from.
enum Answer {
    Workspaces(Vec<Workspace>),
    Windows(Vec<Window>),
}

impl Extension for AeroSpace {
    fn new() -> Self {
        Self {
            client: Client::Looking,
            front: None,
            focused: None,
            entered: false,
            menu: None,
            loading: None,
            target: None,
            tasks: Tasks::new(),
        }
    }

    fn started(&mut self) {
        self.probe(0);
    }

    fn summoned(&mut self) {
        self.front = host::front_app();
        self.focused = None;
        self.forget_menu();
        self.ask_focused();
    }

    fn activate(&mut self) -> Response {
        self.entered = true;
        self.forget_menu();
        Response::Replace(self.root())
    }

    fn select(&mut self, id: &str) -> Response {
        match id {
            ACTION_MOVE => return self.open(Menu::Move),
            ACTION_LAYOUT => return self.open(Menu::Layout),
            _ => {}
        }

        let Some(target) = &self.target else {
            return Response::None;
        };
        let window = target.window_id.to_string();

        if let Some(layout) = id.strip_prefix(LAYOUT_ITEM) {
            self.exec(&["layout", "--window-id", &window, layout], Pending::Done);
            return Response::None;
        }

        let Some(workspace) = id.strip_prefix(WORKSPACE_ITEM) else {
            return Response::None;
        };
        if workspace == target.workspace {
            return Response::Error(format!(
                "{} is already on workspace {workspace}",
                target.app_name
            ));
        }
        self.exec(
            &[
                "move-node-to-workspace",
                // Dismissing hands the keyboard back to the window that just
                // moved, so the view would end up there regardless. Saying so
                // makes it one switch instead of a flicker.
                "--focus-follows-window",
                "--window-id",
                &window,
                workspace,
            ],
            Pending::Done,
        );
        Response::None
    }

    fn task_finished(&mut self, task: TaskId, result: TaskResult) -> Response {
        let Some(pending) = self.tasks.take(task) else {
            return Response::None;
        };
        let TaskResult::Output(output) = result else {
            return Response::None;
        };

        match pending {
            Pending::Probe(index) => {
                match output {
                    Ok(_) => {
                        host::info(format!("using {}", CLIENTS[index]));
                        self.client = Client::Found(CLIENTS[index]);
                        // A summon that came first went unanswered.
                        if self.front.is_some() {
                            self.ask_focused();
                        }
                    }
                    Err(_) if index + 1 < CLIENTS.len() => {
                        self.probe(index + 1);
                        return Response::None;
                    }
                    Err(error) => {
                        host::info(format!("no AeroSpace on this Mac: {error}"));
                        self.client = Client::Missing;
                    }
                }
                self.refresh_root()
            }
            Pending::Focused => {
                // Fails outright when nothing is focused, which is an answer
                // too.
                self.focused = parse::<Window>(output)
                    .ok()
                    .and_then(|windows| windows.into_iter().next());
                host::debug(format!(
                    "in front: {:?}; focused in AeroSpace: {:?}",
                    self.front.as_ref().map(|app| &app.name),
                    self.focused
                ));
                // The layout row has been waiting for this.
                self.refresh_root()
            }
            Pending::Workspaces => self.loaded(parse(output).map(Answer::Workspaces)),
            Pending::Windows => self.loaded(parse(output).map(Answer::Windows)),
            Pending::Done => match run(output) {
                Ok(_) => Response::Dismiss,
                Err(error) => Response::Error(error),
            },
        }
    }

    fn popped(&mut self) {
        self.forget_menu();
    }

    fn dismissed(&mut self) {
        self.entered = false;
        self.forget_menu();
    }
}

export_extension!(AeroSpace);

// --- Reading the client's answers -------------------------------------------

/// What the client printed, or why it did not do as asked.
fn run(output: Result<Output, String>) -> Result<Output, String> {
    let output = output.map_err(|err| format!("Could not run aerospace: {err}"))?;
    if !output.is_success() {
        let stderr = output.stderr();
        let reason = stderr.lines().next().unwrap_or("no reason given").trim();
        return Err(format!("AeroSpace refused: {reason}"));
    }
    Ok(output)
}

fn parse<T: serde::de::DeserializeOwned>(output: Result<Output, String>) -> Result<Vec<T>, String> {
    run(output)?
        .json()
        .map_err(|err| format!("AeroSpace sent an answer we could not read: {err}"))
}

// --- Rows -------------------------------------------------------------------

/// A layout as a person would say it; AeroSpace's own word for anything that
/// is not one of the four, which in practice means `floating`.
fn layout_name(id: &str) -> String {
    match LAYOUTS.iter().find(|layout| layout.id == id) {
        Some(layout) => layout.name.to_string(),
        None if id.is_empty() => "Unknown layout".to_string(),
        None => {
            let mut name = id.replace('_', " ");
            if let Some(first) = name.get_mut(..1) {
                first.make_ascii_uppercase();
            }
            name
        }
    }
}

/// Every layout but the one already in use.
fn layout_items(current: &str) -> Vec<Item> {
    LAYOUTS
        .iter()
        .filter(|layout| layout.id != current)
        .map(|layout| {
            Item::new(format!("{LAYOUT_ITEM}{}", layout.id), layout.name)
                .subtitle(layout.description)
                .icon(Icon::builtin(layout.icon))
                .key(layout.key)
        })
        .collect()
}

/// One row per workspace: its name, how much is on it, and what.
fn workspace_items(overview: &Overview) -> Vec<Item> {
    let mut keys = std::collections::HashSet::new();

    overview
        .workspaces
        .iter()
        .map(|workspace| {
            let windows: Vec<&Window> = overview
                .windows
                .iter()
                .filter(|window| window.workspace == workspace.name)
                .collect();

            let mut item = Item::new(
                format!("{WORKSPACE_ITEM}{}", workspace.name),
                format!("Workspace {}", workspace.name),
            )
            .subtitle(contents(&windows));
            if workspace.name == overview.target.workspace {
                item = item.detail("current");
            }

            // A one-character name is its own key, which for the usual 1–9
            // makes `cmd-3` go to workspace 3.
            let mut characters = workspace.name.chars();
            if let (Some(first), None) = (characters.next(), characters.next()) {
                item = item.glyph(first);
                if first.is_alphanumeric() && keys.insert(first.to_ascii_lowercase()) {
                    item = item.key(first);
                }
            } else {
                item = item.icon(Icon::builtin(icons::LAYOUT_GRID));
            }
            item
        })
        .collect()
}

/// "3 windows · Ghostty, Google Chrome", naming each application once.
fn contents(windows: &[&Window]) -> String {
    if windows.is_empty() {
        return "Empty".to_string();
    }

    let mut apps: Vec<&str> = Vec::new();
    for window in windows {
        if !apps.contains(&window.app_name.as_str()) {
            apps.push(&window.app_name);
        }
    }
    let noun = if windows.len() == 1 {
        "window"
    } else {
        "windows"
    };
    format!("{} {noun} · {}", windows.len(), apps.join(", "))
}

/// Which of `windows` to move: the one AeroSpace had focused if it belongs to
/// the application that was in front, or else that application's first.
fn target(front: &App, focused: Option<Window>, windows: &[Window]) -> Option<Window> {
    focused
        .filter(|window| window.app_pid == front.pid)
        // It may have been closed since the summon.
        .and_then(|focused| {
            windows
                .iter()
                .find(|window| window.window_id == focused.window_id)
        })
        .or_else(|| windows.iter().find(|window| window.app_pid == front.pid))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(window_id: u64, app_pid: i32, app_name: &str, workspace: &str) -> Window {
        Window {
            window_id,
            app_pid,
            app_name: app_name.into(),
            workspace: workspace.into(),
            window_layout: "h_tiles".into(),
        }
    }

    fn front(pid: i32) -> App {
        App {
            name: "Chrome".into(),
            pid,
        }
    }

    fn output(status: i32, stdout: &str, stderr: &str) -> Result<Output, String> {
        Ok(Output {
            status: Some(status),
            stdout: stdout.into(),
            stderr: stderr.into(),
        })
    }

    #[test]
    fn aerospace_json_is_read() {
        let windows: Vec<Window> = parse(output(
            0,
            r#"[{"app-name":"Ghostty","app-pid":61842,"window-id":2581,"workspace":"1"}]"#,
            "",
        ))
        .unwrap();
        assert_eq!(windows[0].window_id, 2581);
        assert_eq!(windows[0].workspace, "1");

        let workspaces: Vec<Workspace> =
            parse(output(0, r#"[{"workspace":"0"},{"workspace":"web"}]"#, "")).unwrap();
        assert_eq!(workspaces[1].name, "web");
    }

    #[test]
    fn a_refusal_says_why() {
        let refused = parse::<Window>(output(2, "", "Can't connect to AeroSpace server\nmore"));
        assert_eq!(
            refused.unwrap_err(),
            "AeroSpace refused: Can't connect to AeroSpace server"
        );
        assert!(parse::<Window>(output(0, "not json", "")).is_err());
        assert!(parse::<Window>(Err("no such file".into())).is_err());
    }

    #[test]
    fn the_focused_window_is_moved_when_it_belongs_to_the_front_app() {
        let windows = vec![window(1, 7, "Chrome", "1"), window(2, 7, "Chrome", "3")];
        let picked = target(&front(7), Some(windows[1].clone()), &windows).unwrap();
        assert_eq!(picked.window_id, 2);
    }

    #[test]
    fn a_focused_centrepiece_falls_back_to_the_front_app() {
        let windows = vec![window(1, 5, "Zed", "1"), window(2, 7, "Chrome", "3")];
        let centrepiece = window(9, 99, "centrepiece", "1");
        let picked = target(&front(7), Some(centrepiece), &windows).unwrap();
        assert_eq!(picked.window_id, 2);

        assert!(target(&front(8), None, &windows).is_none());
    }

    #[test]
    fn rows_say_what_each_workspace_holds() {
        let overview = Overview {
            target: window(1, 7, "Chrome", "1"),
            workspaces: ["1", "2", "web"]
                .map(|name| Workspace { name: name.into() })
                .to_vec(),
            windows: vec![
                window(1, 7, "Chrome", "1"),
                window(2, 7, "Chrome", "1"),
                window(3, 5, "Zed", "1"),
                window(4, 6, "Slack", "web"),
            ],
        };

        let items = workspace_items(&overview);
        assert_eq!(items[0].title, "Workspace 1");
        assert_eq!(
            items[0].subtitle.as_deref(),
            Some("3 windows · Chrome, Zed")
        );
        assert_eq!(items[0].detail.as_deref(), Some("current"));
        assert_eq!(items[0].key.as_deref(), Some("1"));
        assert_eq!(items[1].subtitle.as_deref(), Some("Empty"));
        assert_eq!(items[2].subtitle.as_deref(), Some("1 window · Slack"));
        assert_eq!(items[2].key, None, "a long name has no single key");
        assert_eq!(items[2].id, "workspace:web");
    }

    #[test]
    fn the_layout_in_use_is_not_offered() {
        let items = layout_items("v_tiles");
        let ids: Vec<&str> = items.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(
            ids,
            ["layout:h_tiles", "layout:h_accordion", "layout:v_accordion"]
        );

        // A floating window is in none of them, so all four are on offer.
        assert_eq!(layout_items("floating").len(), LAYOUTS.len());

        let keys: std::collections::HashSet<_> = LAYOUTS.iter().map(|layout| layout.key).collect();
        assert_eq!(keys.len(), LAYOUTS.len(), "duplicate layout keys");
    }

    #[test]
    fn layouts_are_named_for_people() {
        assert_eq!(layout_name("h_accordion"), "Horizontal accordion");
        assert_eq!(layout_name("floating"), "Floating");
    }
}
