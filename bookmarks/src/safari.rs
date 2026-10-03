//! Safari, whose profiles all share one set of bookmarks, kept in
//! `~/Library/Safari/Bookmarks.plist`. macOS guards that folder: Centrepiece
//! reads it only once it has Full Disk Access.

use std::path::Path;

use plist::{Dictionary, Value};

use crate::{Bookmark, Profile};

const BOOKMARKS: &str = "Bookmarks.plist";

/// The one profile there is, as far as bookmarks go.
pub fn profile(data: &Path) -> Profile {
    Profile {
        name: "Safari".to_string(),
        subtitle: None,
        dir: data.to_path_buf(),
        launch: Vec::new(),
        avatar: None,
    }
}

/// Why Safari's bookmarks could not be read.
pub enum Unreadable {
    /// macOS keeps Centrepiece out until it has Full Disk Access.
    NoAccess,
    Broken(String),
}

pub fn bookmarks(dir: &Path) -> Result<Vec<Bookmark>, Unreadable> {
    let value = Value::from_file(dir.join(BOOKMARKS)).map_err(|error| {
        if error.as_io().is_some() {
            Unreadable::NoAccess
        } else {
            Unreadable::Broken(format!("Could not parse Safari's bookmarks: {error}"))
        }
    })?;
    let mut found = Vec::new();
    if let Some(root) = value.as_dictionary() {
        flatten(root, &[], &mut found);
    }
    Ok(found)
}

fn string<'a>(node: &'a Dictionary, key: &str) -> Option<&'a str> {
    node.get(key).and_then(Value::as_string)
}

fn flatten(node: &Dictionary, path: &[String], found: &mut Vec<Bookmark>) {
    match string(node, "WebBookmarkType") {
        Some("WebBookmarkTypeLeaf") => {
            let Some(url) = string(node, "URLString") else {
                return;
            };
            let title = node
                .get("URIDictionary")
                .and_then(Value::as_dictionary)
                .and_then(|uri| string(uri, "title"))
                .filter(|title| !title.is_empty())
                .unwrap_or(url);
            found.push(Bookmark {
                title: title.to_string(),
                url: url.to_string(),
                folder: path.to_vec(),
            });
        }
        Some("WebBookmarkTypeList") => {
            let name = match string(node, "Title").unwrap_or_default() {
                "BookmarksBar" => "Favorites",
                "BookmarksMenu" => "Bookmarks Menu",
                "com.apple.ReadingList" => "Reading List",
                other => other,
            };
            let mut path = path.to_vec();
            if !name.is_empty() {
                path.push(name.to_string());
            }
            for child in node
                .get("Children")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_dictionary)
            {
                flatten(child, &path, found);
            }
        }
        // History, and anything newer than this.
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>WebBookmarkType</key><string>WebBookmarkTypeList</string>
  <key>Title</key><string></string>
  <key>Children</key><array>
    <dict>
      <key>WebBookmarkType</key><string>WebBookmarkTypeProxy</string>
      <key>Title</key><string>History</string>
    </dict>
    <dict>
      <key>WebBookmarkType</key><string>WebBookmarkTypeList</string>
      <key>Title</key><string>BookmarksBar</string>
      <key>Children</key><array>
        <dict>
          <key>WebBookmarkType</key><string>WebBookmarkTypeLeaf</string>
          <key>URLString</key><string>https://rust-lang.org/</string>
          <key>URIDictionary</key><dict><key>title</key><string>Rust</string></dict>
        </dict>
        <dict>
          <key>WebBookmarkType</key><string>WebBookmarkTypeList</string>
          <key>Title</key><string>Work</string>
          <key>Children</key><array>
            <dict>
              <key>WebBookmarkType</key><string>WebBookmarkTypeLeaf</string>
              <key>URLString</key><string>https://github.com/</string>
            </dict>
          </array>
        </dict>
      </array>
    </dict>
  </array>
</dict></plist>"#;

    #[test]
    fn bookmarks_carry_their_folder_path() {
        let value = Value::from_reader_xml(SAMPLE.as_bytes()).unwrap();
        let mut found = Vec::new();
        flatten(value.as_dictionary().unwrap(), &[], &mut found);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].title, "Rust");
        assert_eq!(found[0].folder, ["Favorites"]);
        assert_eq!(found[1].title, "https://github.com/");
        assert_eq!(found[1].folder, ["Favorites", "Work"]);
    }
}
