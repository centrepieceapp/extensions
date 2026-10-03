//! GitHub repositories, under the `gh` prefix.
//!
//! The first use asks for a personal access token and stores it in the
//! keychain. After that, typing filters the repositories the token can see;
//! picking one opens a submenu of actions on it. Under the repositories sits
//! **Pull requests** — the ones you opened and the ones waiting for your
//! review, across every repository — which `pr ` also steps into — and
//! **Refresh repository list**, for when a repository is missing, which
//! `refresh ` also runs.
//!
//! This used to be compiled into Centrepiece; it is now an extension, and the
//! proof that the extension API carries a real extension. Every request goes
//! out through Centrepiece, which only lets it reach `api.github.com` (see
//! `extension.toml`), and each answer comes back to
//! [`Extension::task_finished`].

use std::collections::HashSet;

use centrepiece_extension::{
    Extension, Icon, Item, Response, Screen, TaskId, TaskResult, Tasks, export_extension, host,
    http,
};
use serde::Deserialize;
use serde::de::DeserializeOwned;

/// The keychain entry the token lives in.
const SECRET_KEY: &str = "token";
const API: &str = "https://api.github.com";
/// GitHub rejects requests without one.
const USER_AGENT: &str = "centrepiece (https://github.com)";
/// Repositories are paged at 100; three pages is plenty for Centrepiece and
/// keeps the first search fast.
const MAX_REPO_PAGES: u32 = 3;
const REPO_PAGE_SIZE: usize = 100;
/// The keys offered on the pull request and issue lists.
const SHORTCUT_KEYS: &[char] = &[
    'a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l', 'q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p',
    'z', 'x', 'c', 'v', 'b', 'n', 'm',
];

/// What to ask for, shown under the token prompt.
///
/// The extension only ever issues GETs, so it needs read and nothing else. A
/// classic token cannot express that — `repo` is the only scope that reaches a
/// private repository and it grants write to everything as well — so point
/// people at a fine-grained token instead.
const TOKEN_HELP: &str = "Create a fine-grained token at \
     github.com/settings/personal-access-tokens with read-only access to \
     Metadata, Pull requests and Issues. It is stored in your macOS keychain.";

/// Item ids on the repository submenu.
const ACTION_VIEW: &str = "action:view";
const ACTION_PULLS: &str = "action:pulls";
const ACTION_ISSUES: &str = "action:issues";

/// The row under the repositories that lists your pull requests, and the
/// word that steps into it the way a prefix does: `gh pr `.
const ACTION_PULL_REQUESTS: &str = "action:pull-requests";
const PULL_REQUESTS_WORD: &str = "pr";
const PULL_REQUESTS_TITLE: &str = "Pull requests";

/// The row under that which asks GitHub for the repositories again, and its
/// word: `gh refresh `.
const ACTION_REFRESH: &str = "action:refresh";
const REFRESH_WORD: &str = "refresh";
const REFRESH_TITLE: &str = "Refresh repository list";

/// The Octicons shipped in `assets/`.
mod icons {
    pub const BROWSER: &str = "browser.svg";
    pub const PULL_REQUEST: &str = "git-pull-request.svg";
    pub const PULL_REQUEST_DRAFT: &str = "git-pull-request-draft.svg";
    pub const BUG: &str = "bug.svg";
    pub const REPO: &str = "repo.svg";
    pub const REPO_PRIVATE: &str = "repo-locked.svg";
}

struct Github {
    token: Token,
    /// What each request in flight was for.
    tasks: Tasks<Pending>,
    repos: Vec<Repo>,
    /// Pages of a repository fetch that has not finished yet.
    incoming_repos: Vec<Repo>,
    repos_loading: bool,
    /// The user asked for the list again; it shows until the new one lands.
    refreshing: bool,
    /// The repository whose submenu is open.
    selected: Option<Repo>,
    /// The screens above the repository list, bottom first.
    views: Vec<View>,
    /// Your pull requests, kept between visits and refreshed on each.
    pulls: Vec<PullRequest>,
    /// The first of the two searches, while the second is in flight.
    incoming_pulls: Vec<PullRequest>,
    pulls_loading: bool,
    pulls_error: Option<String>,
    query: String,
    error: Option<String>,
}

/// What we know about the stored token.
enum Token {
    /// Not looked for yet, or being looked for now.
    Unknown,
    /// Looked for, and none came back: the user has to paste one. The
    /// keychain reports a dismissed password dialog the same way as an
    /// absent entry, so this is never trusted beyond the current visit.
    Missing,
    Present(String),
}

