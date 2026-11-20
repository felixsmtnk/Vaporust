# Vaporust changelog

**English** · [Русский](CHANGELOG.ru.md)

What went into each released version, newest first. Plans for the future are in
[ROADMAP.md](ROADMAP.md), archives and full release notes are on the [Releases](../../releases) page.

## ✅ 0.9.2-alpha: Aurelia 0.2.6 and fixes from a code review

*Released 2026-11-20.*

**Aurelia 0.2.6**
- The bundled Aurelia is updated to 0.2.6: games whose depots come from another app install
  correctly, an install that got no base game files is reported as failed, safer storage of the
  login and of Steam Guard tokens, Steam Cloud downloads are size-capped, and the Steam libraries
  move off two dependencies with security advisories (protobuf, rsa).

**Fixes from a code review**
- Selling a game item: with several copies of one skin, the copy you clicked is the one sold (before,
  the first copy of that skin was always chosen).
- "Close game" for a non-Steam game closes only that game and the programs it started, not every
  program from its folder (dangerous for games on the Desktop or in a shared folder).
- Steam Cloud: a damaged download no longer overwrites a local save (the next sync could upload it over
  the cloud copy); big save sets are listed in full; "Make a copy" and "Restore" find saves where the
  sync looks for them (root overrides, SteamID folders, Documents moved to OneDrive or another disk).
- Changing a game language no longer deletes base game files when two depots write the same file with
  different letter case; file names from Steam that would leave the game folder are ignored.
- The saved login: it is encrypted before the very first save; a passing keyring error no longer
  replaces the encryption key (the login stayed unreadable); a damaged session file no longer crashes.
- Updates of Vaporust: the version on trial is no longer confirmed by the old process closing, and
  "Go back to the previous version" stays available until the next start.
- Unfinished downloads in the temporary folder (Steam was open) are visible again: they can be
  continued or deleted; a failed move keeps the record; "Restart Steam to play" appears again.
- After a logout, logging in again (even into the same account) loads the profile, wallet, badges,
  family and the rest again; the download of the old login stops; the friends watcher of the old
  login closes.
- Back / forward no longer show empty pages; LIBRARY leaves the collections page; Community keeps the
  last game for its drop-down.
- Table view: a double click plays or installs (the first click opened the game page at once).
- Smart collections with a genre rule fill after a restart; their name is shown as the list title, in
  the grid too; deleting one no longer resets another chosen collection.
- Endless loading: the store after a language change, a mod opened from a Workshop collection, a
  friend's achievements for another game; the cart count on the Points Shop and News pages; bundles
  in the cart get their name and picture.
- "Delete" while another game downloads now says to wait instead of doing nothing; results like
  "Shortcut created" or "force-closed" are shown while a game runs; the review window no longer shows
  the review loaded last time.
- Group chats from the friends page are not shown as friends; nicknames and "busy" there.
- Launch scripts that start with a quoted path work; non-Steam games with the same program and other
  arguments are no longer merged; desktop shortcuts of non-Steam games work without Steam games.
- Soundtracks: the next track starts in any section and in the tray; FLAC and other unplayable files
  are skipped; the volume works for WAV.
- Refund hint: names with ™ and ®, the latest purchase, dates of last December.
- Cloud screenshots: "Show more" no longer repeats the first page.
- The connection check no longer goes offline when only steamcommunity.com is blocked or a proxy is set.
- Security: picture links in messages are loaded only from the web (http/https), local network
  addresses are refused also on the first request.
- Smaller things: disabled switches look disabled, Steam texts of notifications, wallet history and
  mod pages follow the interface language, curator following is checked as an experimental function.

## ✅ 0.9.1-alpha: more of the Steam client

*Released 2026-11-18.*

**More of the Steam client**
- Properties of a game open in their own window, as in Steam, instead of taking the whole page.
- Smart collections: a name and rules (installed, played, owner, genre, update available); the games
  are picked by the rules every time, like Steam dynamic collections. Kept in Vaporust settings only,
  the Steam cloud collections are not touched.
