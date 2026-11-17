# Vaporust changes to Aurelia 0.2.5

**English** · [Русский](VAPORUST_CHANGES.ru.md)

This directory is a copy of [Aurelia](https://github.com/Drackrath/Aurelia) version 0.2.5
(crate `aurelia-steam`, author Drackrath, license GPL-3.0-or-later, see `LICENSE`).
Vaporust uses it as a library and made the changes listed below (November 2026).
All other files are unchanged. Every change in the code is marked with a `vaporust` comment.

## Updates of the copy

- **0.2.3 -> 0.2.4:** the author's changes merged (`git merge` of tag v0.2.4), no conflicts with
  the Vaporust changes.
- **0.2.4 -> 0.2.5:** merged the same way (tag v0.2.5). The only conflict was `web/web_session.rs`:
  the author added the market eligibility check pass upstream, so the author's `get_text` was taken
  instead of ours; our `post_form` and `put_bytes` were moved to the upstream cookie storage
  (`cookie_header()`).

## Changed files

- `src/steam_client.rs` - connects the new modules (`vaporust_chat`, `vaporust_props`,
  `vaporust_media`, `vaporust_store`, `vaporust_groups`) and exports `CloudScreenshot`, `ChatGroup`,
  `ChatChannel`, `GroupMessage`, `RawChatEvent`, `RawChatKind`, `SentChatMessage`.
- `src/steam_client/chat.rs` - `recent_chat_messages` asks for the history with
  `bbcode_format = true`: without it stickers drop out of the history, and emoticons and images
  come simplified.
- `src/core/config.rs` - `save_session` fails closed: when encryption is required and the OS keyring is
  unavailable, the session is not written at all (the old encrypted file stays) instead of being saved
  as plain text.
- `src/steam_client/content.rs`:
  - `fetch_achievements` moved to `fetch_achievements_for(appid, language, steam_id)`: achievements
    of any user (comparison with a friend);
  - `CELL_OVERRIDE` - the download region chosen by the user (0 - as assigned by Steam), exported.
- `src/steam_client/friends.rs`:
  - `Friend.avatar_hash` (avatar hash in hex) from `CMsgClientPersonaState` - avatars in the friends list;
  - `Friend.nickname` (own nickname for a friend from `CMsgClientPlayerNicknameList`, parsed together
    with the friends list right after login, `apply_nicknames`) and `Friend.rich_status` (the "status"
    text from the game's rich presence); `PERSONA_REQUEST_FLAGS` gains flag 4096 (rich presence).
- `src/steam_client/ident_tests.rs` - the test `Friend` gets the `nickname` and `rich_status` fields
  (otherwise the Aurelia tests did not build after the friends changes).
- `src/steam_client/install.rs` - `set_download_limit_now` (the speed limit applies at once instead of
  after up to 5 s when config.json is reread), exported.
- `src/steam_client/market.rs`:
  - `inventory_via_lang` (inventory with item names in a chosen language; `inventory_via` now calls it
    with "english"); it and `format_currency` are exported;
  - `WalletBalance.fee_rules` and `MarketFeeRules` (minimum price, price step and fees from
    `g_rgWalletInfo`: `wallet_market_minimum`, `wallet_currency_increment`, `wallet_fee_percent`,
    `wallet_publisher_fee_percent_default`), exported - to calculate market prices as the Steam site does.
- `src/steam_client/workshop.rs`, `src/core/models.rs` - `WorkshopItem` gets an image, subscribers and
  votes (`preview_url`, `subscriptions`, `votes_up`, `votes_down`); the mods query asks for votes.
- `src/web/web_session.rs`:
  - `post_form` (a form POST with the web session cookies) and `put_bytes` (a file PUT to the upload server);
  - `post_raw` (POST of a ready body - JSON, multipart - with headers) and `get_raw` (GET with headers,
    for example `x-valve-action-type` for the new market pages);
  - `get_text` follows ordinary redirects inside steamcommunity.com (up to 4 hops, except login and
    eligibilitycheck): otherwise profiles with a short address (`/profiles/<id>/` -> `/id/<name>/`)
    returned an error.

## New files

- `src/steam_client/vaporust_chat.rs`:
  - own emoticons (Player.GetEmoticonList) and stickers (from the Steam inventory 753/6);
  - uploading an image to the chat (beginfileupload -> PUT -> commitfileupload);
  - conversation history in batches (`chat_history`: time_last / ordinal_last of the oldest loaded
    message, the more_available flag) with message reactions (`HistoryMessage.reactions`);
  - `chat_events` (incoming chat messages with the message number `ordinal`, which steam-vent-chat
    drops) and `send_chat_message_ordered` (sending that returns the `ordinal`) - to tell apart two
    identical messages sent one after another.
- `src/steam_client/vaporust_props.rs` - game languages from PICS appinfo; which installed games need
  an update; the country for store prices (by connection IP) and the SteamID64 of the logged-in
  account; the latest depot versions of a branch and the build number (`latest_depot_versions`, for
  pinning a game version); depot descriptions (`depot_descriptions`: name, oslist, language, dlcappid)
  for the rollback table; all branches with a description and a password flag (`branches_detailed`);
  language depots with depotfromapp and version (`language_depots`) and depot file names
  (`depot_file_names` - CDN servers and key as for an install; `fetch_manifest_files` from
  depot_browser does not work: "no available cdn servers") - to change a language without Steam.
- `src/steam_client/vaporust_media.rs` - own screenshots in the Steam cloud (`my_cloud_screenshots`:
  PublishedFile.GetUserFiles, type "myfiles", filetype 4 - screenshots; 5 - collections) and
  screenshots of any user (`user_screenshots`, for friends).
- `src/steam_client/vaporust_store.rs` - the store discovery queue (`discovery_queue`,
  `discovery_skip`); a web access token for Steam Web API methods (`web_access_token`).
- `src/steam_client/vaporust_groups.rs` - group chats: own groups with channels (`my_chat_groups`,
  `ChatGroup.top_members` - a few members to name a group without a name; the group icon address
  `{sha}_64.jpg` as in the Steam client, `_medium.jpg` gave 404), channel history (`group_messages`),
  sending (`send_group_message`), the session's active groups (`activate_chat_groups`).
- `VAPORUST_CHANGES.md` and `VAPORUST_CHANGES.ru.md` - this list.