/// What a task in flight was started for.
enum Pending {
    /// Reading the token from the keychain.
    Token,
    /// Checking a pasted token with GitHub.
    Verify(String),
    /// Saving a checked token.
    Store,
    /// One page of your repositories.
    Repos(u32),
    /// Open pull requests or issues for a repository.
    Entries(Kind, String),
    /// Your pull requests in one role.
    PullRequests(Role),
}

/// A screen above the repository list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    /// A repository's submenu.
    Repo,
    /// A repository's pull requests or issues.
    Entries,
    /// Your pull requests.
    PullRequests,
}

/// Why a pull request is yours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Author,
    Reviewer,
}

impl Role {
    fn label(self) -> &'static str {
        match self {
            Role::Author => "Mine",
            Role::Reviewer => "Review requested",
        }
    }

    /// The search qualifier that finds them; `@me` is whoever the token is.
    fn qualifier(self) -> &'static str {
        match self {
            Role::Author => "author:@me",
            Role::Reviewer => "review-requested:@me",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Pulls,
    Issues,
}

impl Kind {
    fn noun(self) -> &'static str {
        match self {
            Kind::Pulls => "pull request",
            Kind::Issues => "issue",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Kind::Pulls => "Pull requests",
            Kind::Issues => "Issues",
        }
    }
}

impl Github {
    /// The screen to show given what we currently know.
    fn screen(&self) -> Screen {
        match &self.token {
            Token::Unknown => Screen::search(Vec::new())
                .placeholder("Search your GitHub repositories")
                .status("Unlocking your GitHub token…")
                .loading(true),
            Token::Missing => token_prompt(self.error.clone()),
            Token::Present(_) => self.repo_list(),
        }
    }

    fn repo_list(&self) -> Screen {
        let query = self.query.clone();
        // GitHub's own order — most recently pushed first — survives among the
        // repositories that were never picked.
        let matches = host::rank(
            &query,
            self.repos.clone(),
            |repo| repo.full_name.clone(),
            |repo| vec![repo.name.clone(), repo.full_name.clone()],
        );

        let mut items: Vec<Item> = matches
            .into_iter()
            .take(50)
            .map(|repo| {
                let icon = if repo.private {
                    icons::REPO_PRIVATE
                } else {
                    icons::REPO
                };
                let mut item = Item::new(repo.full_name.clone(), repo.full_name.clone())
                    .icon(Icon::asset(icon));
                if let Some(description) = repo.description.filter(|d| !d.trim().is_empty()) {
                    item = item.subtitle(description);
                }
                if let Some(language) = repo.language {
                    item = item.detail(language);
                }
                item
            })
            .collect();

        // Under every repository, where they are always in the same place.
        if row_answers(&query, PULL_REQUESTS_WORD, PULL_REQUESTS_TITLE) {
            items.push(
                Item::new(ACTION_PULL_REQUESTS, PULL_REQUESTS_TITLE)
                    .subtitle("Yours, and those waiting for your review")
                    .detail(PULL_REQUESTS_WORD)
                    .icon(Icon::asset(icons::PULL_REQUEST)),
            );
        }
        if row_answers(&query, REFRESH_WORD, REFRESH_TITLE) {
            items.push(
                Item::new(ACTION_REFRESH, REFRESH_TITLE)
                    .subtitle("Ask GitHub again for the repositories this token can see")
                    .detail(REFRESH_WORD)
                    .icon(Icon::builtin("refresh")),
            );
        }

        let status = match (&self.error, self.repos_loading, self.repos.is_empty()) {
            (Some(error), _, _) => error.clone(),
            (None, true, _) => "Loading your repositories…".to_string(),
            (None, false, true) => "No repositories visible to this token".to_string(),
            (None, false, false) => format!("No repository matches {query:?}"),
        };

        Screen::search(items)
            .placeholder("Search your GitHub repositories")
            .status(status)
            .loading(self.repos_loading && (self.refreshing || self.repos.is_empty()))
    }

    /// Asks GitHub for the repositories again, keeping the current list up
    /// until the new one arrives.
    fn refresh_repos(&mut self) -> Response {
        if matches!(self.token, Token::Present(_)) && !self.repos_loading {
            self.refreshing = true;
            self.load_repos();
        }
        self.query.clear();
        Response::Reset(self.screen())
    }