- The library home has a "Play Next" shelf: owned games never launched, installed ones first.
- The store band (sections, Points Shop, News, Wishlist, Cart) is on the Cart, News and Points Shop
  pages too, so the store reads as one place.
- Cart as in Steam: game capsules, prices, "Remove", the estimated total and "Continue to payment"
  (payment stays on the Steam website).
- News as the Steam news hub: cards with the game capsule, the game, the date, the title and an excerpt.
- Points Shop as in Steam: categories on the left, items on dark tiles with the points price, the
  balance with the points coin on the right.
- Inventory as on the Steam site: games as tabs on top, item tiles with a border of the rarity color,
  a large item card on the right with the colored name and the item description.
- Friends page as in Steam: a menu on the left (friends, pending and sent invites, recently played
  with), friends grouped into in game, online and offline, names in the status colors.
- Badges as the rows of the Steam badges page: game capsule, hours, card drops left, collected cards.
- Steam Family: member cards with avatars and roles, family play time as rows with game capsules.
- Group chats in the look of the Steam chat: groups and channels on the left, names and times above
  the messages, the message field on a plate with the blue send button.
- Profile edit: sections in a menu on the left, as on the Steam site.
- Every page has the same Steam page title.
- Curators on a store page can be followed (experimental functions).

**Fixes**
- Section drop-downs no longer open when the mouse passes over the top bar after a click or while a
  menu is open; they open after the mouse rests on the section.
- The store bar showed "Cart (0)" while the cart had games: the cart is now loaded with the store.
- The STORE hint no longer covers its drop-down.
- The Steam folder is shown as Windows writes it (capital letters, backslashes).
- "Play Next" does not offer non-Steam games.

## ✅ 0.9.0-alpha: new design, as the Steam client

*Released 2026-11-17.*

