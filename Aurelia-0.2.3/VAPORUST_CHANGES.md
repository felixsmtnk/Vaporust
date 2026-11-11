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
- `src/web/web_session.rs` — добавлены методы `post_form` (POST формы с куками
  веб-сессии) и `put_bytes` (PUT файла на сервер загрузки).

## Новые файлы

- `src/steam_client/vaporust_chat.rs` — список своих смайликов (Player.GetEmoticonList)
  и стикеров (из инвентаря Steam 753/6); загрузка картинки в чат
  (beginfileupload -> PUT -> commitfileupload).
- `src/steam_client/vaporust_props.rs` — языки игры из PICS appinfo; проверка, каким
  установленным играм нужно обновление.
- `VAPORUST_CHANGES.md` — этот файл.

Все изменения в коде помечены комментарием `vaporust`.