    /// The submenu that opens once a repository is chosen.
    fn repo_menu(repo: &Repo) -> Screen {
        Screen::menu(
            repo.full_name.clone(),
            vec![
                Item::new(ACTION_VIEW, "View on GitHub")
                    .subtitle(repo.html_url.clone())
                    .icon(Icon::asset(icons::BROWSER))
                    .key('w'),
                Item::new(ACTION_PULLS, "List pull requests")
                    .subtitle("Open pull requests on this repository")
                    .icon(Icon::asset(icons::PULL_REQUEST))
                    .key('p'),
                Item::new(ACTION_ISSUES, "List issues")
                    .subtitle("Open issues on this repository")
                    .icon(Icon::asset(icons::BUG))
                    .key('i'),
            ],
        )
    }

    /// Your pull requests, filtered by the query.
    fn pull_request_list(&self) -> Screen {
        let query = self.query.clone();
        let matches = host::rank(
            &query,
            self.pulls.clone(),
            |pull| pull.html_url.clone(),
            |pull| {
                vec![
                    pull.title.clone(),
                    pull.repo.clone(),
                    pull.author.clone(),
                    format!("#{}", pull.number),
                ]
            },
        );
        let items: Vec<Item> = matches
            .into_iter()
            .take(50)
            .map(pull_request_item)
            .collect();

        let status = match (&self.pulls_error, self.pulls_loading, self.pulls.is_empty()) {
            (Some(error), _, _) => error.clone(),
            (None, true, _) => "Loading your pull requests…".to_string(),
            (None, false, true) => {
                "No open pull requests of yours, or waiting for your review".to_string()
            }
            (None, false, false) => format!("No pull request matches {query:?}"),
        };

        Screen::search(items)
            .title(PULL_REQUESTS_TITLE)
            .placeholder("Search your pull requests")
            .status(status)
            .loading(self.pulls_loading && self.pulls.is_empty())
    }

    /// Steps into the pull request list with `query` typed, refreshing it.
    fn enter_pull_requests(&mut self, query: &str) -> Response {
        self.views.push(View::PullRequests);
        self.query = query.to_string();
        self.load_pull_requests();
        Response::enter(self.pull_request_list(), query)
    }

    /// Asks for the pull requests you opened, then — when those arrive — the
    /// ones waiting for your review.
    fn load_pull_requests(&mut self) {
        if self.pulls_loading {
            return;
        }
        if self.request_pull_requests(Role::Author) {
            self.pulls_loading = true;
            self.pulls_error = None;
            self.incoming_pulls.clear();
        }
    }

    fn request_pull_requests(&mut self, role: Role) -> bool {
        let Token::Present(token) = &self.token else {
            return false;
        };
        let task = get(&pull_requests_url(role), token);
        self.tasks.insert(task, Pending::PullRequests(role));
        true
    }

    fn load_repos(&mut self) {
        if self.repos_loading {
            return;
        }
        if self.request_repos(1) {
            self.repos_loading = true;
            self.error = None;
            self.incoming_repos.clear();
        }
    }

    fn request_repos(&mut self, page: u32) -> bool {
        let Token::Present(token) = &self.token else {
            return false;
        };
        let task = get(&repos_url(page), token);
        self.tasks.insert(task, Pending::Repos(page));
        true
    }

    fn load_entries(&mut self, kind: Kind) -> Response {
        let (Token::Present(token), Some(repo)) = (&self.token, &self.selected) else {
            return Response::None;
        };

        let full_name = repo.full_name.clone();
        let task = get(&entries_url(&full_name, kind), token);
        self.tasks
            .insert(task, Pending::Entries(kind, full_name.clone()));

        self.views.push(View::Entries);
        let loading = format!("Loading open {}s…", kind.noun());
        Response::Push(entry_screen(kind, &full_name, Vec::new(), Some(loading)).loading(true))
    }

    /// Reads the stored token, then loads repositories with it.
    fn unlock(&mut self) {
        self.token = Token::Unknown;
        // Asking again while the keychain dialog is still up would stack a
        // second one behind it.
        if self.tasks.any(|pending| matches!(pending, Pending::Token)) {
            return;
        }
        self.tasks
            .insert(host::read_secret(SECRET_KEY), Pending::Token);
    }

    /// The repository list, if it is the screen showing; results that land
    /// while the user is in a submenu wait for them to come back.
    fn repo_list_if_showing(&self) -> Response {
        if self.views.is_empty() {
            Response::Replace(self.screen())
        } else {
            Response::None
        }
    }

    fn token_read(&mut self, result: TaskResult) -> Response {
        let token = match result {
            TaskResult::Secret(Ok(Some(secret))) => String::from_utf8(secret).ok(),
            TaskResult::Secret(Err(err)) => {
                host::warn(format!(
                    "could not read the GitHub token from the keychain: {err}"
                ));
                None
            }
            _ => None,
        };
        match token {
            Some(token) => {
                self.token = Token::Present(token);
                self.load_repos();
            }
            None => self.token = Token::Missing,
        }
        self.repo_list_if_showing()
    }