**New design: the Steam client, compact**
- The whole window follows the Steam desktop client: the same palette (taken from the Steam
  client's own stylesheet), Segoe UI with a semibold font for titles, the same places for
  everything, but smaller paddings and buttons.
- Top of the window, as in Steam: a row of menus (Vaporust, View, Friends, Games, Help) with
  notifications, the account (avatar, name, wallet) and the window buttons on the right; below it
  back / forward arrows and STORE, LIBRARY, COMMUNITY, profile. The pages of a section open in a
  drop-down on hover, as in Steam, instead of a separate row.
- Library: the left column as in Steam ("Home" with the collections button, the list choice with
  "recent first" and "ready to play" buttons, the search with a filter button, "- ALL GAMES (n/m)",
  a dense list with 16 px icons). The home page has "What's New", "Recent Games" and "ALL GAMES"
  shelves. New collections page ("Your collections", create a collection).
- Game page as in Steam: the background with the game logo, the play bar (PLAY with an icon, last
  played, play time, achievements with a small bar, size, buttons for properties, store page and
  favorites), the band of links, then two columns: news and DLC on the left; friends who play,
  achievements, information, collections and notes on the right.
- Store: the blue Steam store bar with the sections, the search, Wishlist and Cart; the game page
  with large screenshots on the left and the game capsule on the right (description, reviews,
  dates, developer, tags) and a purchase box.
- Friends list colors and groups as in Steam (In-Game, Online Friends, Offline), the "FRIENDS" bar
  with the search.
- Settings and game properties as in Steam: a dark list of sections, rows with thin lines, switches
  instead of check boxes.
- Downloads: the graph with network, peak and free space is shown even when nothing downloads;
  "Up Next" and "Completed" titles as in Steam.
- Bottom bar: "Add a Game", "Manage Downloads", "Friends & Chat" without plates, as in Steam.
- The friends list can be a separate window, as in Steam (Settings -> Window); the chat window got
  Steam's strip of conversation tabs. Achievements page, the forum topic list and progress bars in
  the new style; the store bar moves its search to a second row in a narrow window.

**Code**
- main.rs (3300 lines) and net.rs (3800 lines) are split by meaning: launching, the download
  queue, the library rows, the friends panel, the library column, achievements; network files,
  chat, library and connection, smaller actions, live tests.

**Fixes**
- Without signing in, the library column showed no list choice; the game page news spun forever.
- A friend whose name was not loaded yet was shown by the account number in several places; now
  "loading...". The wallet next to the account name is loaded right after signing in.
- The friends activity feed showed page code ("$J( function() { InitializeCommentThread...") and
  "Rate up / View all comments" in event texts.

Also the rest of the Codex QA report on 0.8.5.

**Safety and reliability**
- Pictures, covers, screenshots and updates are downloaded in pieces and stop at a size limit,
  instead of being read whole into memory first. Slow servers time out; a redirect to this PC or
  the home network is refused.
- Restoring a save backup unpacks the files into a temporary folder on the disk instead of memory,
  and refuses a "zip bomb" (a tiny archive that unpacks into gigabytes).
- An update that fails to start twice in a row is rolled back by itself to the previous version,
  and that version is not offered again automatically. "Settings -> About" has "Go back to the
  previous version" while it is kept.
- Answers that arrive after a language switch (store pages, charts, achievements, download
  regions) are dropped instead of mixing the old language into the new one.
- steamwebhelper is frozen only when it lies in the Steam folder: a process of another program
  with the same name is never touched.

**Interface**
- An empty library says why: Steam not found, filters hide everything, the list is loading, no
  connection, or really no games; with buttons "Rescan", "Add a library folder", "Add a non-Steam
  game", "Reset filters".
- Explanations in the settings are larger and easier to read; warnings and errors have one look.
- The program icon is simplified for small sizes (taskbar, Explorer list).

**For weak PCs**
- "Games only" (Settings -> Performance): the store, community, profile and friends are hidden,
  only the library, launching, downloads and settings stay, and light mode turns on.
- "Together with Steam -> Measure" in Performance adds up the memory of Vaporust, steam.exe, the
  Steam browser and the overlay, as Task Manager counts it.
- After a game with freezing, Settings -> Launching games shows what the Steam browser took
  before freezing and while frozen on this PC.

**Tests and documents**
- New failure tests: a read-only file, an update rollback, an endless download, a redirect to a
  local address, a "zip bomb".
- README screenshots no longer show other people's names: the friends panel is hidden.
- TESTING.md says exactly what is checked and what is not measured yet; ROADMAP has the plan of
  measurements and launch checks on real games.

## ✅ 0.8.6-alpha: fixes from the QA report

*Released 2026-11-17.*

Fixes from the Codex QA report on 0.8.5.

**Files and saves**
- "Storage" no longer offers an installed game's folder as a leftover when its appmanifest is
  truncated (a game file without a whole record or a folder name stops the leftover search), and it
  checks the folder again right before deleting.
- Restoring a save backup never writes two files into one place and no longer matches a file by its
  name alone ("slotA\save.dat" does not land in "slotB\save.dat"); very large or strange archives
  are refused before anything is read.
- Files are written through a unique temporary file that is flushed to the disk.

**Chat and safety**
- A message with thousands of nested "/me", spoilers or quotes no longer crashes Vaporust.
- Addresses are checked by the standard URL parser: a backslash trick can no longer make another
  site look like Steam; addresses with a user name inside are not opened or loaded.
- The saved login stays encrypted: if the Windows key storage is briefly unavailable, it is not
  saved again rather than written as plain text.
- Huge animated stickers are not decoded; the frames kept in memory are limited.

**Accounts**
- Switching accounts is done step by step with a way back: a failed write changes nothing, a failed
  restart puts the old choice back; Steam is switched by the new window after its login.
- A pending Steam switch is retried after an error instead of being forgotten; a damaged
  loginusers.vdf is not edited, and when the registry refuses, the file goes back as it was.
- Signing into another account (for example "Sign in again" after a lost connection) drops
  everything of the previous one: purchase history, profile settings, store and inventory pages.
- A chat message of the previous account can no longer appear after the account changed.

**Interface and updates**
- The dark style always: with a light Windows theme some panels used to turn white with dark text.
- Reviews: after a language switch they load again (they used to spin forever), an old answer can
  not get into the new language, and changing the order or language is not undone by the next page.
  One "helpful" vote on a Russian review is counted.
- The update check accepts only a whole Windows program that is Vaporust; an error of an update
  started from the top bar is shown, not hidden.
- Unfreezing checks the process start time too, so a process that got the same number is not touched;
  only Steam's own processes with a known path are finished off.
- A shortcut of a portable copy no longer hands its game to the normal copy.

## ✅ 0.8.5-alpha: fixes from code reviews

*Released 2026-11-17.*

Fixes from two code reviews (Claude Fable and Gemini).

**Safety**
- Chat loads images by itself only from Steam sites (over https); an image from another site is a
  link with a "Show" button, so the sender does not learn your IP address. Network paths
  (`file://\\server\...`) are never loaded; very large files and "decompression bombs" are refused.
- A link whose text pretends to be another site's address shows the real address; a link to a
  non-Steam site asks first ("Do not enter your Steam login and password there").
- Restoring a save backup writes only to this game's save paths (an archive from the internet can
  no longer put a file anywhere); a backup made under another Windows user name still fits.
