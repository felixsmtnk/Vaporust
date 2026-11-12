# Изменения Vaporust в Aurelia 0.2.3

Этот каталог — копия [Aurelia](https://github.com/Drackrath/Aurelia) версии 0.2.3
(crate `aurelia-steam`, автор — Drackrath, лицензия GPL-3.0-or-later, см. `LICENSE`).
Проект Vaporust использует её как библиотеку и внёс следующие изменения
(ноябрь 2026). Остальные файлы не менялись.

## Изменённые файлы

- `src/steam_client/chat.rs` — `recent_chat_messages` запрашивает историю с
  `bbcode_format = true`: без этого стикеры выпадают из истории, а смайлики и
  картинки приходят упрощёнными.
- `src/steam_client.rs` — подключены два новых модуля (`vaporust_chat`, `vaporust_props`).
- `src/steam_client/market.rs` — добавлена `inventory_via_lang` (инвентарь с выбранным
  языком названий; `inventory_via` теперь вызывает её с "english"); в `steam_client.rs`
  она и `format_currency` вынесены в публичный `pub use`.
- `src/web/web_session.rs` — добавлены методы `post_form` (POST формы с куками
  веб-сессии) и `put_bytes` (PUT файла на сервер загрузки). `get_text` проходит
  перенаправления Steam через `/market/eligibilitycheck/` (как браузер: с cookie,
  которые ставит эта страница), а не считает их отказом в доступе к площадке.

## Новые файлы

- `src/steam_client/vaporust_chat.rs` — список своих смайликов (Player.GetEmoticonList)
  и стикеров (из инвентаря Steam 753/6); загрузка картинки в чат
  (beginfileupload -> PUT -> commitfileupload).
- `src/steam_client/vaporust_props.rs` — языки игры из PICS appinfo; проверка, каким
  установленным играм нужно обновление; страна для цен магазина (по IP подключения) и
  SteamID64 вошедшего аккаунта; последние версии хранилищ ветки и номер сборки
  (`latest_depot_versions`, для закрепления версии игры). Описания хранилищ (`depot_descriptions`:
  имя, oslist, язык, dlcappid) для таблицы отката. Все ветки с описанием и признаком пароля
  (`branches_detailed`).
- `VAPORUST_CHANGES.md` — этот файл.

Все изменения в коде помечены комментарием `vaporust`.