    fn token_verified(&mut self, token: String, result: TaskResult) -> Response {
        match parse::<Account>(result) {
            Ok(account) => {
                host::info(format!("signed in to GitHub as {}", account.login));
                // Storing is best-effort: a token that cannot be saved still
                // works for this session.
                self.tasks.insert(
                    host::store_secret(SECRET_KEY, token.as_bytes()),
                    Pending::Store,
                );
                self.token = Token::Present(token);
                self.query.clear();
                self.load_repos();
            }
            Err(error) => {
                self.token = Token::Missing;
                self.error = Some(error);
            }
        }
        Response::Replace(self.screen())
    }

    fn repos_arrived(&mut self, page: u32, result: TaskResult) -> Response {
        match parse::<Vec<Repo>>(result) {
            Ok(batch) => {
                let complete = batch.len() < REPO_PAGE_SIZE || page >= MAX_REPO_PAGES;
                self.incoming_repos.extend(batch);
                if !complete && self.request_repos(page + 1) {
                    return Response::None;
                }
                self.repos = std::mem::take(&mut self.incoming_repos);
                self.error = None;
            }
            Err(error) => {
                self.incoming_repos.clear();
                self.error = Some(error);
            }
        }
        self.repos_loading = false;
        self.refreshing = false;
        self.repo_list_if_showing()
    }

    fn pull_requests_arrived(&mut self, role: Role, result: TaskResult) -> Response {
        match parse::<SearchPage>(result) {
            Ok(page) => {
                // A pull request cannot be both yours and waiting for your
                // review, but the same one is listed once all the same.
                let mut seen: HashSet<String> = self
                    .incoming_pulls
                    .iter()
                    .map(|pull| pull.html_url.clone())
                    .collect();
                for hit in page.items {
                    if seen.insert(hit.html_url.clone()) {
                        self.incoming_pulls.push(hit.into_pull_request(role));
                    }
                }
                if role == Role::Author && self.request_pull_requests(Role::Reviewer) {
                    return Response::None;
                }
                self.pulls = std::mem::take(&mut self.incoming_pulls);
            }
            Err(error) => {
                self.incoming_pulls.clear();
                self.pulls_error = Some(error);
            }
        }
        self.pulls_loading = false;
        if self.views.last() != Some(&View::PullRequests) {
            return Response::None;
        }
        Response::Replace(self.pull_request_list())
    }

    fn entries_arrived(&mut self, kind: Kind, repo: String, result: TaskResult) -> Response {
        // The user may have gone back, or into another repository, while this
        // was in flight.
        if self.views.last() != Some(&View::Entries)
            || self.selected.as_ref().map(|r| r.full_name.as_str()) != Some(repo.as_str())
        {
            return Response::None;
        }
        let result = parse::<Vec<Entry>>(result).map(|entries| {
            entries
                .into_iter()
                // GitHub's issues endpoint also returns pull requests; the
                // issue list should not.
                .filter(|entry| kind == Kind::Pulls || entry.pull_request.is_none())
                .collect::<Vec<_>>()
        });
        let (items, note) = match result {
            Ok(entries) if entries.is_empty() => {
                (Vec::new(), Some(format!("No open {}s", kind.noun())))
            }
            Ok(entries) => (entry_items(kind, &entries), None),
            Err(error) => (Vec::new(), Some(error)),
        };
        Response::Replace(entry_screen(kind, &repo, items, note))
    }
}

impl Extension for Github {
    fn new() -> Self {
        Self {
            token: Token::Unknown,
            tasks: Tasks::new(),
            repos: Vec::new(),
            incoming_repos: Vec::new(),
            repos_loading: false,
            refreshing: false,
            selected: None,
            views: Vec::new(),
            pulls: Vec::new(),
            incoming_pulls: Vec::new(),
            pulls_loading: false,
            pulls_error: None,
            query: String::new(),
            error: None,
        }
    }

    fn activate(&mut self) -> Response {
        self.query.clear();
        self.selected = None;
        self.views.clear();

        match self.token {
            // Look again even if the last visit found nothing: that may have
            // been the password dialog being dismissed, and this is the
            // user's next chance to answer it.
            Token::Unknown | Token::Missing => self.unlock(),
            Token::Present(_) if self.repos.is_empty() => self.load_repos(),
            Token::Present(_) => {}
        }

        Response::Replace(self.screen())
    }