- Deleting leftovers warns when a folder does not fit into the Recycle Bin; leftovers are not
  searched if some game file of the library could not be read.
- Windows programs (cmd, Explorer, PowerShell, Notepad) are started by their full path; the exe from
  Steam's launch table cannot point outside the game folder; the log hides user folder names in any
  form and the account number of the Steam folder.

**Steam and files**
- Closing Vaporust (or a crash) during a game no longer leaves steamwebhelper frozen: the frozen
  processes are written to a file and unfrozen on exit and on the next start.
- New "Softly" freezing mode (the way NoSteamWebHelper-Reloaded does it): low priority and Windows
  efficiency mode instead of suspending, the Steam overlay keeps working.
- Steam is not killed while it is busy: updating itself at start (up to 15 minutes), syncing the
  cloud when closing. If a game does not appear within 120 s, the Steam window is shown instead of
  closing Steam.
- "Close forcefully" can no longer hit an unrelated process that got the number of a closed one;
  only processes from the Steam folder are finished off.
- Steam files with empty values ("BetaKey" "") and quotes inside are read correctly; the game file
  and the account list are written via a temporary file.
- A game launched without Steam can be released with "Stop tracking the game".
- A leftover in the temporary download folder no longer blocks continuing a download (it is moved
  aside).
- Backups and downloaded screenshots go to the real "Documents" and "Pictures" (also when moved to
  OneDrive or another disk).

**Fixes**
- Account switching finds the account in Steam by its login, not by the display name; switching
  from the "Sign in" menu closes the old window.
- "Quick sale" no longer lists an item above the cheapest listing in currencies like the hryvnia.
- Store reviews no longer re-request a failed page endlessly.
- Without a connection the top bar keeps "Library" and "Settings".
- Pausing a language change, a rollback or a check no longer turns it into a plain install.
- Chat: earlier messages stay after sending a sticker or an image; an echo of your own message is
  not added twice; a quote of a name with a space works; "Copy text" keeps spoilers and links; a
  closed tab does not come back as a draft.
- Group chats: the right default channel, images and stickers instead of plain text.
- Inventory: game items are shown one by one (different wear, trade lock), Steam items are stacked.
- Properties: "public" and an empty beta are the same branch; the language "Update now" button
  shows when Steam did not write the installed language; launch options and shortcuts go to the
  Steam account you are signed in with.
- Tray: a minimized window comes back on the screen; `--tray` with a maximized window stays in the
  tray.
- The window buttons in the corner are no longer taken by the resize edge.
- English interface: reviews, guides, tags and notifications in English; Steam texts load again
  after a language switch.

**Speed**
- The library list is built only when something changes (it was rebuilt several times per frame);
  chat messages are parsed once; the size of unfinished games is counted in the background; names
  of a Workshop collection come in one request per 100 mods.

## ✅ 0.8.4-alpha: one style

*Released 2026-11-16.* The library, settings and windows in the style of the new sign-in screen.

- Buttons, lists and fields across the program are dark plates with a soft hover, as on the sign-in screen.
- Larger top menu with a smooth hover and a growing underline; "Add a game", "Downloads" and
  "Friends and chat" share one animated button style with drawn icons.
