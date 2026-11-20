# Изменения Vaporust в Aurelia 0.2.6

[English](VAPORUST_CHANGES.md) · **Русский**

Этот каталог - копия [Aurelia](https://github.com/Drackrath/Aurelia) версии 0.2.6
(crate `aurelia-steam`, автор - Drackrath, лицензия GPL-3.0-or-later, см. `LICENSE`).
Vaporust использует ее как библиотеку и внес перечисленные ниже изменения (ноябрь 2026).
Остальные файлы не менялись. Каждое изменение в коде помечено комментарием `vaporust`.

## Обновления копии

- **0.2.3 -> 0.2.4:** слиты изменения автора (`git merge` тега v0.2.4), конфликтов с правками
  Vaporust не было.
- **0.2.4 -> 0.2.5:** так же (тег v0.2.5). Единственный конфликт - `web/web_session.rs`: автор сам
  сделал проход через проверку допуска к торговой площадке, поэтому вместо нашей версии `get_text`
  взята авторская; наши `post_form` и `put_bytes` переведены на его хранение cookie (`cookie_header()`).
- **0.2.5 -> 0.2.6:** так же (тег v0.2.6). Конфликты: `core/config.rs` - автор сам сделал так, что
  `save_session` не пишет вход открытым текстом, если шифрование включено, поэтому взята его версия;
  `library/cloud_sync_tests.rs` - оставлены новые тесты обеих сторон. Библиотеки Steam перешли на
  aurelia-steam-vent 0.8, -chat 0.9, -proto 0.6 (Vaporust использует те же).

## Измененные файлы

- `src/steam_client.rs` - подключены новые модули (`vaporust_chat`, `vaporust_props`,
  `vaporust_media`, `vaporust_store`, `vaporust_groups`), наружу вынесены `CloudScreenshot`, `ChatGroup`,
  `ChatChannel`, `GroupMessage`, `RawChatEvent`, `RawChatKind`, `SentChatMessage`.
- `src/steam_client/chat.rs` - `recent_chat_messages` запрашивает историю с
  `bbcode_format = true`: без этого стикеры выпадают из истории, а смайлики и картинки приходят
  упрощенными.
- `src/steam_client/content.rs`:
  - `fetch_achievements` вынесена в `fetch_achievements_for(appid, язык, steam_id)`: достижения
    любого пользователя (сравнение с другом);
  - `CELL_OVERRIDE` - регион загрузки, выбранный пользователем (0 - как назначил Steam), вынесен наружу.
- `src/steam_client/friends.rs`:
  - `Friend.avatar_hash` (хэш аватара в hex) из `CMsgClientPersonaState` - аватарки в списке друзей;
  - `Friend.nickname` (свой ник для друга из `CMsgClientPlayerNicknameList`, разбирается вместе со
    списком друзей сразу после входа, `apply_nicknames`) и `Friend.rich_status` (текст "status" из
    rich presence игры); `PERSONA_REQUEST_FLAGS` дополнен флагом 4096 (rich presence).
- `src/steam_client/ident_tests.rs` - в тестовом `Friend` добавлены поля `nickname` и `rich_status`
  (иначе тесты Aurelia не собирались после доработки друзей).
- `src/steam_client/install.rs` - `set_download_limit_now` (ограничение скорости применяется сразу, а
  не через 5 с при перечитывании config.json), вынесена наружу.
- `src/steam_client/market.rs`:
  - `inventory_via_lang` (инвентарь с названиями на выбранном языке; `inventory_via` теперь вызывает
    ее с "english"); она и `format_currency` вынесены наружу;
  - `WalletBalance.fee_rules` и `MarketFeeRules` (минимальная цена, шаг цены и комиссии из
    `g_rgWalletInfo`: `wallet_market_minimum`, `wallet_currency_increment`, `wallet_fee_percent`,
    `wallet_publisher_fee_percent_default`), вынесены наружу - для расчета цен площадки как на сайте Steam.
  - `InventoryItem.name_color` (цвет редкости) и `InventoryItem.description` (строки описания без
    HTML-тегов) - для плиток и карточки предмета, как на странице инвентаря Steam.
- `src/steam_client/workshop.rs`, `src/core/models.rs` - у `WorkshopItem` добавлены картинка,
  подписчики и оценки (`preview_url`, `subscriptions`, `votes_up`, `votes_down`); запрос модов просит оценки.
- `src/web/web_session.rs`:
  - `post_form` (POST формы с куками веб-сессии) и `put_bytes` (PUT файла на сервер загрузки);
  - `post_raw` (POST готового тела - JSON, multipart - с заголовками) и `get_raw` (GET с заголовками,
    например `x-valve-action-type` для новых страниц площадки);
  - `get_text` проходит обычные перенаправления внутри steamcommunity.com (до 4 переходов, кроме входа
    и eligibilitycheck): иначе профили с коротким адресом (`/profiles/<id>/` -> `/id/<имя>/`)
    возвращались ошибкой.

## Новые файлы

- `src/steam_client/vaporust_chat.rs`:
  - свои смайлики (Player.GetEmoticonList) и стикеры (из инвентаря Steam 753/6);
  - загрузка картинки в чат (beginfileupload -> PUT -> commitfileupload);
  - история переписки порциями (`chat_history`: time_last / ordinal_last самого старого загруженного
    сообщения, признак more_available) с реакциями на сообщения (`HistoryMessage.reactions`);
  - `chat_events` (входящие сообщения чата вместе с номером сообщения `ordinal`, который
    steam-vent-chat отбрасывает) и `send_chat_message_ordered` (отправка с возвратом `ordinal`) -
    чтобы различать два одинаковых сообщения, отправленных подряд.
- `src/steam_client/vaporust_props.rs` - языки игры из PICS appinfo; каким установленным играм нужно
  обновление; страна для цен магазина (по IP подключения) и SteamID64 вошедшего аккаунта; последние
  версии хранилищ ветки и номер сборки (`latest_depot_versions`, для закрепления версии игры);
  описания хранилищ (`depot_descriptions`: имя, oslist, язык, dlcappid) для таблицы отката; все ветки
  с описанием и признаком пароля (`branches_detailed`); языковые хранилища с depotfromapp и версией
  (`language_depots`) и имена файлов хранилища (`depot_file_names` - серверы CDN и ключ как у установки;
  `fetch_manifest_files` из depot_browser не работает: «no available cdn servers») - для смены языка
  без Steam.
- `src/steam_client/vaporust_media.rs` - свои скриншоты в облаке Steam (`my_cloud_screenshots`:
  PublishedFile.GetUserFiles, type "myfiles", filetype 4 - скриншоты; 5 - коллекции) и скриншоты
  любого пользователя (`user_screenshots`, для друзей).
- `src/steam_client/vaporust_store.rs` - очередь открытий магазина (`discovery_queue`,
  `discovery_skip`); веб-токен доступа для методов Web API Steam (`web_access_token`).
- `src/steam_client/vaporust_groups.rs` - групповые чаты: свои группы с каналами (`my_chat_groups`,
  `ChatGroup.top_members` - несколько участников для названия группы без имени; адрес значка группы
  `{sha}_64.jpg`, как в клиенте Steam, `_medium.jpg` давал 404), история канала (`group_messages`),
  отправка (`send_group_message`), активные группы сеанса (`activate_chat_groups`).
- `src/library/cloud_sync.rs`: скачанный файл, который не совпал с ожидаемым хэшем ни как есть, ни
  после распаковки, - ошибка (с повтором; испорченные данные не записываются поверх сохранения, которое
  следующая синхронизация отправила бы в облако); список файлов облака читается постранично до
  `total_files`; `WinMyDocuments` и `WinSavedGames` - настоящие папки Windows (Документы на OneDrive или
  другом диске).
- `src/core/session_crypto.rs`: новый ключ в хранилище Windows создается только если записи нет
  (временная ошибка хранилища больше не заменяет ключ зашифрованного входа); nonce неверной длины -
  ошибка, а не падение.
- `src/steam_client/vaporust_props.rs` `language_depots`: номер манифеста читается через
  `extract_manifest_id_robust` (в двоичном PICS он бывает числом).
- `Cargo.toml`: возможность `Win32_System_Com` у windows-sys (для папок Windows).
- `VAPORUST_CHANGES.md` и `VAPORUST_CHANGES.ru.md` - этот список.
