<p align="center"><img src="assets/logo.png" width="128" alt="Vaporust"></p>

# Vaporust: легкий клиент Steam для Windows

**Быстрый и легкий лаунчер и менеджер библиотеки Steam на Rust + egui. Устанавливайте,
обновляйте и запускайте игры Steam без тяжелого официального интерфейса.**

Vaporust основан на [Aurelia](https://github.com/Drackrath/Aurelia), но с удобным оконным
интерфейсом, своими возможностями и сделан под Windows.

[![License: GPL-3.0-or-later](https://img.shields.io/badge/License-GPL--3.0--or--later-blue.svg)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-orange.svg)](https://www.rust-lang.org/)
[![Status: early alpha](https://img.shields.io/badge/status-early%20alpha-red.svg)](#состояние-проекта)
[![Platform: Windows](https://img.shields.io/badge/platform-Windows%2010%2B-0078d6.svg)](#установка)

Название - от *vapor* (пар) + *Rust*. [English summary below](#english).

> [!WARNING]
> **Отказ от ответственности. Прочитайте перед использованием.**
>
> - Vaporust - **независимый неофициальный проект**. Он **не связан с Valve Corporation
>   и Steam**, не одобрен, не спонсируется и не поддерживается ими.
> - «Steam», «Valve», логотипы Steam и названия игр - товарные знаки и собственность
>   Valve Corporation и соответствующих правообладателей. Все права на Steam и его сервисы
>   принадлежат Valve.
> - Программа **напрямую меняет файлы Steam** (настройки запуска, файлы игр) и работает
>   с сетью Steam не через официальный клиент. Это может повредить установку Steam
>   или игры, а Valve может ограничить аккаунт за использование неофициальных программ.
> - Программа предоставляется **«как есть», без каких-либо гарантий**. Авторы
>   **не несут ответственности** за любые последствия ее использования: потерю данных,
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

- **Привычно, как в Steam.** Те же разделы МАГАЗИН, БИБЛИОТЕКА, СООБЩЕСТВО и профиль, список игр
  слева, страница игры с большой кнопкой «Играть», профиль, друзья и чат - но без тяжелого
  встроенного браузера.
- **Без встроенного браузера.** Интерфейс нарисован на [egui](https://github.com/emilk/egui):
  никаких Electron, CEF и steamwebhelper. Сам лаунчер занимает около 30-80 МБ памяти и в покое
  почти не нагружает процессор (в «Настройки» -> «Производительность» можно отключить анимации
  и обложки), тогда как у официального клиента только steamwebhelper легко
  занимает сотни мегабайт.
- **Игры запускаются через настоящий Steam.** Никаких эмуляторов steam_api: Steam поднимается
  незаметно, в урезанном режиме, только на время игры и закрывается вместе с ней. Достижения,
  облачные сохранения, оверлей и защита от копирования работают как обычно.
- **Для слабых компьютеров.** Делался для старого ноутбука, на котором официальный клиент
  тормозит. Пока идет игра или обновление, тяжелый steamwebhelper можно «заморозить»:
  он перестает занимать оперативную память.
- **Все нужное под рукой.** Установка и обновление игр, свойства игры как в Steam, достижения,
  магазин со скидками и списком желаемого, друзья и чат со смайликами, стикерами и картинками.
- **Открытый код.** Лицензия GPL-3.0-or-later.

### Сравнение

| | Официальный Steam | [Aurelia](https://github.com/Drackrath/Aurelia) | **Vaporust** |
|---|---|---|---|
| **Интерфейс** | Окна (встроенный браузер CEF) | Командная строка | Окна (egui, без браузера) |
| **Платформы** | Windows, Linux, macOS | Linux (в первую очередь), Windows | Windows 10/11, 64 и 32 бит |
| **Запуск игр** | Сам Steam | Напрямую, через Proton/Wine или Steam | Через скрытый Steam; игры без защиты - напрямую, без Steam |
| **Установка и обновление** | Да | Да | Да, без запуска Steam (через Aurelia); откат и закрепление версии |
| **Друзья и чат** | Да | Да (текст) | Да (смайлики, стикеры, картинки, эффекты) |
| **Профиль и сообщество** | Да | Частично | Да: профиль, витрины, комментарии, обсуждения, Мастерская |
| **Память** | Сотни МБ (steamwebhelper) | Мало (консоль) | Около 30-80 МБ |
| **Открытый код** | Нет | Да (GPL-3.0) | Да (GPL-3.0) |

---

## Состояние проекта

Vaporust в **альфе** (последняя версия 0.5.0-alpha): основное работает и проверено вручную
на двух компьютерах с Windows 10 (64 бит), включая старый ноутбук, но ошибки вполне вероятны.

| Раздел | Статус | Что работает |
|---|---|---|
| **Вход** | ✅ | Пароль + Steam Guard (код из почты или приложения), вход по QR-коду, восстановление сессии |
| **Библиотека** | ✅ | Купленные и установленные игры, **семейная библиотека Steam**, время в игре, поиск, сортировка, коллекции Steam, вид как в Steam: список игр слева, обложки плиткой или компактная таблица, страница игры с фоном, достижениями и новостями; **сторонние игры** (не из Steam); «Уже скачана…»; поиск дублей установок |
| **Запуск** | ✅ | Через скрытый Steam (закрывается после игры, steamwebhelper замораживается); игры без защиты можно запускать без Steam |
| **Установка** | ✅ | Прогресс, скорость, проверка места, докачка, выбор папки, очередь загрузок с паузой, ограничение скорости, **расписание загрузок** |
| **Обновления игр** | ✅ | **Без запуска Steam** (или через Steam, по выбору в настройках); откат на старую версию и закрепление версии |
| **Свойства игры** | ✅ | Параметры запуска, **смена языка и беты без Steam**, описания веток, проверка файлов, перенос, удаление, DLC, Мастерская, облачные сохранения вручную |
| **Друзья и чат** | ✅ | Аватары, статусы и игры друзей, страница «Друзья» с заявками, чат с группами сообщений и поиском (Ctrl+F), история, смайлики, анимированные стикеры, картинки, **эффекты комнаты**, свой статус, профили, заявки, список желаемого друга |
| **Уведомления** | ✅ | Свои всплывающие уведомления: сообщения, друг в игре или в сети, скидки из списка желаемого, игра скачана; у каждого вида свой звук |
| **Профиль и сообщество** | ✅ | Профиль как в Steam (фон, витрины, недавние игры, значки, друзья с уровнями, комментарии по страницам), активность друзей, групповые чаты, обсуждения и руководства, страница мода Мастерской, значки и карточки, очки Steam |
| **Скриншоты** | ✅ | На этом ПК и в облаке Steam, по играм, скачивание на компьютер |
| **Магазин** | ✅ | Поиск, скидки и акции Steam, список желаемого, сортировка и фильтры, страница игры с **отзывами** и **системными требованиями**, регион цен; покупка на сайте Steam |
| **Инвентарь и рынок** | ✅ | Инвентарь по играм с ценами, кошелек, поиск по торговой площадке, свои лоты и заявки; продажа на сайте Steam |
| **Программа** | ✅ | Вид и цвета как у клиента Steam, «Настройки» (в том числе «Производительность»: анимации, обложки, экономия памяти), трей, автообновление, прокси, 32-битная сборка |
| **Закрытые беты с паролем** | ⏸ | Пока включаются в самом Steam |

Планы по версиям - в [ROADMAP.md](ROADMAP.md).

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
2. **Разделы сверху**, как в Steam: **МАГАЗИН**, **БИБЛИОТЕКА**, **СООБЩЕСТВО** и ваше имя
   (профиль). Под ними - подразделы выбранного раздела. Слева от них меню **Vaporust**:
   «Настройки», выход из аккаунта, закрытие программы. Справа - колокольчик уведомлений Steam.
3. **Библиотека.** Слева список игр с поиском, коллекцией, сортировкой и «Только установленные»;
   справа «Главная» - обложки плиткой или компактная таблица (переключатель «Плитка / Таблица»).
   Щелчок по игре открывает ее страницу, двойной щелчок - запуск, правый щелчок - меню игры
   (коллекции, свойства, папка, скриншоты, сообщество).
4. **Играть.** Нажмите «ИГРАТЬ». Steam откроется незаметно, игра запустится, а после выхода
   из нее Steam закроется сам.
5. **Установить и обновить.** Снимите «Только установленные» и нажмите «УСТАНОВИТЬ» у нужной игры.
   У игр с обновлением есть кнопка «ОБНОВИТЬ»: Vaporust скачает его сам, без запуска Steam.
   Если файлы игры уже лежат в `steamapps\common`, на странице игры есть «Уже скачана…».
6. **Загрузки** - посередине нижней полосы: ход текущей загрузки, по щелчку - страница с очередью
   (поднять выше, пауза, убрать). Расписание и ограничение скорости - в «Настройки» -> «Загрузки».
7. **Свойства игры** - шестеренка на странице игры: параметры запуска, запуск без Steam, язык,
   версии и беты (откат и закрепление версии), файлы, DLC, облако, Мастерская. Менять их можно,
   когда Steam закрыт.
8. **+ Добавить игру** - слева внизу: сторонняя игра (не из Steam, запускается напрямую)
   или активация ключа Steam.
9. **Друзья и чат** - кнопка справа внизу. Сверху панели - ваш аватар и статус (щелчок по статусу
   меняет его), меню из трех полосок: добавить друга, заявки, активность, групповые чаты.
   Щелчок по другу открывает чат (отдельное окно; поиск по переписке - лупа или Ctrl+F).
10. **Профиль** - ваше имя сверху: фон, витрины, недавние игры, значки, друзья, комментарии.
    Там же «Друзья» (заявки), «Инвентарь», «Значки и карточки», «Семья Steam».
11. **Магазин** - поиск, скидки и акции, чарты, рекомендации, список желаемого, новости ваших
    игр, магазин очков. Покупка - на сайте Steam.
12. **Сообщество** - активность друзей, групповые чаты; со страницы игры - обсуждения,
    руководства и Мастерская (страница мода открывается прямо в программе).
13. **Трей.** По умолчанию крестик прячет Vaporust к часам, чат продолжает работать. Вернуть -
    щелчок по иконке; правый щелчок - «Крестик сворачивает в трей» и «Выход».
14. **Уведомления** всплывают в правом нижнем углу экрана: сообщения, друг в игре, скидки,
    игра скачана. Настраиваются в «Настройки» -> «Уведомления».
15. **Слабый компьютер?** «Настройки» -> «Производительность»: отключить анимации и обложки,
    режим экономии памяти, отдавать память системе во время игры.
16. **Обновления Vaporust** - полоса «Доступна новая версия» сверху; «Обновить» скачивает и
    ставит ее, потом «Перезапустить».

---

## Как это устроено

- **Запуск игр** - через настоящий Steam с флагами `-silent -no-browser` (меняются в «Настройках»).
  Vaporust следит за процессами Steam и игры, а после игры закрывает Steam. Игры без защиты
  можно запускать напрямую: что запускать, Vaporust берет из таблицы запуска Steam и запоминает.
- **Закрепление версии.** Steam обновляет игру, если версии ее частей (хранилищ) в файле игры
  не последние. Для закрепленной игры Vaporust хранит настоящие версии у себя, а в файл игры
  перед запуском записывает последние - Steam считает игру обновленной. Открепление все возвращает.
- **Работа с сетью Steam** (вход, список игр, загрузка, достижения, друзья, чат) идет через
  библиотеку [Aurelia](https://github.com/Drackrath/Aurelia). Ее копия лежит в каталоге
  `vendor/aurelia` (версия 0.2.5) с небольшими доработками; список изменений -
  [`vendor/aurelia/VAPORUST_CHANGES.md`](vendor/aurelia/VAPORUST_CHANGES.md).
- **Обновления, смена языка и беты.** Нужные файлы скачивает Vaporust (через Aurelia), а в файл
  игры (appmanifest) вписываются только новые версии частей игры и выбранный язык или бета.
  Остальные записи Steam в этом файле остаются как были, поэтому Steam считает игру обновленной.
  При смене языка файлы старого языка удаляются, как это делает Steam.
- **Параметры запуска** хранятся там же, где их хранит Steam (`localconfig.vdf`), поэтому
  они видны и в официальном клиенте.
- **Обновления Vaporust** берутся со страницы Releases на GitHub: архив скачивается, работающий
  `vaporust.exe` переименовывается в `vaporust.exe.old`, на его место встает новый.

---

## Вопросы и ответы

### Нужен ли официальный Steam?

Да, для большинства игр. Vaporust сознательно запускает игры через настоящий Steam, чтобы
работали достижения, облако, защита от копирования и все остальное. Официальный клиент просто
открывается незаметно и только на время игры. Игры без защиты Steam можно запускать и напрямую
(галочка «Запускать без Steam» в свойствах игры) - тогда Steam не нужен вовсе, но нет
достижений, облака и учета времени.

### Работает ли на 32-битной Windows?

Vaporust - да, есть отдельный архив `windows-x86`. Но **сам Steam с 1 января 2026 года не
поддерживает 32-битную Windows**: уже установленный клиент какое-то время работает, но не
обновляется. На такой Windows в Vaporust работают магазин, друзья и чат, инвентарь, установка игр
(через Aurelia) и **запуск игр без Steam** (для игр без защиты); запуск через Steam - пока
жив установленный клиент.

### Чем это отличается от Aurelia?

[Aurelia](https://github.com/Drackrath/Aurelia) - консольный клиент Steam (в первую очередь
для Linux), который умеет запускать игры и без официального Steam. Vaporust - оконный лаунчер
для Windows, построенный на Aurelia: он берет у нее работу с сетью Steam, а игры всегда
запускает через официальный клиент.

### Это безопасно? Могут ли забанить?

Vaporust неофициальный, не связан с Valve и напрямую меняет файлы Steam. Сторонние программы,
работающие со Steam, несут риск санкций против аккаунта. Прочитайте отказ от ответственности
в начале этого файла. Используйте на свой страх и риск.

### Работает ли на Linux или macOS?

Нет, Vaporust сделан под Windows. На Linux попробуйте [Aurelia](https://github.com/Drackrath/Aurelia).

### Сколько памяти занимает?

Сам лаунчер - около 30-80 МБ (в Диспетчере задач), в зависимости от компьютера и открытых разделов.
На ПК с видеокартой NVIDIA около 34 МБ из этого занимает сам драйвер видеокарты, на старом
ноутбуке с графикой Intel вся программа укладывается примерно в 30 МБ. В покое Vaporust почти
не нагружает процессор, а в трее и во время игры отдает свою память системе («Настройки» ->
«Производительность»).
Пока идет игра, к этому добавляется Steam; его steamwebhelper можно заморозить («Настройки» →
«Запуск игр»).

---

## Благодарности

Vaporust вдохновлен **[Aurelia](https://github.com/Drackrath/Aurelia)** и построен на ее коде.
Вход в Steam, загрузка игр, достижения, друзья, чат и многое другое - это работа автора Aurelia,
**Drackrath**. Огромное спасибо: без Aurelia этого проекта бы не было.

Aurelia, в свою очередь, выросла из **[SteamFlow](https://github.com/weter11/SteamFlow)**
и стоит на **[steam-vent](https://codeberg.org/steam-vent/steam-vent)**, где проделана
огромная работа по разбору сетевого протокола Steam.

Спасибо и проектам-вдохновителям: **[OpenSteamClient](https://github.com/OpenSteamClient/OpenSteamClient)**
(Rosentti и участники) показал, что открытый клиент Steam - реальная задача.

### Использованные проекты

- [Aurelia](https://github.com/Drackrath/Aurelia): консольный клиент Steam на Rust, основа Vaporust (входит в проект измененной копией). Спасибо!
- [SteamFlow](https://github.com/weter11/SteamFlow): проект, из которого выросла Aurelia
- [steam-vent](https://codeberg.org/steam-vent/steam-vent): реализация сетевого протокола Steam
- [steam-vent-chat](https://codeberg.org/steam-vent/chat): протокол чата Steam (в Aurelia - измененный форк [aurelia-steam-vent-chat](https://codeberg.org/Drackrath/aurelia-steam-vent-chat))
- [steam-cdn](https://github.com/OccamyLab/steam-cdn): загрузка содержимого игр (в Aurelia - форк [aurelia-steam-cdn](https://github.com/Drackrath/aurelia-steam-cdn))
- [egui / eframe](https://github.com/emilk/egui): библиотека интерфейса; `egui_extras` - загрузка картинок в чате; [ehttp](https://github.com/emilk/ehttp) - загрузка обложек
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
которой он основан. История лицензии Aurelia - в файле [`vendor/aurelia/LICENSE`](vendor/aurelia/LICENSE).

---

## English

**Vaporust** (*vapor* + *Rust*) is a lightweight Steam client and launcher for Windows, written
in Rust with an [egui](https://github.com/emilk/egui) interface - no embedded browser. It is
built for low-end PCs where the official client's steamwebhelper is too heavy.

- Games are **launched through the real Steam client**, started hidden and closed together
  with the game (steamwebhelper can be frozen while you play); DRM-free games can be launched
  directly without Steam.
- **Looks and feels like the Steam client**: STORE, LIBRARY, COMMUNITY and profile sections, a game
  list on the left, covers grid or a compact table, a game page with a big PLAY button, Steam
  Workshop with in-app mod pages, non-Steam games.
- Install and update games, and switch language or beta, without starting Steam; game properties
  (launch options, file verification, moving, uninstalling, DLC, cloud saves), achievements.
- Steam Family Sharing library; download scheduling; screenshots (local and Steam Cloud).
- Friends & chat with avatars, emoticons, animated stickers, images and Steam room effects;
  a Steam-like profile page (showcases, recent games, badges, friends, comments), friend requests,
  activity feed, group chats, discussions and guides; pop-up notifications with avatars.
- Store with reviews and system requirements; inventory, wallet and Community Market browsing;
  version rollback and pinning; self-update from GitHub Releases; 64- and 32-bit builds.
- Light on resources: about 30-80 MB of RAM and near-zero CPU when idle; a Performance section
  turns off animations and covers for low-end PCs.
- Steam networking is provided by a lightly modified copy of
  **[Aurelia](https://github.com/Drackrath/Aurelia)** by Drackrath - huge thanks; this project
  is inspired by and built on it (changes are listed in `vendor/aurelia/VAPORUST_CHANGES.md`).
- Based on Aurelia, but with a friendly windowed interface, its own features, and made for Windows.
- **Status: alpha (0.5.0-alpha).** The interface is in Russian for now; English is planned for the beta.

**Disclaimer:** an independent, unofficial project, **not affiliated with, endorsed or
sponsored by Valve Corporation or Steam**. Steam and Valve are trademarks of Valve
Corporation. The software modifies Steam's files and is provided **"as is", without any
warranty**; the authors are **not liable** for any consequences of its use, including data
loss, broken installations or account restrictions. Use at your own risk.

License: [GPL-3.0-or-later](LICENSE).