- Library: "Home" with an icon and a blue marker, search with a magnifier. Settings: sections like
  the library column, groups on plates, quieter names. The key activation window is restyled.
- Without login: "Sign in" opens a menu of saved accounts (no password needed); key activation is
  shown only after sign-in.

## ✅ 0.8.3-alpha: new sign-in and accounts

*Released 2026-11-16.* A new sign-in screen and first-run wizard, reliable account switching.

- **Sign-in screen** in the middle of the window, in the program's style: account name, password and
  Steam Guard code on the left, the QR code on the right, a language switch; it fits low windows too.
- **First-run wizard** comes after sign-in (or "Continue without login"), redrawn as a card with
  choice cards and switches.
- **Account switching** also switches Steam: auto-login in Steam's `loginusers.vdf`, a restore point
  before each switch and "Restore the previous Steam login"; a new account or one switched while
  Steam was open is switched in Steam after sign-in.
- Fix: switching back to the main account no longer opens another account's session.
- The top bar without login, the no-login and offline bars and the update bar are restyled.

## ✅ 0.8.2-alpha: chat look and fixes

*Released 2026-11-16.* A small update after the second Codex review.

- Chat: two identical messages sent close together (for example two "yes" one second apart) are
  always shown as two; messages are matched by the number Steam gives them, not by text and time.
- Chat window in the new style: no extra close button in the search (the magnifier or Esc closes it),
  emoticon and picture buttons inside the input field, a send button, emoticon panel with tabs and tiles.
- Reviews: the chosen language is kept when the interface language changes; changing the language
  or order starts the list over, so languages no longer mix.
- The Vaporust update button really says "Update" now.

## ✅ 0.8.1-alpha: fixes after review

*Released 2026-11-16.* Fixes after the Codex review of 0.8.0-alpha.

- Switching the interface language during a download no longer breaks the download schedule or the
  "download complete" handling (the kind of operation is no longer recognised by its text).
- A new chat message identical to an older one (for example "yes") no longer disappears when the
  history loads at the same moment.
- Reviews open in the interface language by default; friends playing now are grouped under
  "IN GAME"; the Vaporust update button says "Update".

## ✅ 0.8.0-alpha: in English, lighter

*Released 2026-11-16.* The whole interface in English, light mode for weak PCs, a weekly check of Steam pages.

- **English interface.** The whole interface is available in English and Russian ("Settings ->
  Window -> Interface language", by default it follows the Windows language; also on the first
  step of the first-run wizard). Steam texts (descriptions, news, achievements) come in the chosen language.
- **Light mode** ("Settings -> Performance"): the window repaints less often, friend statuses
  refresh every 15 s, the Steam connection is checked once a minute, Steam notifications every
  30 minutes; the "What's new" shelf and friends activity on the game page are not loaded.
  Turning it on also turns animations off and memory saving on.
- **Weekly check of Steam pages** on GitHub: fresh public pages are downloaded and parsed, and if
  Valve changed the markup, an issue is opened automatically.
- The GitHub page, security notes, roadmap and this changelog are in English, with Russian copies
  (`*.ru.md`).

## ✅ 0.7.1-alpha: code for everyone

*Released 2026-11-16.* Fixes after the Codex and Gemini reviews, code comments in English.

- Code comments translated to English (for other developers).
- The "Restore Steam as it was" button is renamed to "Remove Vaporust changes in Steam": it removes
  service changes, while the language, beta and launch options of games stay.
- The tray icon is a white silhouette without a background, like other programs (dark on a light taskbar).
- The steamwebhelper freeze checkbox in the first-run wizard now really turns the freeze on and off.
- Two identical messages in a row in the chat (for example two "yes") are no longer merged into one.
- The "Remove from sale" and "Cancel buy order" buttons are shown greyed out with a hint without
  experimental features, instead of disappearing.

## ✅ 0.7.0-alpha: no password needed, safer

*Released 2026-11-16.* After an external review by Fable: trust, safety, a way back.

- **Without login mode:** installed games only, launched through Steam, no password needed. The
  choice is in the first-run wizard and as a "Without login" button on the login screen.
