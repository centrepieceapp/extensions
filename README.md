# Centrepiece extensions

The official extensions for Centrepiece, the keyboard-driven command bar for
macOS. Each is a WebAssembly component, built with the
[extension SDK](https://github.com/centrepieceapp/sdk), that runs in
Centrepiece's sandbox and reaches only what its manifest asks for.

| Prefix | Extension | What it does |
| --- | --- | --- |
| `bm` | Chrome | Search bookmarks across every Chrome profile, and open the URL on the clipboard in a profile of your choosing |
| `bb` | Browser Bookmarks | Pick an installed browser, then a profile, then one of its bookmarks |
| `gh` | GitHub | Search your repositories and pull requests, then act on one |
| `em` | Emoji | Copy an emoji; the smallest extension, and the template for new ones |
| `col` | Color | Preview a color and copy it as hex, rgb or hsl |
| `calc` | Calculator | Evaluate arithmetic and copy the answer; also answers unprefixed queries |
| `wm` | AeroSpace | Move the window you were in to another AeroSpace workspace, or switch its layout |
| `sys` | System | Lock, sleep, start the screen saver, or open Sound, Bluetooth, Wi-Fi and Displays settings |

## Installing

In Centrepiece, pick **Install extension** (or type `ext` and a space), then
**From official repo**: it lists every extension published here, and `↩`
downloads one, shows what it may use, and installs it on a second `↩`. It
works straight away, without a restart. **From URL** installs an archive from
anywhere else.

From a checkout of this repository, `scripts/extension.sh install <name>`
builds one and copies it into `~/.config/centrepiece/extensions/` (under
`$XDG_CONFIG_HOME` when that is set). Centrepiece loads it the next time it
starts.

## How they are published

[`.github/workflows/build.yml`](.github/workflows/build.yml) runs on every
push to `main`. It works out which extensions the push touched
([`scripts/affected.sh`](scripts/affected.sh)) — an extension's own folder
affects that extension; the workspace, `Cargo.lock`, the toolchain, the build
script or the workflow affect them all — and builds each into an archive of its
own, `<id>.tar.gz`: one folder with `extension.toml`, `extension.wasm` and
`assets/`. Each archive is uploaded to the release tagged `<id>-latest`,
replacing the last, and is also kept on the workflow run as an artifact. An
extension that has never been published is built whatever changed, so a new
one gets its first archive on the next push.

Then [`scripts/index.sh`](scripts/index.sh) writes `index.json` — every
published extension's id, name, description, prefix, API version, version
and archive URL — to the release tagged `index`. That file is what Centrepiece
reads for **From official repo**.

Pull requests build and test the extensions they touch, and publish nothing.
**Run workflow** on the Actions tab rebuilds everything when asked to.

## Building

Requires the toolchain in `rust-toolchain.toml`, which brings the
`wasm32-wasip2` target with it.

```sh
scripts/extension.sh build github     # -> target/extensions/github/
scripts/extension.sh build            # every extension
scripts/extension.sh package github   # -> dist/github.tar.gz, as the workflow publishes it
scripts/extension.sh install github   # built, then copied into ~/.config/centrepiece/extensions/
```

The SDK, `centrepiece-extension`, comes from
[centrepieceapp/sdk](https://github.com/centrepieceapp/sdk) at the release
tag `Cargo.toml` names, the same way an extension outside this repository
gets it. Moving to a new release is changing that tag; since it changes
`Cargo.lock`, the workflow then rebuilds every extension. To build against a
local checkout of the SDK instead — while changing the contract, say — point
`CENTREPIECE_SDK` at it:

```sh
CENTREPIECE_SDK=../sdk scripts/extension.sh build
```

Centrepiece's own sandbox tests run these extensions; they look for the builds
in `../extensions/target/extensions` beside it, so run `scripts/extension.sh
build` here before `cargo test` there.

## Writing one

The [SDK's README](https://github.com/centrepieceapp/sdk#readme) covers the
manifest, the sandbox and the API. The extensions here are worked examples:
[`emoji`](emoji/src/lib.rs) is the smallest, [`github`](github/src/lib.rs)
chains requests as tasks, [`chrome`](chrome/src/lib.rs) offers the copied URL,
and [`color`](color/src/lib.rs) answers at the root.

A new one is a folder with a `Cargo.toml` (a `cdylib` depending on
`centrepiece-extension.workspace = true`), an `extension.toml` and `src/`,
added to `members` in the workspace `Cargo.toml`; the folder's name is its id.
To try it while it builds, point Centrepiece at the build:

```sh
scripts/extension.sh build emoji
/path/to/Centrepiece.app/Contents/MacOS/centrepiece --replace --show --extension target/extensions/emoji
```

Centrepiece picks up a rebuilt component the next time you enter the
extension. The extensions' own tests run natively: `cargo test -p
centrepiece-emoji`, or `cargo test` for all of them.

## The extensions

With the Color extension installed, a query that is a color — `#f80`, `#ff8800cc`,
`rgb(255 136 0)`, `rgba(255, 136, 0, 0.5)`, `hsl(32, 100%, 50%)` — gets a row
of its own with a swatch of the color, and so does a color on the clipboard.
`↩` opens a menu that copies it as hex, as `rgb()` (`rgba()` when it has
alpha), or as `hsl()`.

With the Calculator extension installed, a query that is arithmetic — numbers joined by `+` `-` `*` `/` `^` (or `**`),
with parentheses and signs — gets a row with the answer: `2 * (3 + 4)` shows
`14`. `^` binds tightest and to the right, so `-2^2` is `-4`. `↩` copies the
answer. Anything that is not wholly arithmetic, such as `google-chrome`, is
searched as usual. Use `calc` to enter the calculator explicitly.

### System — `sys`

Install it from Centrepiece with **Install extension ▸ From official repo**,
or from here with `scripts/extension.sh install system`.

**Lock computer**, **Sleep** and **Screen Saver** request native session actions
through the host; none needs a macOS permission prompt. The extension explicitly
grants these operations with `system-actions` in its manifest.

**Sound**, **Bluetooth**, **Wi-Fi** and **Displays** open their respective panes
in System Settings. All seven items are searchable under `sys` and are shortcuts
at the root: type `lock`, `sleep`, `saver`, `sound`, `bluetooth`, `wi-fi` (or
`wifi`), or `displays` without entering the extension first.

### AeroSpace — `wm`

Drives [AeroSpace](https://github.com/nikitabobko/AeroSpace) through its
command line client. Install it from Centrepiece with **Install extension ▸
From official repo**, or from here with `scripts/extension.sh install aerospace`.
The extension looks for `aerospace` in `/opt/homebrew/bin`, then `/usr/local/bin` — the two
places its manifest lets it run it from — once at startup. Without it, `wm`
says so rather than listing anything.

**Move _application_ to workspace** acts on the window you were in when you
summoned Centrepiece, and opens a list of every workspace: its name, how many
windows it holds and which applications they belong to, with `current` beside
the one the window is on now. `↩` moves the window there and follows it; a
workspace with a one-character name is also picked by that character, so `wm`,
`↩`, `3` sends the window to workspace 3.

**Switch layout** shows the layout in use as its subtitle — `Workspace 1 ·
Horizontal tiles` — and offers the other three of AeroSpace's four: horizontal
tiles (`h`), vertical tiles (`v`), horizontal accordion (`a`) and vertical
accordion (`s`). The layout belongs to the container the window sits in, which
on a workspace without nested containers is the workspace itself. A floating
window is in none of the four, so it is offered them all.

### Chrome — `bm`

What used to be two built-in extensions, Chrome Bookmarks (`bm`) and Chrome URL
(`url`), as one extension. Install it from Centrepiece with **Install extension
▸ From official repo**, or from here with `scripts/extension.sh install chrome`.

**Bookmarks.** Type `bm` then a space, then keep typing to search titles, URLs
and folder names across every Chrome, Chrome Beta, Chrome Canary and Chromium
profile on the machine; the ones you open most float up. `↩` opens the page in
Chrome specifically, or in the default browser when Chrome will not start.

**The copied URL.** Copy a link, press the hotkey, and the first row is **Open
clipboard URL in Chrome profile**, with the link under it. `↩` lists every
profile Chrome knows about — its name and the account it is signed in to — and
each has a digit, so the whole trip is the hotkey, `↩`, `2`. The row is only
there when the clipboard holds exactly one `http` or `https` URL and nothing
else, and Chrome has profiles to open it in; it also leads the bookmark list
under `bm` while it is.

Bookmarks and profiles are read from Chrome's own files — a `Bookmarks` file
per profile and `Local State` beside them — fresh every 30 seconds at most.
Opening a page runs Chrome's own binary, with `--profile-directory` for a
profile, which is the one way to name a profile that works whether or not
Chrome is already running: `open` drops its arguments as soon as Chrome is up.
Chrome has to be in `/Applications` or `~/Applications`. The manifest lets the
extension read `~/Library/Application Support/Google` and `…/Chromium`, see the
clipboard as it was at the summon, and run those two copies of Chrome — nothing
else.

### Browser Bookmarks — `bb`

Install it from Centrepiece with **Install extension ▸ From official repo**,
or from here with `scripts/extension.sh install bookmarks`.

Type `bb` then a space for the browsers installed on this Mac — Safari, Chrome
(and Beta, Canary), Chromium, Brave, Edge, Vivaldi, Opera, Helium, Firefox, Zen
and LibreWolf, wherever their bundle is in `/Applications` or `~/Applications`.
`↩` on one lists its profiles, each with a digit — and, for Chromium
browsers, the account's picture, or the profile's colour without one; a browser with a single
profile skips straight to its bookmarks. They are shown folder by folder,
just as the browser files them: the folders first, each with how many
bookmarks are inside, then the folder's own bookmarks. `↩` on a folder opens
it — folders nest as deep as the browser's do — and `⌫` steps back out; a
profile with nothing but a bookmarks bar opens straight inside it. Typing
searches every bookmark in the open folder and the folders within it, by title,
URL or folder name, showing where each one sits. `↩` on a bookmark opens the
page in that browser and that profile — or in the default browser if it will
not start. A folder with no bookmarks anywhere inside it is not listed.

Chromium browsers keep a `Bookmarks` JSON file per profile, named in `Local
State`. The Firefox family keeps `places.sqlite` per profile, named in
`profiles.ini`; the extension reads it through `/usr/bin/sqlite3`, opened
immutable because the browser locks it while it runs, so a bookmark added in
the last moments may only appear once the browser has written it back. Safari
shares one `Bookmarks.plist` across its profiles, in a folder macOS only lets
Centrepiece read with **Full Disk Access** (System Settings › Privacy &
Security); without it, Safari's screen is one row, **Open Full Disk Access
settings**, which goes straight to that list. Arc keeps its sidebar
elsewhere, and is not listed.

The manifest names each browser's bundle (to tell whether it is installed),
its data folder, and its binary, plus `open` and `sqlite3` — nothing else.

### GitHub — `gh`

Install it from Centrepiece with **Install extension ▸ From official repo**,
or from here with `scripts/extension.sh install github`.

The first time you use it, Centrepiece asks for a personal access token and
stores it in the macOS keychain (as an internet password for
`centrepiece.local/plugin.github.token`, from when extensions were called
plugins); it is never written to disk in the clear
and never leaves the machine except to `api.github.com` — the only host its
manifest lets it reach. Later uses read it straight back out. A token stored
by the built-in extension that came before (`centrepiece.local/github`) is not picked
up; paste it again once.

Type to filter your repositories. `↩` on one opens a submenu — no longer a
search, so single keys act directly:

* `w` — View on GitHub (opens the repository in your default browser)
* `p` — List pull requests
* `i` — List issues

Both lists give every entry its own letter; pressing it opens that pull request
or issue on GitHub. The pull request list leads with **View owner/name pull
requests on GitHub**, so `↩` straight away opens the repository's whole list
in the browser. `backspace` steps back one screen at a time.

Under the last repository sits **Pull requests**: the open pull requests you
wrote and the ones waiting for your review, across every repository the token
can see, newest activity first. It is a search rather than a menu — type to
filter by title, repository, author or number, `↩` opens the one selected —
and it has a word of its own, so `gh pr ` steps straight into it and `gh pr
fix` arrives already searching for `fix`. The list is fetched afresh each time
it opens, with the previous one showing until it arrives.

Under that sits **Refresh repository list**, for a repository created or
shared since the list was loaded: it asks GitHub again and keeps the current
list up until the answer arrives. `gh refresh ` runs it without picking the
row.

#### What the token needs

Centrepiece only ever reads. Use a [fine-grained
token](https://github.com/settings/personal-access-tokens) with three
**read-only** repository permissions:

| Permission | Needed for |
| --- | --- |
| Metadata | `GET /user/repos` — the repository list |
| Pull requests | `GET /repos/{owner}/{repo}/pulls` — `p`; `GET /search/issues` — the **Pull requests** list |
| Issues | `GET /repos/{owner}/{repo}/issues` — `i` |

A binary reads back the token it stored without being asked. Any other binary
raises macOS's keychain consent dialog — twice in a row, because the keychain
checks an item's access list ("wants to use your confidential information") and
its partition list ("wants to access key") separately. Choose **Always Allow**
on both to be asked once. Rebuilding Centrepiece changes its ad-hoc code
signature, which makes it a different binary, so a development build asks again
after every build. Centrepiece stays up while it waits rather than dismissing
itself behind the dialogs, and takes the keyboard back when they are gone.

Nothing else. **Contents is not required** — Centrepiece never reads file
content. `GET /user`, which it calls once to check the token and find the login
to file it under, needs no permission at all. `w` makes no request; it opens the
URL already in the repository payload.

A **classic token** works too, but it cannot express least privilege: `repo` is
the only scope that reaches a private repository, and it grants full read *and
write* to code, collaborators and webhooks. Prefer fine-grained. Classic tokens
also need to be SSO-authorized for organizations that use SAML.

Two things to know about fine-grained tokens:

* They belong to **one resource owner**. A token owned by your personal account
  will not list organization repositories, and Centrepiece stores a single
  token — so personal and organization repositories cannot both be covered at
  once today.
* An organization can **require approval**. Until an owner approves it, the
  token reads public resources only, so private organization repositories will
  be missing from the list rather than erroring.


## Layout

```
aerospace/       AeroSpace workspaces and layouts, through its client
bookmarks/       bookmarks by browser and profile: Chromium, Firefox, Safari
chrome/          Chrome bookmarks and profiles
color/           color previews, copied as hex, rgb or hsl
calculator/      arithmetic previews and copying answers
system/          session actions and System Settings shortcuts
github/          GitHub repositories and pull requests
emoji/           the smallest extension
scripts/
  extension.sh   builds, packages and installs extensions
  extensions.sh  lists them
  affected.sh    the ones a change touches
  index.sh       the catalogue Centrepiece installs from
.github/workflows/build.yml   builds and publishes them
```
