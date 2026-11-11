# Vaporust

**Лёгкий клиент-лаунчер Steam для Windows на Rust + egui.** Сделан для слабых компьютеров,
где официальный клиент Steam со встроенным браузером (steamwebhelper) работает слишком тяжело.
Название — от *vapor* (пар) + *Rust*.

> [!WARNING]
> **Отказ от ответственности. Прочитайте перед использованием.**
>
> - Vaporust — **независимый неофициальный проект**. Он **не связан с Valve Corporation
>   и Steam**, не одобрен, не спонсируется и не поддерживается ими.
> - «Steam», «Valve», логотипы Steam и названия игр — товарные знаки и собственность
>   Valve Corporation и соответствующих правообладателей. Все права на Steam и его сервисы
>   принадлежат Valve.
> - Программа **напрямую меняет файлы Steam** (настройки запуска, файлы игр) и работает
>   с сетью Steam не через официальный клиент. Это может повредить установку Steam
>   или игры, а Valve может ограничить аккаунт за использование неофициальных программ.
> - Программа предоставляется **«как есть», без каких-либо гарантий**. Авторы
>   **не несут ответственности** за любые последствия её использования: потерю данных,
>   повреждение установки, блокировку аккаунта и т. п. Используйте на свой страх и риск.

## Что умеет

- **Библиотека**: купленные и установленные игры, время в игре, поиск.
- **Запуск через настоящий Steam**: Steam поднимается незаметно, в урезанном режиме,
  и закрывается вместе с игрой. Можно «заморозить» тяжёлый steamwebhelper на время игры.
- **Установка игр** с прогрессом, скоростью и оставшимся временем; докачка прерванных.
- **Обновления**: проверка, каким играм нужно обновление, и обновление через скрытый Steam
  без запуска игры.
- **Свойства игры**, как в Steam: параметры запуска, язык, версии и беты, проверка
  целостности файлов, перенос на другой диск, удаление, DLC, сохранения в облаке.
- **Достижения**.
- **Друзья и чат** в отдельном окне: статусы и игры друзей, история переписки, смайлики,
  анимированные стикеры, картинки (в том числе по Ctrl+V и перетаскиванием).

## Как это работает

Игры запускаются **только через настоящий Steam** (эмуляторы steam_api не используются):
лаунчер поднимает его с флагами `-silent -no-browser`, ждёт игру и закрывает Steam после неё.
Вход в аккаунт, список игр, загрузка, чат и прочая работа с сетью Steam идут через библиотеку
[Aurelia](https://github.com/Drackrath/Aurelia) — её копия лежит в каталоге `Aurelia-0.2.3`
с небольшими доработками (список — в `Aurelia-0.2.3/VAPORUST_CHANGES.md`).

## Требования

- Windows 10 или новее.
- Установленный Steam, в который хотя бы раз выполнен вход с галочкой «Запомнить пароль».

## Сборка

Нужен [Rust](https://rustup.rs/) (проверено на 1.90, MSVC).

```bash
cargo build --release
```

Готовая программа: `target\release\vaporust.exe`.

## Благодарности

- **[Aurelia](https://github.com/Drackrath/Aurelia)** (автор — Drackrath): консольный
  клиент Steam на Rust. Огромное спасибо! Vaporust вдохновлён Aurelia и построен на её коде:
  вход в Steam, загрузка игр, достижения, друзья, чат и многое другое — это Aurelia.
- **[SteamFlow](https://github.com/weter11/SteamFlow)**: проект, из которого выросла Aurelia.
- **[steam-vent](https://codeberg.org/steam-vent/steam-vent)**: реализация сетевого
  протокола Steam, на которой работает Aurelia.
- **[egui](https://github.com/emilk/egui)**: библиотека интерфейса.

## Лицензия

[GPL-3.0-or-later](LICENSE), как и у Aurelia, на коде которой основан проект.
История лицензии Aurelia — в файле `Aurelia-0.2.3/LICENSE`.

---

## English

**Vaporust** is a lightweight Steam client/launcher for Windows written in Rust + egui,
built for low-end PCs where the official client's embedded browser is too heavy. Games are
always launched through the real Steam client, started hidden and closed together with the
game. Networking (login, library, downloads, achievements, friends, chat) is provided by a
lightly modified copy of [Aurelia](https://github.com/Drackrath/Aurelia) by Drackrath —
many thanks; this project is inspired by and built on it.

**Disclaimer:** an independent, unofficial project, **not affiliated with, endorsed or
sponsored by Valve Corporation or Steam**. Steam and Valve are trademarks of Valve
Corporation. The software modifies Steam's files and is provided **"as is", without any
warranty**; the authors are **not liable** for any consequences of its use, including data
loss, broken installations or account restrictions. Use at your own risk.

License: [GPL-3.0-or-later](LICENSE).