- **Experimental features** (Settings -> Account), off by default: selling and trades, version
  pinning and rollback, comments, ratings, awards, groups, blocking, profile editing.
- **"Restore Steam as it was"** (Settings -> About): unfinished games become visible to Steam
  again, pinned versions are unpinned, the offline mode is removed, temporary download folders go
  to the Recycle Bin.
- On Intel graphics the normal Windows frame is the default (against the black bar on laptops).
- A new logo: a V made of a connecting rod with a piston and a lever on a copper gear. The window
  icon is square, the tray one is round.
- Steam groups no longer appear in the friend list (they were shown as long numbers).
- The library grid opens at the top at startup (it used to open from the second or third row).
- A library cache per account; the old playtime history is moved only once; the chat history does
  not lose new messages; login keys are cut out of the log; family games without "Launch without
  Steam"; DLC without a name are not shown as numbers.
- GitHub build: actions pinned to exact versions, read-only permissions, provenance attestation
  for archives. New SECURITY.md and the "Password and security" README section, TESTING.md, issue templates.

## ✅ 0.6.5-alpha: responsiveness and order

*Released 2026-11-15.* Fixes after external code reviews (Codex, Gemini).

- QR login no longer holds up everything else: you can change your mind and log in with a
  password, or press "Cancel QR login".
- Chat history, achievements and library refresh load in the background and do not delay other actions.
- Heavy data (games, conversations, achievements) is not copied entirely on every event.
- Market buy orders are parsed as JSON, the currency is checked strictly.
- The language of Steam texts is gathered in one place (preparation for translation).
- Grid: no black bar at the bottom, the button does not disappear at the edge; the game list on
  the left has no empty space at the bottom.
- The old shared playtime history is moved only with a button in "Statistics"; the sale message is
  tied to the item itself; price calculation with a zero game fee.
- README: honest words about freezing steamwebhelper instead of `-no-browser`, a comparison with
  Steam compact mode and Playnite; the version history moved to CHANGELOG.md; a new social preview image.

## ✅ 0.6.4-alpha: selling and the library grid

*Released 2026-11-15.* Fixes after the author's and an external reviewer's check of 0.6.3-alpha.

- **Quick and instant sale for all games.** Items without buy orders (common in CS2) are no
  longer treated as "not selling"; quick sale is always available, even if the cheapest listing is
  already at the minimum price.
- **The quick sale step is configurable:** "Settings - Account - Quick sale" (by default one price
  step of the currency).
- The game's own market fee, if the game has one (as on the Steam website).
- The "confirm the sale" message is shown for 90 seconds and only for the listed item.
- **Library grid as in Steam:** on hover the whole tile grows together with the button; not
  installed games get a blue "Install" button; one game is no longer highlighted in two places;
  no repeated "ALL GAMES (N)".
- **The right-click menu** in Steam's order: Play, Game page, links, Collections, Manage (folder,
  uninstall), Properties.
- Statistics: the old shared history goes to an account only if Vaporust was used with one
  account and its time is not bigger than the current one; the old file is removed only after the
  new one is written.
- The "answer from the current account" check is under the same lock as the write.
- Diagnostics also hide short Windows user names.

## ✅ 0.6.3-alpha: after a detailed review

*Released 2026-11-15.* Fixes after an external reviewer's QA report (16 findings).

- **Market prices by the rules of the currency**, as on the Steam website: minimum price, price
  step and fees come from the wallet. Without the rules selling is not offered.
- **Restoring saves**: a file that cannot be read (used by the game) stops the restore before
  anything is replaced - the rollback could delete it before.