    fn search(&mut self, query: &str) -> Response {
        // On the token prompt the text is the token, not a query.
        if matches!(self.token, Token::Missing) {
            self.query = query.to_string();
            return Response::None;
        }
        if self.views.last() == Some(&View::PullRequests) {
            self.query = query.to_string();
            return Response::Replace(self.pull_request_list());
        }
        // `pr ` at the repository list steps into the pull requests, the way
        // `gh ` at the root stepped in here; `refresh ` runs the refresh.
        if self.views.is_empty() && matches!(self.token, Token::Present(_)) {
            if let Some(rest) = word(query, PULL_REQUESTS_WORD) {
                return self.enter_pull_requests(rest);
            }
            if word(query, REFRESH_WORD).is_some() {
                return self.refresh_repos();
            }
        }
        self.query = query.to_string();
        Response::Replace(self.screen())
    }

    fn select(&mut self, item: &str) -> Response {
        match item {
            ACTION_VIEW => {
                let Some(repo) = &self.selected else {
                    return Response::None;
                };
                host::open_url(&repo.html_url);
                Response::Dismiss
            }
            ACTION_PULLS => self.load_entries(Kind::Pulls),
            ACTION_ISSUES => self.load_entries(Kind::Issues),
            ACTION_PULL_REQUESTS => self.enter_pull_requests(""),
            ACTION_REFRESH => self.refresh_repos(),
            // Pull request and issue rows carry their own URL.
            url if url.starts_with("https://") => {
                host::open_url(url);
                Response::Dismiss
            }
            full_name => {
                let Some(repo) = self
                    .repos
                    .iter()
                    .find(|repo| repo.full_name == full_name)
                    .cloned()
                else {
                    return Response::None;
                };
                host::record_pick(&repo.full_name);
                let screen = Self::repo_menu(&repo);
                self.selected = Some(repo);
                self.views.push(View::Repo);
                Response::Push(screen)
            }
        }
    }

    fn submit(&mut self, value: &str) -> Response {
        let token = value.trim().to_string();
        if token.is_empty() {
            return Response::Error("Paste a personal access token to continue".into());
        }

        self.error = None;
        let task = get(&format!("{API}/user"), &token);
        self.tasks.insert(task, Pending::Verify(token));

        Response::Replace(
            Screen::prompt("GitHub", "Checking the token with GitHub…", true).loading(true),
        )
    }

    fn task_finished(&mut self, task: TaskId, result: TaskResult) -> Response {
        let Some(pending) = self.tasks.take(task) else {
            return Response::None;
        };
        match pending {
            Pending::Token => self.token_read(result),
            Pending::Verify(token) => self.token_verified(token, result),
            Pending::Store => {
                if let TaskResult::Done(Err(err)) = result {
                    host::warn(format!("could not store the GitHub token: {err}"));
                }
                Response::None
            }
            Pending::Repos(page) => self.repos_arrived(page, result),
            Pending::PullRequests(role) => self.pull_requests_arrived(role, result),
            Pending::Entries(kind, repo) => self.entries_arrived(kind, repo, result),
        }
    }

    fn popped(&mut self) {
        if self.views.pop() == Some(View::Repo) {
            self.selected = None;
        }
    }

    fn dismissed(&mut self) {
        self.query.clear();
        self.selected = None;
        self.views.clear();
        self.error = None;
    }
}

export_extension!(Github);

/// Whether a row with its own `word` and `title` belongs under a repository
/// search for `query`: always when nothing is typed, and while what is typed
/// could still be either.
fn row_answers(query: &str, word: &str, title: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty() || word.starts_with(&query) || title.to_lowercase().starts_with(&query)
}

/// The rest of `query` if it starts with `word` and a space — the way `gh `
/// names an extension, followed by what to search it for.
fn word<'a>(query: &'a str, word: &str) -> Option<&'a str> {
    let (first, rest) = query.split_once(' ')?;
    first.eq_ignore_ascii_case(word).then(|| rest.trim_start())
}

/// One of your pull requests as a row: the repository, number and author
/// under the title, and why it is yours at the right.
fn pull_request_item(pull: PullRequest) -> Item {
    let icon = if pull.draft {
        icons::PULL_REQUEST_DRAFT
    } else {
        icons::PULL_REQUEST
    };
    Item::new(pull.html_url, pull.title)
        .subtitle(format!("{} #{} · {}", pull.repo, pull.number, pull.author))
        .detail(pull.role.label())
        .icon(Icon::asset(icon))
}

