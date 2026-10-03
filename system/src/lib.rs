//! System actions and Settings panes, under `sys` and as root shortcuts.

use centrepiece_extension::{
    Extension, Icon, Item, Response, Screen, SystemAction, export_extension, host,
};

struct System;

#[derive(Clone, Copy)]
enum Target {
    Session(SystemAction),
    Settings(&'static str),
}

struct Action {
    id: &'static str,
    title: &'static str,
    subtitle: &'static str,
    /// An extension asset when suffixed `.svg`, otherwise a built-in icon name.
    icon: &'static str,
    target: Target,
}

const ACTIONS: &[Action] = &[
    Action {
        id: "lock",
        title: "Lock computer",
        subtitle: "Lock the screen, the same as ⌃⌘Q",
        icon: "shield-lock",
        target: Target::Session(SystemAction::Lock),
    },
    Action {
        id: "sleep",
        title: "Sleep",
        subtitle: "Put the Mac to sleep",
        icon: "bed",
        target: Target::Session(SystemAction::Sleep),
    },
    Action {
        id: "screen-saver",
        title: "Screen Saver",
        subtitle: "Start the screen saver",
        icon: "screen-saver",
        target: Target::Session(SystemAction::ScreenSaver),
    },
    Action {
        id: "sound",
        title: "Sound",
        subtitle: "Open System Settings / Sound",
        icon: "volume-2.svg",
        target: Target::Settings("x-apple.systempreferences:com.apple.Sound-Settings.extension"),
    },
    Action {
        id: "bluetooth",
        title: "Bluetooth",
        subtitle: "Open System Settings / Bluetooth",
        icon: "bluetooth.svg",
        target: Target::Settings("x-apple.systempreferences:com.apple.BluetoothSettings"),
    },
    Action {
        id: "wifi",
        title: "Wi-Fi",
        subtitle: "Open System Settings / Wi-Fi (WiFi)",
        icon: "wifi.svg",
        target: Target::Settings("x-apple.systempreferences:com.apple.wifi-settings-extension"),
    },
    Action {
        id: "displays",
        title: "Displays",
        subtitle: "Open System Settings / Displays",
        icon: "settings",
        target: Target::Settings("x-apple.systempreferences:com.apple.Displays-Settings.extension"),
    },
];

fn row(action: &Action) -> Item {
    Item::new(action.id, action.title)
        .subtitle(action.subtitle)
        .icon(if action.icon.ends_with(".svg") {
            Icon::asset(action.icon)
        } else {
            Icon::builtin(action.icon)
        })
}

impl Extension for System {
    fn new() -> Self {
        Self
    }

    fn started(&mut self) {
        host::set_shortcuts(&ACTIONS.iter().map(row).collect::<Vec<_>>());
    }

    fn activate(&mut self) -> Response {
        Response::Replace(
            Screen::search(ACTIONS.iter().map(row).collect())
                .placeholder("Search system actions and settings")
                .host_filtered(),
        )
    }

    fn suggest(&mut self, query: &str) -> Vec<Item> {
        // Root shortcuts match title words, so accept the unhyphenated name
        // too.
        if query.trim().eq_ignore_ascii_case("wifi") {
            ACTIONS
                .iter()
                .filter(|action| action.id == "wifi")
                .map(row)
                .collect()
        } else {
            Vec::new()
        }
    }

    fn select(&mut self, id: &str) -> Response {
        let Some(action) = ACTIONS.iter().find(|action| action.id == id) else {
            return Response::None;
        };
        match action.target {
            Target::Session(action) => match host::perform_system_action(action) {
                Ok(()) => Response::Dismiss,
                Err(error) => Response::Error(error),
            },
            Target::Settings(url) => {
                host::open_url(url);
                Response::Dismiss
            }
        }
    }
}

export_extension!(System);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_actions_are_listed_and_host_searchable() {
        let Response::Replace(screen) = System::new().activate() else {
            panic!("expected a search screen");
        };
        assert_eq!(screen.items.len(), 7);
        assert!(matches!(screen.filter, centrepiece_extension::Filter::Host));
        let ids: std::collections::HashSet<_> = screen.items.iter().map(|item| &item.id).collect();
        assert_eq!(ids.len(), 7);
    }

    #[test]
    fn settings_rows_use_their_lucide_assets() {
        for (id, file) in [
            ("sound", "volume-2.svg"),
            ("bluetooth", "bluetooth.svg"),
            ("wifi", "wifi.svg"),
        ] {
            let action = ACTIONS.iter().find(|action| action.id == id).unwrap();
            assert!(matches!(row(action).icon, Icon::Asset(name) if name == file));
            assert!(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("assets")
                    .join(file)
                    .is_file()
            );
        }
        assert!(matches!(
            System::new().suggest("wifi")[0].icon,
            Icon::Asset(_)
        ));
    }

    #[test]
    fn unknown_actions_are_ignored_and_wifi_has_a_root_alias() {
        assert!(matches!(System::new().select("unknown"), Response::None));
        assert_eq!(System::new().suggest("WiFi")[0].id, "wifi");
        assert!(System::new().suggest("safari").is_empty());
    }
}
