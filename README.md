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
  магазин со скидками и списком желаемого, друзья и чат со смайликами, стикерами и картинками.
- **Открытый код.** Лицензия GPL-3.0-or-later.

### Сравнение

| | Официальный Steam | [Aurelia](https://github.com/Drackrath/Aurelia) | **Vaporust** |
|---|---|---|---|
| **Интерфейс** | Окна (встроенный браузер CEF) | Командная строка | Окна (egui, без браузера) |
| **Платформы** | Windows, Linux, macOS | Linux (в первую очередь), Windows | Windows 10/11, 64 и 32 бит |
| **Запуск игр** | Сам Steam | Напрямую, через Proton/Wine или Steam | Через скрытый Steam; игры без защиты — напрямую, без Steam |
| **Установка и обновление** | Да | Да | Да (установка — Aurelia, обновление — Steam), откат и закрепление версии |
| **Друзья и чат** | Да | Да (текст) | Да (смайлики, стикеры, картинки) |
| **Открытый код** | Нет | Да (GPL-3.0) | Да (GPL-3.0) |

---

## Состояние проекта

Vaporust в **альфе**: основное работает и проверено вручную на одном компьютере
(Windows 10, 64 бит), но ошибки вполне вероятны. Пункты с пометкой «новое» появились в 0.2.0-alpha
и ещё ждут проверки на разных компьютерах.

| Раздел | Статус | Что работает |
|---|---|---|
| **Вход** | ✅ | Пароль + Steam Guard (код из почты или приложения), вход по QR-коду, восстановление сессии |
| **Библиотека** | ✅ | Купленные и установленные игры, время в игре, поиск, сортировка, коллекции Steam, вид «Плитка» с обложками или «Список», страница игры с фоном, достижениями и новостями; «Уже скачана…» — добавить игру с диска; поиск дублей установок |
| **Запуск** | ✅ | Через скрытый Steam (закрывается после игры, steamwebhelper замораживается); **новое:** запуск без Steam для игр без защиты |
| **Установка** | ✅ | Прогресс, скорость, проверка места, докачка, выбор папки, очередь загрузок с паузой; **новое:** ограничение скорости |
| **Обновления игр** | ✅ | Проверка и обновление через скрытый Steam; **новое:** откат на старую версию и закрепление версии |
| **Свойства игры** | ✅ | Параметры запуска, язык, версии и беты (с описаниями), проверка файлов, перенос, удаление, DLC, Мастерская; **новое:** облачные сохранения вручную (скачать, отправить, выбор при расхождении) |
| **Друзья и чат** | ✅ | Статусы и игры друзей, история, смайлики, анимированные стикеры, картинки, свой статус, уведомления, профили, заявки в друзья |
| **Магазин** | ✅ | Поиск, скидки, **события и распродажи**, список желаемого, сортировка и фильтры, страница игры, регион цен; покупка — на сайте Steam |
| **Инвентарь и рынок** | ✅ | Инвентарь по играм с ценами, кошелёк, поиск по торговой площадке, свои лоты и заявки; продажа — на сайте Steam |
| **Программа** | ✅ | Вкладка «Настройки», трей, **автообновление**, **портативный режим**, прокси, 32-битная сборка |
| **Закрытые беты с паролем** | ⏸ | Пока включаются в самом Steam |

Планы по версиям — в [ROADMAP.md](ROADMAP.md).

---

## Установка

**Требования:**

- Windows 10 или новее: 64-битная (архив `windows-x64`) или 32-битная (архив `windows-x86`).
- Установленный официальный Steam, в который хотя бы раз выполнен вход с галочкой
  «Запомнить пароль». Vaporust запускает игры через него (кроме игр без защиты, если включить
  «Запускать без Steam»).

**Готовая программа:** скачайте архив со страницы [Releases](../../releases), распакуйте
в любую папку и запустите `vaporust.exe`. Установка не нужна. Новые версии Vaporust находит сам
и ставит одной кнопкой.

**Портативный режим (для флешки):** «Настройки» → «О программе» → «Портативный режим», либо
просто положите рядом с `vaporust.exe` пустой файл `portable.txt`. Тогда настройки и вход хранятся
в папке `VaporustData` рядом с программой.

---

## Сборка из исходников

Нужен [Rust](https://rustup.rs/) (проверено на 1.90) и Microsoft C++ Build Tools.

```bash
git clone https://github.com/felixsmtnk/Vaporust.git
cd Vaporust
cargo build --release
```

Готовая программа: `target\release\vaporust.exe`.

**32-битная версия** дополнительно требует [NASM](https://www.nasm.us/) в `PATH` (для библиотеки
шифрования aws-lc) и стандарт C11:

```bash
rustup target add i686-pc-windows-msvc
set AWS_LC_SYS_C_STD=11
set CFLAGS_i686_pc_windows_msvc=/std:c11
cargo build --release --target i686-pc-windows-msvc
```

Обе версии собирает GitHub Actions ([`.github/workflows/build.yml`](.github/workflows/build.yml));
для меток `v*` архивы прикладываются к черновику релиза.

---

## Как пользоваться

1. **Вход.** При первом запуске войдите в аккаунт Steam: логин и пароль (плюс код Steam Guard)
   или QR-код из мобильного приложения Steam. Дальше вход запоминается.
2. **Играть.** Нажмите «Играть». Steam откроется незаметно, игра запустится, а после выхода
   из неё Steam закроется сам. Если игре нужно обновление, Steam сначала докачает его,
   прогресс виден внизу окна.
3. **Установить.** Снимите галочку «Только установленные» и нажмите «Установить» у нужной игры.
   Если файлы игры уже лежат в `steamapps\common`, на странице игры есть «Уже скачана…».
4. **Обновить.** У игр с доступным обновлением есть жёлтая кнопка «Обновить».
5. **Свойства игры** — кнопка ⚙: параметры запуска, запуск без Steam, язык, версии и беты
   (откат и закрепление версии), файлы, DLC, облако, Мастерская. Менять их можно, когда Steam
   закрыт: при выходе он перезаписывает свои файлы.
6. **Вкладки вверху:** «Библиотека», «Магазин» (поиск, скидки, события, список желаемого),
   «Инвентарь» (предметы, кошелёк, торговая площадка), «Настройки». С зажатым Ctrl магазин и
   инвентарь открываются отдельным окном.
7. **Друзья** — кнопка справа вверху: статусы, чат (отдельное окно), профили, заявки, «Добавить
   друга», звук сообщений. «Аккаунт» слева вверху — свой профиль и выход.
8. **Трей.** По умолчанию крестик прячет Vaporust к часам, чат продолжает работать. Вернуть —
   щелчок по иконке; правый щелчок — галочка «Крестик сворачивает в трей» и «Выход».
9. **Очередь загрузок.** Пока одна игра качается, «Установить» у другой ставит её в очередь;
   у каждой игры в очереди есть «пауза» и «продолжить».
10. **Коллекции** — правый щелчок по игре в библиотеке: Избранное, Скрытые или своя коллекция.
11. **Мастерская** — кнопка «Мастерская» на странице игры: поиск модов и подписка.
12. **Обновления Vaporust** — полоса «Доступна новая версия» сверху; «Обновить» скачивает и
    ставит её, потом «Перезапустить».
---

## Как это устроено

- **Запуск игр** — через настоящий Steam с флагами `-silent -no-browser` (меняются в «Настройках»).
  Vaporust следит за процессами Steam и игры, а после игры закрывает Steam. Игры без защиты
  можно запускать напрямую: что запускать, Vaporust берёт из таблицы запуска Steam и запоминает.
- **Закрепление версии.** Steam обновляет игру, если версии её частей (хранилищ) в файле игры
  не последние. Для закреплённой игры Vaporust хранит настоящие версии у себя, а в файл игры
  перед запуском записывает последние — Steam считает игру обновлённой. Открепление всё возвращает.
- **Работа с сетью Steam** (вход, список игр, загрузка, достижения, друзья, чат) идёт через
  библиотеку [Aurelia](https://github.com/Drackrath/Aurelia). Её копия лежит в каталоге
  `vendor/aurelia` (версия 0.2.3) с небольшими доработками; список изменений —
  [`vendor/aurelia/VAPORUST_CHANGES.md`](vendor/aurelia/VAPORUST_CHANGES.md).
- **Язык и бета** записываются в файл игры (appmanifest) так же, как это делает Steam,
  а нужные файлы докачивает сам Steam.
- **Параметры запуска** хранятся там же, где их хранит Steam (`localconfig.vdf`), поэтому
  они видны и в официальном клиенте.
- **Обновления Vaporust** берутся со страницы Releases на GitHub: архив скачивается, работающий
  `vaporust.exe` переименовывается в `vaporust.exe.old`, на его место встаёт новый.

---

## Вопросы и ответы

### Нужен ли официальный Steam?

Да, для большинства игр. Vaporust сознательно запускает игры через настоящий Steam, чтобы
работали достижения, облако, защита от копирования и всё остальное. Официальный клиент просто
открывается незаметно и только на время игры. Игры без защиты Steam можно запускать и напрямую
(галочка «Запускать без Steam» в свойствах игры) — тогда Steam не нужен вовсе, но нет
достижений, облака и учёта времени.

### Работает ли на 32-битной Windows?

Vaporust — да, есть отдельный архив `windows-x86`. Но **сам Steam с 1 января 2026 года не
поддерживает 32-битную Windows**: уже установленный клиент какое-то время работает, но не
обновляется. На такой Windows в Vaporust работают магазин, друзья и чат, инвентарь, установка игр
(через Aurelia) и **запуск игр без Steam** (для игр без защиты); запуск через Steam — пока
жив установленный клиент.

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
- [egui / eframe](https://github.com/emilk/egui): библиотека интерфейса; `egui_extras` — загрузка картинок в чате; [ehttp](https://github.com/emilk/ehttp) — загрузка обложек
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

- Games are **launched through the real Steam client**, started hidden and closed together
  with the game (steamwebhelper can be frozen while you play); DRM-free games can be launched
  directly without Steam.
- Library as a list or a grid with covers, Steam collections, a game page; Steam Workshop
  (search, subscribe); friend profiles and friend requests.
- Install and update games, game properties (launch options, language, betas, file
  verification, moving, uninstalling, DLC, cloud saves), achievements, and friends & chat
  with emoticons, animated stickers and images.
- Inventory, wallet and Community Market browsing; store events; version rollback and pinning;
  manual Steam Cloud sync; self-update from GitHub Releases; portable mode; 64- and 32-bit builds.
- Steam networking is provided by a lightly modified copy of
  **[Aurelia](https://github.com/Drackrath/Aurelia)** by Drackrath — huge thanks; this project
  is inspired by and built on it (changes are listed in `vendor/aurelia/VAPORUST_CHANGES.md`).
- Based on Aurelia, but with a friendly windowed interface, its own features, and made for Windows.
- **Status: alpha (0.2.0-alpha).** The interface is in Russian for now; English is planned for the beta.

**Disclaimer:** an independent, unofficial project, **not affiliated with, endorsed or
sponsored by Valve Corporation or Steam**. Steam and Valve are trademarks of Valve
Corporation. The software modifies Steam's files and is provided **"as is", without any
warranty**; the authors are **not liable** for any consequences of its use, including data
loss, broken installations or account restrictions. Use at your own risk.

License: [GPL-3.0-or-later](LICENSE).