/// The screen that asks for a token, explaining what it needs to be able to do.
fn token_prompt(error: Option<String>) -> Screen {
    Screen::prompt("GitHub", "Paste a GitHub personal access token", true)
        .status(error.unwrap_or_else(|| TOKEN_HELP.to_string()))
}

/// A repository's pull requests or issues.
///
/// The pull request list leads with a row that opens the whole list on
/// GitHub, so it is what `↩` does before anything else is selected. `note` —
/// loading, nothing open, or what went wrong — goes under that row, since a
/// screen's status only shows when it has no rows at all; the issue list,
/// which has no such row, shows it as the status.
fn entry_screen(kind: Kind, repo: &str, mut items: Vec<Item>, note: Option<String>) -> Screen {
    let title = format!("{repo} · {}", kind.title());
    if kind == Kind::Pulls {
        let url = format!("https://github.com/{repo}/pulls");
        items.insert(
            0,
            Item::new(url.clone(), format!("View {repo} pull requests on GitHub"))
                .subtitle(note.unwrap_or(url))
                .icon(Icon::asset(icons::BROWSER)),
        );
        return Screen::menu(title, items);
    }
    let screen = Screen::menu(title, items);
    match note {
        Some(note) => screen.status(note),
        None => screen,
    }
}

/// Turns pull requests or issues into rows, each with the key that opens it.
fn entry_items(kind: Kind, entries: &[Entry]) -> Vec<Item> {
    entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            // `draft` only means anything on a pull request; the issues
            // endpoint leaves it false.
            let icon = match (kind, entry.draft) {
                (Kind::Issues, _) => icons::BUG,
                (Kind::Pulls, true) => icons::PULL_REQUEST_DRAFT,
                (Kind::Pulls, false) => icons::PULL_REQUEST,
            };
            let mut item = Item::new(entry.html_url.clone(), entry.title.clone())
                .subtitle(format!("#{} · {}", entry.number, entry.user.login))
                .icon(Icon::asset(icon));
            if let Some(key) = SHORTCUT_KEYS.get(index) {
                item = item.key(*key);
            }
            item
        })
        .collect()
}

