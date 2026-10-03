//! Bookmarks as a tree of folders, worked out from where each one sits.
//!
//! Every reader hands back a flat list in the order the browser shows it, each
//! bookmark carrying the names of the folders it is in. A folder is then a
//! path, and what is in it is every bookmark whose path starts there. A folder
//! with no bookmarks anywhere under it does not show up at all.

use crate::Bookmark;

/// A folder inside the one being shown.
#[derive(Debug, PartialEq, Eq)]
pub struct Folder {
    pub name: String,
    /// Every bookmark under it, however deep.
    pub count: usize,
}

/// What `path` holds directly: its folders, in the order the browser has
/// them, and its own bookmarks.
pub fn level<'a>(bookmarks: &'a [Bookmark], path: &[String]) -> (Vec<Folder>, Vec<&'a Bookmark>) {
    let mut folders: Vec<Folder> = Vec::new();
    let mut own = Vec::new();
    for bookmark in under(bookmarks, path) {
        match bookmark.folder.get(path.len()) {
            None => own.push(bookmark),
            Some(name) => match folders.iter_mut().find(|folder| &folder.name == name) {
                Some(folder) => folder.count += 1,
                None => folders.push(Folder {
                    name: name.clone(),
                    count: 1,
                }),
            },
        }
    }
    (folders, own)
}

/// Every bookmark in `path` or any folder inside it.
pub fn under<'a>(bookmarks: &'a [Bookmark], path: &[String]) -> Vec<&'a Bookmark> {
    bookmarks
        .iter()
        .filter(|bookmark| bookmark.folder.starts_with(path))
        .collect()
}

/// Where a profile's bookmarks open: inside its one top folder when that is
/// all there is — a bookmarks bar and nothing else — rather than a list of one.
pub fn start(bookmarks: &[Bookmark]) -> Vec<String> {
    match level(bookmarks, &[]) {
        (folders, own) if folders.len() == 1 && own.is_empty() => vec![folders[0].name.clone()],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bookmark(url: &str, folder: &[&str]) -> Bookmark {
        Bookmark {
            title: url.to_string(),
            url: url.to_string(),
            folder: folder.iter().map(|name| name.to_string()).collect(),
        }
    }

    fn path(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    fn sample() -> Vec<Bookmark> {
        vec![
            bookmark("a", &["Bar"]),
            bookmark("b", &["Bar", "Work"]),
            bookmark("c", &["Bar", "Work", "Deep"]),
            bookmark("d", &["Bar", "Play"]),
            bookmark("e", &["Bar"]),
            bookmark("f", &["Other"]),
        ]
    }

    #[test]
    fn a_level_lists_its_folders_in_order_and_its_own_bookmarks() {
        let bookmarks = sample();
        let (folders, own) = level(&bookmarks, &path(&["Bar"]));
        assert_eq!(
            folders,
            [
                Folder {
                    name: "Work".into(),
                    count: 2
                },
                Folder {
                    name: "Play".into(),
                    count: 1
                },
            ]
        );
        let urls: Vec<&str> = own.iter().map(|b| b.url.as_str()).collect();
        assert_eq!(urls, ["a", "e"]);
    }

    #[test]
    fn nested_folders_open_one_level_at_a_time() {
        let bookmarks = sample();
        let (folders, own) = level(&bookmarks, &path(&["Bar", "Work"]));
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].name, "Deep");
        assert_eq!(own[0].url, "b");
        let all: Vec<&str> = under(&bookmarks, &path(&["Bar", "Work"]))
            .into_iter()
            .map(|b| b.url.as_str())
            .collect();
        assert_eq!(all, ["b", "c"]);
    }

    #[test]
    fn a_lone_top_folder_is_opened_straight_away() {
        assert_eq!(start(&sample()), Vec::<String>::new());
        let only_bar = vec![bookmark("a", &["Bar"]), bookmark("b", &["Bar", "Work"])];
        assert_eq!(start(&only_bar), path(&["Bar"]));
    }
}
