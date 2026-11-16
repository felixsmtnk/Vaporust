# Vaporust changelog

**English** · [Русский](CHANGELOG.ru.md)

What went into each released version, newest first. Plans for the future are in
[ROADMAP.md](ROADMAP.md), archives and full release notes are on the [Releases](../../releases) page.

## 🔧 Coming in the next version

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
