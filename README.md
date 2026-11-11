# Vaporust: лёгкий клиент Steam для Windows

**Быстрый и лёгкий лаунчер и менеджер библиотеки Steam на Rust + egui. Устанавливайте,
обновляйте и запускайте игры Steam без тяжёлого официального интерфейса.**

Vaporust основан на [Aurelia](https://github.com/Drackrath/Aurelia), но с удобным оконным
интерфейсом, своими возможностями и сделан под Windows.

[![License: GPL-3.0-or-later](https://img.shields.io/badge/License-GPL--3.0--or--later-blue.svg)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-orange.svg)](https://www.rust-lang.org/)
[![Status: early alpha](https://img.shields.io/badge/status-early%20alpha-red.svg)](#состояние-проекта)
[![Platform: Windows](https://img.shields.io/badge/platform-Windows%2010%2B-0078d6.svg)](#установка)

Название — от *vapor* (пар) + *Rust*. [English summary below](#english).

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

## Содержание

- [Зачем Vaporust?](#зачем-vaporust)
- [Сравнение](#сравнение)
- [Состояние проекта](#состояние-проекта)
- [Установка](#установка)
- [Сборка из исходников](#сборка-из-исходников)
- [Как пользоваться](#как-пользоваться)
- [Как это устроено](#как-это-устроено)
- [Вопросы и ответы](#вопросы-и-ответы)
- [Благодарности](#благодарности)
- [Похожие проекты](#похожие-проекты)
- [Лицензия](#лицензия)
- [English](#english)

---

## Зачем Vaporust?

- **Без встроенного браузера.** Интерфейс нарисован на [egui](https://github.com/emilk/egui):
  никаких Electron, CEF и steamwebhelper. Сам лаунчер занимает около 60–150 МБ памяти, тогда как
  у официального клиента только steamwebhelper легко занимает сотни мегабайт.
- **Игры запускаются через настоящий Steam.** Никаких эмуляторов steam_api: Steam поднимается
  незаметно, в урезанном режиме, только на время игры и закрывается вместе с ней. Достижения,
  облачные сохранения, оверлей и защита от копирования работают как обычно.
- **Для слабых компьютеров.** Делался для старого ноутбука, на котором официальный клиент
  тормозит. Пока идёт игра или обновление, тяжёлый steamwebhelper можно «заморозить»:
  он перестаёт занимать оперативную память.
- **Всё нужное под рукой.** Установка и обновление игр, свойства игры как в Steam, достижения,
  друзья и чат со смайликами, стикерами и картинками.
- **Открытый код.** Лицензия GPL-3.0-or-later.

### Сравнение

| | Официальный Steam | [Aurelia](https://github.com/Drackrath/Aurelia) | **Vaporust** |
|---|---|---|---|
| **Интерфейс** | Окна (встроенный браузер CEF) | Командная строка | Окна (egui, без браузера) |
| **Платформы** | Windows, Linux, macOS | Linux (в первую очередь), Windows | Windows |
| **Запуск игр** | Сам Steam | Напрямую, через Proton/Wine или Steam | Всегда через настоящий Steam (скрытый) |
| **Установка и обновление** | Да | Да | Да (установка — Aurelia, обновление — Steam) |
| **Друзья и чат** | Да | Да (текст) | Да (смайлики, стикеры, картинки) |
| **Открытый код** | Нет | Да (GPL-3.0) | Да (GPL-3.0) |

---

## Состояние проекта

Vaporust в **ранней альфе**: основное работает и проверено вручную на одном компьютере
(Windows 10), но ошибки вполне вероятны.

| Раздел | Статус | Что работает |
|---|---|---|
| **Вход** | ✅ | Пароль + Steam Guard (код из почты или приложения), вход по QR-коду, восстановление сессии |
| **Библиотека** | ✅ | Купленные и установленные игры, время в игре, поиск, сортировка (по имени, недавно запущенные, по времени в игре, по размеру), отметка игр с доступным обновлением |
| **Запуск** | ✅ | Через скрытый Steam; Steam закрывается после игры; заморозка steamwebhelper на время игры |
| **Установка** | ✅ | Загрузка с прогрессом, скоростью и оставшимся временем, проверка места на диске, докачка, выбор папки библиотеки, очередь загрузок с паузой у каждой игры |
| **Обновления** | ✅ | Проверка, каким играм нужно обновление; обновление через скрытый Steam без запуска игры |
| **Свойства игры** | ✅ | Параметры запуска, язык, версии и беты, проверка целостности, перенос на другой диск, удаление, DLC, сохранения в облаке |
| **Достижения** | ✅ | Список с описаниями и редкостью, на русском |
| **Друзья и чат** | ✅ | Отдельное окно: статусы и игры друзей, история, смайлики, анимированные стикеры, картинки (выбор файла, перетаскивание, Ctrl+V), свой статус (в сети / отошёл / не беспокоить / невидимка), уведомления о сообщениях (звук, мигание в панели задач) |
| **Удобство** | ✅ | Настройки и размер окна сохраняются между запусками, иконка в трее («Крестик — в трей») |
| **Закрытые беты с паролем** | 🚧 | Пока нет |
| **Магазин, список желаемого, новости** | 🚧 | В планах (возможности уже есть в Aurelia) |
| **Инвентарь, кошелёк, коллекции** | 🚧 | В планах |

Планы по версиям — в [ROADMAP.md](ROADMAP.md).

---

## Установка

**Требования:**

- Windows 10 или новее, 64-битная.
- Установленный официальный Steam, в который хотя бы раз выполнен вход с галочкой
  «Запомнить пароль». Vaporust запускает игры через него.

**Готовая программа:** скачайте архив со страницы [Releases](../../releases), распакуйте
в любую папку и запустите `vaporust.exe`. Установка не нужна.

---

## Сборка из исходников

Нужен [Rust](https://rustup.rs/) (проверено на 1.90, `x86_64-pc-windows-msvc`) и
Microsoft C++ Build Tools.

```bash
git clone https://github.com/felixsmtnk/Vaporust.git
cd Vaporust
cargo build --release
```

Готовая программа: `target\release\vaporust.exe`.

---

## Как пользоваться

1. **Вход.** При первом запуске войдите в аккаунт Steam: логин и пароль (плюс код Steam Guard)
   или QR-код из мобильного приложения Steam. Дальше вход запоминается.
2. **Играть.** Нажмите «Играть». Steam откроется незаметно, игра запустится, а после выхода
   из неё Steam закроется сам. Если игре нужно обновление, Steam сначала докачает его,
   прогресс виден внизу окна.
3. **Установить.** Снимите галочку «Только установленные» и нажмите «Установить» у нужной игры.
4. **Обновить.** У игр с доступным обновлением есть жёлтая кнопка «Обновить».
5. **Свойства игры** — кнопка ⚙: параметры запуска, язык, беты, файлы, DLC, облако.
   Менять их можно, когда Steam закрыт: при выходе он перезаписывает свои файлы.
6. **Друзья** — кнопка «Друзья» вверху; щелчок по другу открывает чат в отдельном окне.
   Статус для друзей (в сети, невидимка и т. д.) выбирается там же.
7. **Трей.** Галочка «Крестик — в трей» внизу окна: крестик прячет Vaporust к часам, чат
   продолжает работать. Вернуть — щелчок по иконке, выйти — правый щелчок → «Выход».
8. **Очередь загрузок.** Пока одна игра качается, «Установить» у другой ставит её в очередь;
   у каждой игры в очереди есть «пауза» и «продолжить».

---

## Как это устроено

- **Запуск игр** — всегда через настоящий Steam с флагами `-silent -no-browser`.
  Vaporust следит за процессами Steam и игры, а после игры закрывает Steam.
- **Работа с сетью Steam** (вход, список игр, загрузка, достижения, друзья, чат) идёт через
  библиотеку [Aurelia](https://github.com/Drackrath/Aurelia). Её копия лежит в каталоге
  `vendor/aurelia` (версия 0.2.3) с небольшими доработками; список изменений —
  [`vendor/aurelia/VAPORUST_CHANGES.md`](vendor/aurelia/VAPORUST_CHANGES.md).
- **Язык и бета** записываются в файл игры (appmanifest) так же, как это делает Steam,
  а нужные файлы докачивает сам Steam.
- **Параметры запуска** хранятся там же, где их хранит Steam (`localconfig.vdf`), поэтому
  они видны и в официальном клиенте.

---

## Вопросы и ответы

### Нужен ли официальный Steam?

Да. Vaporust сознательно запускает игры через настоящий Steam, чтобы работали достижения,
облако, защита от копирования и всё остальное. Официальный клиент просто открывается
незаметно и только на время игры.

### Чем это отличается от Aurelia?

[Aurelia](https://github.com/Drackrath/Aurelia) — консольный клиент Steam (в первую очередь
для Linux), который умеет запускать игры и без официального Steam. Vaporust — оконный лаунчер
для Windows, построенный на Aurelia: он берёт у неё работу с сетью Steam, а игры всегда
запускает через официальный клиент.

### Это безопасно? Могут ли забанить?

Vaporust неофициальный, не связан с Valve и напрямую меняет файлы Steam. Сторонние программы,
работающие со Steam, несут риск санкций против аккаунта. Прочитайте отказ от ответственности
в начале этого файла. Используйте на свой страх и риск.

### Работает ли на Linux или macOS?

Нет, Vaporust сделан под Windows. На Linux попробуйте [Aurelia](https://github.com/Drackrath/Aurelia).

### Сколько памяти занимает?

Сам лаунчер — около 60–150 МБ, в зависимости от открытых окон (чат с картинками занимает больше).
Пока идёт игра, к этому добавляется Steam; его steamwebhelper можно заморозить галочкой внизу окна.

---

## Благодарности

Vaporust вдохновлён **[Aurelia](https://github.com/Drackrath/Aurelia)** и построен на её коде.
Вход в Steam, загрузка игр, достижения, друзья, чат и многое другое — это работа автора Aurelia,
**Drackrath**. Огромное спасибо: без Aurelia этого проекта бы не было.

Aurelia, в свою очередь, выросла из **[SteamFlow](https://github.com/weter11/SteamFlow)**
и стоит на **[steam-vent](https://codeberg.org/steam-vent/steam-vent)**, где проделана
огромная работа по разбору сетевого протокола Steam.

### Использованные проекты

- [Aurelia](https://github.com/Drackrath/Aurelia): консольный клиент Steam на Rust, основа Vaporust (входит в проект изменённой копией). Спасибо!
- [SteamFlow](https://github.com/weter11/SteamFlow): проект, из которого выросла Aurelia
- [steam-vent](https://codeberg.org/steam-vent/steam-vent): реализация сетевого протокола Steam
- [steam-vent-chat](https://codeberg.org/steam-vent/chat): протокол чата Steam (в Aurelia — изменённый форк [aurelia-steam-vent-chat](https://codeberg.org/Drackrath/aurelia-steam-vent-chat))
- [steam-cdn](https://github.com/OccamyLab/steam-cdn): загрузка содержимого игр (в Aurelia — форк [aurelia-steam-cdn](https://github.com/Drackrath/aurelia-steam-cdn))
- [egui / eframe](https://github.com/emilk/egui): библиотека интерфейса; `egui_extras` — загрузка картинок в чате
- [tokio](https://tokio.rs/), [sysinfo](https://github.com/GuillaumeGomez/sysinfo), [winreg](https://github.com/gentoo90/winreg-rs), [windows-sys](https://github.com/microsoft/windows-rs), [image](https://github.com/image-rs/image), [arboard](https://github.com/1Password/arboard), [rfd](https://github.com/PolyMeilex/rfd), [chrono](https://github.com/chronotope/chrono), [qrcode](https://github.com/kennytm/qrcode-rust)

### Исследования протокола

- [SteamDatabase/Protobufs](https://github.com/SteamDatabase/Protobufs) и [SteamKit2](https://github.com/SteamRE/SteamKit): описания протоколов Steam
- [node-steamcommunity](https://github.com/DoctorMcKay/node-steamcommunity) и [node-steam-user](https://github.com/DoctorMcKay/node-steam-user): описание того, как отправлять картинки и стикеры в чат Steam

---

## Похожие проекты

- [Aurelia](https://github.com/Drackrath/Aurelia): консольный клиент Steam на Rust, на котором построен Vaporust
- [SteamFlow](https://github.com/weter11/SteamFlow): проект, из которого выросла Aurelia
- [OpenSteamClient](https://github.com/OpenSteamClient/OpenSteamClient): открытый клиент Steam на C++ и Qt
- [SteamCMD](https://developer.valvesoftware.com/wiki/SteamCMD): официальная консольная утилита Valve для загрузки игр и серверов

---

## Лицензия

Vaporust распространяется по лицензии [GPL-3.0-or-later](LICENSE), как и Aurelia, на коде
которой он основан. История лицензии Aurelia — в файле [`vendor/aurelia/LICENSE`](vendor/aurelia/LICENSE).

---

## English

**Vaporust** (*vapor* + *Rust*) is a lightweight Steam client and launcher for Windows, written
in Rust with an [egui](https://github.com/emilk/egui) interface — no embedded browser. It is
built for low-end PCs where the official client's steamwebhelper is too heavy.

- Games are **always launched through the real Steam client**, started hidden and closed
  together with the game; steamwebhelper can be frozen while you play.
- Install and update games, game properties (launch options, language, betas, file
  verification, moving, uninstalling, DLC, cloud saves), achievements, and friends & chat
  with emoticons, animated stickers and images.
- Steam networking is provided by a lightly modified copy of
  **[Aurelia](https://github.com/Drackrath/Aurelia)** by Drackrath — huge thanks; this project
  is inspired by and built on it (changes are listed in `vendor/aurelia/VAPORUST_CHANGES.md`).
- Based on Aurelia, but with a friendly windowed interface, its own features, and made for Windows.
- **Status: early alpha.** The interface is in Russian for now.

**Disclaimer:** an independent, unofficial project, **not affiliated with, endorsed or
sponsored by Valve Corporation or Steam**. Steam and Valve are trademarks of Valve
Corporation. The software modifies Steam's files and is provided **"as is", without any
warranty**; the authors are **not liable** for any consequences of its use, including data
loss, broken installations or account restrictions. Use at your own risk.

License: [GPL-3.0-or-later](LICENSE).