// --- The GitHub API ---------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
struct Repo {
    name: String,
    full_name: String,
    html_url: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    private: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct Entry {
    number: u64,
    title: String,
    html_url: String,
    user: Account,
    #[serde(default)]
    draft: bool,
    /// Present on issues that are really pull requests, which is how GitHub
    /// signals that the issues endpoint returned one.
    #[serde(default)]
    pull_request: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct Account {
    login: String,
}

/// One of your pull requests, wherever it is.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PullRequest {
    number: u64,
    title: String,
    html_url: String,
    /// `owner/name`.
    repo: String,
    author: String,
    draft: bool,
    role: Role,
}

/// A page of search results.
#[derive(Debug, Deserialize)]
struct SearchPage {
    items: Vec<SearchHit>,
}

#[derive(Debug, Deserialize)]
struct SearchHit {
    number: u64,
    title: String,
    html_url: String,
    user: Account,
    #[serde(default)]
    draft: bool,
    /// The API URL of the repository, `…/repos/owner/name`; search results
    /// name the repository no other way.
    repository_url: String,
}

impl SearchHit {
    fn into_pull_request(self, role: Role) -> PullRequest {
        let repo = self
            .repository_url
            .split_once("/repos/")
            .map(|(_, repo)| repo.to_string())
            .unwrap_or(self.repository_url);
        PullRequest {
            number: self.number,
            title: self.title,
            html_url: self.html_url,
            repo,
            author: self.user.login,
            draft: self.draft,
            role,
        }
    }
}

/// Starts a GET against the API; the answer comes to `task_finished`.
fn get(url: &str, token: &str) -> TaskId {
    http::get(url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", USER_AGENT)
        .send()
}

/// Reads an API answer, or says what went wrong in words worth showing.
fn parse<T: DeserializeOwned>(result: TaskResult) -> Result<T, String> {
    match result {
        TaskResult::Http(Ok(response)) if response.is_success() => response
            .json()
            .map_err(|err| format!("GitHub sent a response we could not read: {err}")),
        TaskResult::Http(Ok(response)) => Err(describe(response.status)),
        TaskResult::Http(Err(err)) => Err(format!("Could not reach GitHub: {err}")),
        _ => Err("Centrepiece answered a request with something other than a response".into()),
    }
}

/// Turns an HTTP status into something worth showing a user.
fn describe(status: u16) -> String {
    match status {
        401 => "GitHub rejected the token. Check that it is valid and has not expired.".into(),
        403 => "GitHub refused the request — the token may be rate limited, or missing \
                read access to Metadata, Pull requests or Issues."
            .into(),
        404 => "Not found on GitHub.".into(),
        status => format!("GitHub returned HTTP {status}."),
    }
}

fn repos_url(page: u32) -> String {
    format!(
        "{API}/user/repos?per_page={REPO_PAGE_SIZE}&sort=updated&affiliation=owner,collaborator,organization_member&page={page}"
    )
}

/// The URL for a repository's open pull requests or issues, newest activity
/// first.
///
/// `direction` has to be explicit: on the pulls endpoint it only defaults to
/// `desc` when sorting by `created`, so `sort=updated` on its own would list
/// the *least* recently touched pull requests first.
fn entries_url(repo: &str, kind: Kind) -> String {
    let path = match kind {
        Kind::Pulls => "pulls",
        Kind::Issues => "issues",
    };
    format!("{API}/repos/{repo}/{path}?state=open&per_page=50&sort=updated&direction=desc")
}

/// The search for open pull requests you stand in `role` to, most recently
/// updated first.
fn pull_requests_url(role: Role) -> String {
    format!(
        "{API}/search/issues?q=is:pr+is:open+{}&sort=updated&order=desc&per_page=50",
        role.qualifier()
    )
}

#[cfg(test)]
mod tests {
    use centrepiece_extension::{Header, HttpResponse};

    use super::*;

    fn response(status: u16, body: &str) -> TaskResult {
        TaskResult::Http(Ok(HttpResponse {
            status,
            headers: vec![Header {
                name: "content-type".into(),
                value: "application/json".into(),
            }],
            body: body.as_bytes().to_vec(),
        }))
    }

    #[test]
    fn entry_lists_are_ordered_newest_activity_first() {
        for kind in [Kind::Pulls, Kind::Issues] {
            let url = entries_url("owner/repo", kind);
            assert!(url.contains("sort=updated"), "{url}");
            // Without this the pulls endpoint sorts ascending.
            assert!(url.contains("direction=desc"), "{url}");
        }
        assert!(entries_url("owner/repo", Kind::Pulls).contains("/repos/owner/repo/pulls"));
        assert!(entries_url("owner/repo", Kind::Issues).contains("/repos/owner/repo/issues"));
    }

    #[test]
    fn answers_are_read_or_explained() {
        let account: Account = parse(response(200, r#"{"login":"me"}"#)).unwrap();
        assert_eq!(account.login, "me");
        assert!(
            parse::<Account>(response(401, "{}"))
                .unwrap_err()
                .contains("rejected the token")
        );
        assert!(
            parse::<Account>(response(200, "not json"))
                .unwrap_err()
                .contains("could not read")
        );
        assert!(
            parse::<Account>(TaskResult::Http(Err("refused".into())))
                .unwrap_err()
                .contains("Could not reach GitHub: refused")
        );
    }

    #[test]
    fn pull_requests_are_filtered_out_of_issues() {
        let mut github = Github::new();
        github.views = vec![View::Repo, View::Entries];
        github.selected = Some(Repo {
            name: "r".into(),
            full_name: "o/r".into(),
            html_url: "https://github.com/o/r".into(),
            description: None,
            language: None,
            private: false,
        });
        let body = r#"[
            {"number":1,"title":"A bug","html_url":"https://x/1","user":{"login":"me"}},
            {"number":2,"title":"A PR","html_url":"https://x/2","user":{"login":"me"},
             "pull_request":{"url":"https://x"}}
        ]"#;

        let Response::Replace(screen) =
            github.entries_arrived(Kind::Issues, "o/r".into(), response(200, body))
        else {
            panic!("the issue list should be redrawn");
        };
        assert_eq!(screen.items.len(), 1);
        assert_eq!(screen.items[0].title, "A bug");

        // ...and a result for a repository the user has left is dropped.
        let late = github.entries_arrived(Kind::Issues, "o/other".into(), response(200, body));
        assert_eq!(late, Response::None);
    }

    #[test]
    fn the_pull_request_list_leads_with_the_page_on_github() {
        let mut github = Github::new();
        github.views = vec![View::Repo, View::Entries];
        github.selected = Some(Repo {
            name: "r".into(),
            full_name: "o/r".into(),
            html_url: "https://github.com/o/r".into(),
            description: None,
            language: None,
            private: false,
        });
        let body = r#"[{"number":7,"title":"Fix it","html_url":"https://github.com/o/r/pull/7",
                        "user":{"login":"me"}}]"#;

        let Response::Replace(screen) =
            github.entries_arrived(Kind::Pulls, "o/r".into(), response(200, body))
        else {
            panic!("the pull request list should be redrawn");
        };
        assert_eq!(screen.items.len(), 2);
        assert_eq!(screen.items[0].id, "https://github.com/o/r/pulls");
        assert_eq!(screen.items[0].title, "View o/r pull requests on GitHub");
        assert_eq!(screen.items[0].key, None);
        assert_eq!(screen.items[1].title, "Fix it");
        assert_eq!(screen.items[1].key.as_deref(), Some("a"));

        // With nothing open, the row stays and says so.
        let Response::Replace(empty) =
            github.entries_arrived(Kind::Pulls, "o/r".into(), response(200, "[]"))
        else {
            panic!("the pull request list should be redrawn");
        };
        assert_eq!(empty.items.len(), 1);
        assert_eq!(
            empty.items[0].subtitle.as_deref(),
            Some("No open pull requests")
        );
    }

    #[test]
    fn the_action_rows_sit_under_a_search_they_could_answer() {
        let pulls = |query| row_answers(query, PULL_REQUESTS_WORD, PULL_REQUESTS_TITLE);
        assert!(pulls(""));
        assert!(pulls("p"));
        assert!(pulls("PR"));
        assert!(pulls("pull re"));
        assert!(!pulls("centrepiece"));
        assert!(!pulls("requests"));

        let refresh = |query| row_answers(query, REFRESH_WORD, REFRESH_TITLE);
        assert!(refresh(""));
        assert!(refresh("ref"));
        assert!(refresh("refresh repo"));
        assert!(!refresh("repo"));
    }

    #[test]
    fn a_word_and_a_space_run_the_action() {
        assert_eq!(word("pr ", PULL_REQUESTS_WORD), Some(""));
        assert_eq!(word("PR  fix", PULL_REQUESTS_WORD), Some("fix"));
        assert_eq!(word("pr", PULL_REQUESTS_WORD), None);
        assert_eq!(word("pro ", PULL_REQUESTS_WORD), None);
        assert_eq!(word("a pr ", PULL_REQUESTS_WORD), None);
        assert_eq!(word("refresh ", REFRESH_WORD), Some(""));
        assert_eq!(word("refresh ", PULL_REQUESTS_WORD), None);
    }

    #[test]
    fn search_hits_name_their_repository() {
        let page: SearchPage = serde_json::from_str(
            r#"{"total_count":1,"items":[
                {"number":7,"title":"Fix it","html_url":"https://github.com/o/r/pull/7",
                 "user":{"login":"me"},"draft":true,
                 "repository_url":"https://api.github.com/repos/o/r"}
            ]}"#,
        )
        .unwrap();
        let pull = page
            .items
            .into_iter()
            .next()
            .unwrap()
            .into_pull_request(Role::Reviewer);
        assert_eq!(pull.repo, "o/r");
        assert!(pull.draft);

