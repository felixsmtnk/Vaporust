# Vaporust roadmap

**English** · [Русский](ROADMAP.ru.md)

Where the project is heading. The order is approximate: priorities change with feedback. What has
already shipped - in [CHANGELOG.md](CHANGELOG.md).

Icons: 📋 planned · ⏸ postponed · ⛔ not possible for now · 💭 idea for the future.

Much here relies on [Aurelia](https://github.com/Drackrath/Aurelia), which Vaporust is built on.

**Contents**
- [Plans](#plans): [measurements](#-next-measurements-on-a-weak-laptop), [new design](#-then-new-design)
- [Postponed](#-postponed), [Not possible for now](#-not-possible-for-now), [Ideas for the future](#-ideas-for-the-future)

---

# Plans

## 📋 Next: measurements on a weak laptop

Light mode and the English interface are ready (see the CHANGELOG). What is left is to prove the
benefit with numbers.

- A "regular Steam vs Vaporust" table on a ThinkPad E330: memory of all Vaporust and Steam
  processes, time until the game starts, CPU load; with the counter and the PC named.
- The light mode goal is 25-35 MB of memory on a laptop with Intel graphics.

## 📋 Then: new design

A redesign of the interface: buttons and sections in the same places as in the Steam client, but
compact. We start with concrete problems that are already visible:

- Readability at 1366x768 and 125-150% scale: cut-off names in the left game list, small pale
  secondary text.
- Game page: ten links in one row with the same weight - keep the main ones (store, achievements,
  community, Workshop), move the rest (folder, shortcut, note, support, trailer) to the manage menu.
- Long names of games, friends and groups in all lists.
- Clear grouping of actions: one main and noticeable action, service actions in a menu.
- Pages do not "jump" while images load: space for the cover and screenshots is reserved in
  advance, placeholders are shown.
- Common text sizes: captions and explanations (reviews, dates, award names, Points Shop) are
  readable at 100% without zooming.
- Common interface parts with rules for a narrow window and 100-200% scale: a search and filter
  row, a settings row, a table, a page header, an empty state and an error, a dialog (Esc -
  cancel). Inventory in a narrow window: "grid <-> item" instead of two columns.
- Feature readiness in this roadmap - by levels: "in the interface", "tested by hand", "failures
  tested", "tested on a second PC".
- Along the way - order in the code: modules in folders (interface, network, Steam files,
  Windows), screen state in separate structures, parsing Steam pages with an HTML parser where
  possible instead of substring search.

## 📋 Later: remaining features

**Library and games**
- Steam Cloud on/off per game.
- The full list of download regions, as in Steam (Steam keeps it only inside its client).

**Store and market**
- Buying from the Community Market, a new buy order, offering a trade to a friend (on the Steam website for now).
- Following curators; your own game tags.

**Profile and community**
- Choosing the favorite badge; likes and comments in the activity feed; Steam Family restrictions.

**Program**
- A light theme.

## 📋 Last

Hard things: a week or more each.

- Remote install of a game on another of your PCs.
- Uploading your screenshots to the Steam cloud.
- A gamepad mode (like Big Picture).
- Trailers and videos right in the program.
- Sale pages with their storefronts.
- Broadcasts (watching friends' streams).
- Setting up profile showcases.
- Writing guides.
- Gameplay recording. Possible without injecting into the game (window capture by Windows), but
  it is a lot of work and not a priority.

---

## ⏸ Postponed

- **A friends and chat window over the game** by a hotkey - a light "overlay" without injecting into the game.
- **Free giveaways** (notifications about free games) and an **"achievement hunter"**.
- **A portable version** (data next to the program, for a USB stick). For the login to work on
  any PC it would have to be stored unencrypted. We will come back when there is a safe solution.
- **Private betas with a password.** Steam does not show private branches until they are unlocked
  with a password; for now such betas are turned on in Steam itself.
- **Inviting to a game from the chat.** Impossible from a separate session: Steam takes the game
  from the session that sent the invitation. Invite from the game itself or the Steam overlay.

## ⛔ Not possible for now

We will come back if a way is found.

- **A real in-game overlay.** It requires injecting into the game process, and anti-cheats (VAC,
  EAC) may ban the account for that. Instead - a window over the game by a hotkey (see "Postponed").
- **Steam Input (gamepad layouts).** It works only inside Steam itself: games launched through
  Vaporust get it, but layouts have to be set up in Steam.
- **Remote Play, Remote Play Together, Steam Link.** Video streaming over a closed protocol.
- **Buying a gift and adding funds to the wallet.** Only on the Steam website: Vaporust does not
  enter payment details.
- **Changing the password and email, Steam Guard, the mobile authenticator.** Account security -
  only in the official Steam and app.

## 💭 Ideas for the future

- **Steam voice chat.** A closed protocol over WebRTC: a lot of work, and the program would get heavier.
- **Windows 7.** Steam itself has not worked on Windows 7 since January 1, 2024, so only launching
  without Steam is left there; plus a special Rust build is needed.
- **New Aurelia versions.** Follow them and bring them in (0.2.5 inside now).

---

Have an idea or found a bug? Open an [issue](../../issues).