- **Answers of the previous account** do not reach the new one, from subtasks too ("Game
  properties", friends).
- **Playtime statistics per account.**
- **Each page has its own scroll**: "Downloads", achievements and the game page open at the top.
- **A small window and a big scale**: the minimum window size grows with the scale, settings rows
  and library filters wrap, the store search bar takes two rows in a narrow window, the inventory
  search fits the width, table headers do not stick together.
- The sale window: honest text about confirming on the phone, Esc - cancel.
- "Storage": paths without the technical `\\?\` prefix.
- "Settings -> About -> Copy diagnostics" for bug reports.

## ✅ 0.6.2-alpha: data safety

*Released 2026-11-15.* Fixes after the second external code review.

- Restoring saves is "all or nothing": the archive is checked entirely first, and on a failure in
  the middle the already replaced files are put back.
- Two save backups in the same second no longer overwrite each other.
- Answers of background tasks of the previous account (after logging out or into another one) do
  not reach the new one.
- If the login could not be encrypted (Windows key storage unavailable), Vaporust warns at once.
- The library search hint without a wrong game count.

## ✅ 0.6.1-alpha: reliability and security

*Released 2026-11-14.* Fixes after an external code review, and automated tests.

- The saved Steam login is stored encrypted (the key is in the Windows Credential Manager); an
  existing plain login file is encrypted at the first start.
- The key for Steam requests is tied to the account and reset at logout.
- Restoring saves does not start if a backup of the current ones could not be made.
- The market sale price is checked strictly; a wrong price does not allow listing.
- The self-update verifies the archive checksum and the program bitness.
- Automated tests: parsing Steam pages on saved samples, and failures (no space, interrupted
  download, damaged files, broken archives).
- The window size and position are remembered (with a check that the window fits the screen).

## ✅ 0.6.0-alpha: closer to Steam

*Released 2026-11-14.* Almost everything that was missing compared to Steam: the market, cart,
profile editing, several accounts, chat tabs.

**Library and games**
- "Recent games" and "What's new" (news of your games) shelves on the library home.
- "Ready to play", "Update available" and "Genre" filters; sorting "by name" is plain alphabetical.
- Game page: developer, publisher, release date, genres, DLC (what is owned), "Support",
  "Trailer", a desktop "Shortcut", your own "Note".
- "PAUSE" and "RESUME" right on the game page; double-clicking a not installed game offers to install it.
- Achievements and game properties are pages of the main window; auto-update per game.
- Non-Steam games both ways: import from Steam and add to Steam.
- Soundtracks from the library (MP3, WAV, WMA play right in the program).

**Downloads**
- Downloading with Steam open: into a temporary folder, then moved into the library.
- Uninstalling an installed game with Steam open.
- A new library folder, clearing the Steam download cache, download region as a list of cities.

**Store**
- "Home" with shelves (special offers, top sellers, new releases, recommendations, wishlist
  discounts) and search.
- A cart in the program (payment on the Steam website).
- The wishlist in Steam's order, reordering with a right click; a notification when a game is released.
- On the game page: curators, all tags, "Not interested", your own review, Points awards for reviews.

**Inventory and Community Market**
- For an item: the best buy order, the cheapest listing, a 30-day price chart.
- Selling from the inventory: your own price, quick (cheaper than the cheapest listing) and
  instant (at the highest buy order); confirmation in the Steam mobile app.
- "Remove from sale", "Cancel buy order", a "Trades" tab (accept, decline, cancel).

**Friends and chat**
- Friends in collapsible groups, a mini profile on hover.
- Your own nickname for a friend, detailed in-game status, "Recent players", blocking.
- Chat: conversation tabs, replying to a message, reactions, spoilers, code, quotes, "/me".
- Creating a group chat.

**Profile and community**
- Editing your profile, avatar, privacy settings.
- A community group page (join, leave), a new discussion topic.
- Workshop collections, rating and favoriting mods; Steam Family invitations.

**Program**
- Several accounts and quick switching; a first-run wizard.
- Interface scale 75-200%; "My licenses", purchase and wallet history.
- Download progress on the tray icon.

**Known issues**
- An unfinished game started while Steam was closed sometimes does not continue while Steam is
  open - close Steam and press "CONTINUE".
- An open Steam does not see a game downloaded while it was running until it restarts (that is how
  Steam itself works).
- Long names in the "Steam Family" table are cut off.
- A black bar at the top of the window on a laptop with Intel graphics. Workaround: "Settings -
  Window - Window frame: Standard Windows".

## ✅ 0.5.1-alpha: fixes after testing

*Released 2026-11-14.* Fixes after testing 0.5.0-alpha on two PCs.

- Uninstalling a game from the right-click menu; unfinished games are removed completely.
- Games in a shared folder (Counter-Strike, Half-Life, Blue Shift, Opposing Force in
  `common\Half-Life`): only their own files are removed, neighbouring games are not affected.
- "STOP" on the page of a running game; child processes of the game are closed too.
- Non-Steam games with an icon from the exe; a key window with examples, as in Steam.
- A "Downloads" page as in Steam: a speed chart, "Up next", "Not finished", "Completed".
- Friend profiles with a short URL are complete again; showcases with numbers.
- Workshop as a grid, as in Steam; the Points Shop without reloading images.
- No separate title bar: window buttons in the section row, next to the bell.
- A lost internet connection is noticed without a restart.
- A new look of pop-up notifications.

## ✅ 0.5.0-alpha: like Steam

*Released 2026-11-13.* A look like the Steam client, many community features, much less memory
and Aurelia 0.2.4 inside.

- At the top: STORE, LIBRARY, COMMUNITY and the profile, with subsections under them.
- Library: the game list on the left with icons, search and filters, on the right "Home" (a cover
  grid or a table sortable by columns) or a game page with a big background and a green "PLAY".
- At the bottom: "+ Add a game", "Downloads" with progress, "Friends and chat".
- Steam-like colors, an own window frame; Chinese, Japanese, Korean names and emoji instead of squares.
- Non-Steam games; "Close game", game protection check, launch scripts, relinking a game to
  another library; storage and leftovers cleanup, save backups, CSV export, statistics, "What to play?".
- A downloads page and queue, resuming interrupted downloads, offline mode.
- Charts, news of your games in one feed, recommendations, key activation, Points Shop, badges and cards.
- A friends page, chat header with avatar, messages grouped, day separators, search (Ctrl+F),
  favorite friends and notes.
- A Steam-like profile, friends' screenshots, discussions and guides, activity feed, group chats,
  Workshop mod pages, Steam notifications, detailed achievements, Steam Family, renaming and
  deleting collections.
- Memory: about 55-60 MB at startup, about 80 MB after visiting all sections (was 140 MB); a
  "Performance" section; account details, autostart, an error log.

## ✅ 0.2.0-alpha: everything for Windows

*Released 2026-11-12.* Almost all of Aurelia's features brought to Windows.

- A cover grid or a compact list; Steam collections; a game page with a background, achievements and news.
- The Steam Family library; "Already downloaded..." and duplicate installs search; screenshots on
  the PC and in the Steam cloud; launching DRM-free games without Steam; version rollback and
  pinning; manual cloud saves; beta descriptions.
- Updating games and changing beta and language without starting Steam; a download schedule and
  a speed limit that applies at once.
- Reviews, system requirements and Steam events in the store; inventory with prices, wallet,
  market search, own listings and buy orders.
- Profiles, adding and removing friends, requests, a friend's wishlist; room effects in the chat;
  own notifications with different sounds; Steam Workshop.
- A "Settings" tab, self-update from GitHub Releases, proxy, a 32-bit build, working in the tray,
  low resource use (about 0.3% of one core when idle), automated builds on GitHub.

## ✅ 0.1.0-alpha.3: store

*Released 2026-11-12.* The Steam store inside the program: search, specials, wishlist, sorting
and filters; a game page with price, reviews summary, release date, players online, tags,
screenshots, description, news, similar games; a selectable price region. Buying happens on the
Steam page in the browser.

## ✅ 0.1.0-alpha.2: everyday convenience

*Released 2026-11-12.* Settings saved between runs, online status, a tray icon; new message
notifications (sound, taskbar flashing, unread count); library sorting; a download queue with
pause per game and for the whole queue.

## ✅ 0.1.0-alpha.1: first public alpha

*Released 2026-11-11.* The first version on GitHub: login (password + Steam Guard, QR code),
library, playtime, search; launching games through a hidden Steam and closing it after the game,
freezing steamwebhelper; game properties; installing games with progress; achievements; friends
and chat (emoticons, animated stickers, images, Ctrl+V).
