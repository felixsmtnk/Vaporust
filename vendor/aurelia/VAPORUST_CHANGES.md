# Изменения Vaporust в Aurelia 0.2.5

Этот каталог - копия [Aurelia](https://github.com/Drackrath/Aurelia) версии 0.2.5
(crate `aurelia-steam`, автор - Drackrath, лицензия GPL-3.0-or-later, см. `LICENSE`).
Проект Vaporust использует ее как библиотеку и внес следующие изменения
(ноябрь 2026). Остальные файлы не менялись.

В ноябре 2026 копия обновлена с 0.2.3 до 0.2.4 слиянием изменений автора (git merge тега v0.2.4):
конфликтов с правками Vaporust не было.
Затем до 0.2.5 (git merge тега v0.2.5). Единственный конфликт - `web/web_session.rs`: автор сам
сделал проход через проверку допуска к торговой площадке (eligibilitycheck), поэтому вместо нашей
версии `get_text` взята авторская; наши `post_form` и `put_bytes` переведены на его хранение cookie
(`cookie_header()`).

## Измененные файлы

- `src/steam_client/chat.rs` - `recent_chat_messages` запрашивает историю с
  `bbcode_format = true`: без этого стикеры выпадают из истории, а смайлики и
  картинки приходят упрощенными.
- `src/steam_client.rs` - подключены новые модули (`vaporust_chat`, `vaporust_props`, `vaporust_media`; из последнего наружу - `CloudScreenshot`).
- `src/steam_client/market.rs` - добавлена `inventory_via_lang` (инвентарь с выбранным
  языком названий; `inventory_via` теперь вызывает ее с "english"); в `steam_client.rs`
  она и `format_currency` вынесены в публичный `pub use`.
- `src/steam_client/install.rs` - добавлена `set_download_limit_now` (ограничение скорости
  применяется сразу, а не через 5 с при перечитывании config.json); вынесена в `pub use`.
- `src/web/web_session.rs` - добавлены методы `post_form` (POST формы с куками
  веб-сессии) и `put_bytes` (PUT файла на сервер загрузки). Проход через
  `/market/eligibilitycheck/` с 0.2.5 делает сам автор в `get_text`, наша версия убрана.
  В `get_text` добавлен проход обычных перенаправлений внутри steamcommunity.com (до 4 переходов,
  кроме входа и eligibilitycheck): иначе профили с коротким адресом (`/profiles/<id>/` ->
  `/id/<имя>/`) возвращались ошибкой.
- `src/steam_client/content.rs` - `fetch_achievements` вынесена в `fetch_achievements_for(appid,
  язык, steam_id)`: достижения любого пользователя (сравнение с другом).

## Новые файлы

- `src/steam_client/vaporust_chat.rs` - список своих смайликов (Player.GetEmoticonList)
  и стикеров (из инвентаря Steam 753/6); загрузка картинки в чат
  (beginfileupload -> PUT -> commitfileupload).
- `src/steam_client/vaporust_props.rs` - языки игры из PICS appinfo; проверка, каким
  установленным играм нужно обновление; страна для цен магазина (по IP подключения) и
  SteamID64 вошедшего аккаунта; последние версии хранилищ ветки и номер сборки
  (`latest_depot_versions`, для закрепления версии игры). Описания хранилищ (`depot_descriptions`:
  имя, oslist, язык, dlcappid) для таблицы отката. Все ветки с описанием и признаком пароля
  (`branches_detailed`). Языковые хранилища с depotfromapp и версией (`language_depots`) и имена
  файлов хранилища (`depot_file_names` - серверы CDN и ключ как у установки; `fetch_manifest_files` из
  depot_browser не работает: «no available cdn servers») - для смены языка без Steam.
- `src/steam_client/vaporust_media.rs` - свои скриншоты в облаке Steam (`my_cloud_screenshots`:
  PublishedFile.GetUserFiles, type "myfiles", filetype 4 - скриншоты; 5 - коллекции). Скриншоты
  любого пользователя (`user_screenshots`, для друзей).
- `src/steam_client/vaporust_store.rs` - очередь открытий магазина (`discovery_queue`, `discovery_skip`);
  веб-токен доступа для методов Web API Steam (`web_access_token`).
- `src/steam_client/vaporust_groups.rs` - групповые чаты: свои группы с каналами (`my_chat_groups`),
  история канала (`group_messages`), отправка (`send_group_message`), активные группы сеанса
  (`activate_chat_groups`).
- `src/steam_client/vaporust_chat.rs` (дополнение) - история переписки порциями (`chat_history`: time_last/
  ordinal_last самого старого загруженного сообщения, признак more_available).
- `src/core/models.rs`, `src/steam_client/workshop.rs` - у `WorkshopItem` добавлены картинка, подписчики и
  оценки (`preview_url`, `subscriptions`, `votes_up`, `votes_down`), запрос модов просит оценки.
- `VAPORUST_CHANGES.md` - этот файл.

Все изменения в коде помечены комментарием `vaporust`.
- `src/steam_client/vaporust_groups.rs`: `ChatGroup.top_members` (несколько участников - для названия группы
  без имени); адрес значка группы `{sha}_64.jpg` (как в клиенте Steam; `_medium.jpg` давал 404).
- `steam_client/friends.rs`: поле `Friend.avatar_hash` (хэш аватара в hex) из `CMsgClientPersonaState`
  - для аватарок в списке друзей.
- `steam_client/friends.rs`: `Friend.nickname` (свой ник для друга из `CMsgClientPlayerNicknameList`, разбирается
  вместе со списком друзей сразу после входа, `apply_nicknames`) и `Friend.rich_status` (текст "status" из rich
  presence игры); `PERSONA_REQUEST_FLAGS` дополнен флагом 4096 (rich presence).
- `web/web_session.rs`: `post_raw` (POST готового тела - JSON, multipart - с заголовками) и `get_raw` (GET с
  заголовками, например `x-valve-action-type` для новых страниц площадки).
- `steam_client/content.rs`: `CELL_OVERRIDE` (регион загрузки, выбранный пользователем; 0 - как назначил Steam),
  вынесен в `pub use`.
- `steam_client/vaporust_chat.rs`: `HistoryMessage.reactions` (реакции на сообщения из истории переписки).
- `steam_client/ident_tests.rs`: в тестовом `Friend` добавлены поля `nickname` и `rich_status` (иначе тесты Aurelia не
  собирались после нашей доработки друзей).
- `steam_client/market.rs`: `WalletBalance.fee_rules` и `MarketFeeRules` (минимальная цена, шаг цены и комиссии из
  `g_rgWalletInfo`: `wallet_market_minimum`, `wallet_currency_increment`, `wallet_fee_percent`,
  `wallet_publisher_fee_percent_default`), вынесено в `pub use` - для расчета цен площадки как на сайте Steam.
- `steam_client/vaporust_chat.rs`: `chat_events` (входящие сообщения чата вместе с номером сообщения `ordinal`,
  который steam-vent-chat отбрасывает) и `send_chat_message_ordered` (отправка с возвратом `ordinal`);
  наружу - `RawChatEvent`, `RawChatKind`, `SentChatMessage`. Нужно, чтобы различать два одинаковых сообщения,
  отправленных подряд.