        let item = pull_request_item(pull);
        assert_eq!(item.id, "https://github.com/o/r/pull/7");
        assert_eq!(item.subtitle.as_deref(), Some("o/r #7 · me"));
        assert_eq!(item.detail.as_deref(), Some("Review requested"));
        assert_eq!(item.icon, Icon::asset(icons::PULL_REQUEST_DRAFT));
    }

    #[test]
    fn pull_request_searches_ask_for_open_ones_newest_first() {
        let url = pull_requests_url(Role::Author);
        assert!(
            url.contains("/search/issues?q=is:pr+is:open+author:@me"),
            "{url}"
        );
        assert!(url.contains("sort=updated&order=desc"), "{url}");
        assert!(pull_requests_url(Role::Reviewer).contains("review-requested:@me"));
    }

    #[test]
    fn every_entry_up_to_the_key_limit_gets_a_shortcut() {
        let entries: Vec<Entry> = (0..30)
            .map(|index| Entry {
                number: index,
                title: format!("Entry {index}"),
                html_url: format!("https://example.com/{index}"),
                user: Account { login: "me".into() },
                draft: false,
                pull_request: None,
            })
            .collect();

        let items = entry_items(Kind::Pulls, &entries);
        assert_eq!(items[0].key.as_deref(), Some("a"));
        assert_eq!(items[25].key.as_deref(), Some("m"));
        assert_eq!(items[26].key, None, "ran out of keys without complaining");

        let keys: HashSet<_> = items.iter().filter_map(|item| item.key.clone()).collect();
        assert_eq!(keys.len(), SHORTCUT_KEYS.len(), "duplicate shortcut keys");
    }
}
