# Mods

Native extensions: compiled libraries the server loads at startup.

There are **two** extension points and they are for different things.

|                | scripted plugin (`plugins/`) | native mod (here) |
|----------------|------------------------------|-------------------|
| language       | [Rhai], a small embedded one | anything with a C ABI -- Rust, C, C++, Zig |
| build step     | none                         | a toolchain |
| blast radius   | a log line                   | the whole process |
| speed          | interpreted, operation-capped | native, uncapped |
| reaches        | events and a dozen effects   | 181 host calls across 20 API tables |

A plugin is the right answer for most of what people want: drop a folder
in, no build, and a mistake is a line in the log rather than a crash. A
mod is for the things a plugin cannot be -- a world generator, a mob AI,
anything doing work per block or per tick.

If you are not sure which you want, you want a plugin. See
`plugins/README.md`.

## What a mod looks like

```text
mods/
  greeter/
    mod.ron        <- the manifest: name, version, deps, settings
    greeter.dll    <- windows
    libgreeter.so  <- linux
    libgreeter.dylib <- macos
```

The library exports exactly one symbol, `primitive_mod_register`, and
`primitive_modapi::declare_mod!` writes it for you.

## Четыре мода в комплекте

| | |
|---|---|
| `greeter/` | **рабочий пример.** Всё, что автор мода делает хотя бы раз, сделано там по разу и с объяснением рядом: подписка, чтение настройки из манифеста, состояние, переживающее рестарт, отмена действия, ответ на команду. |
| `flight/` | **`/fly`.** Первое, что пользуется возможностью, которую API отрастил ради него — `PlayersApi::set_flying`, добавленной в 1.1. |
| `sandbox/` | **`/sandbox`.** Диагностика: зовёт каждую функцию каждой таблицы и считает каждое событие. |
| `dypt/` | **`/dypt`.** Язык программирования внутри мира — и **точка расширения поверх точки расширения**: моды на dypt пишутся без сборки, файлом в `dypt/mods/`, и перечитываются по `/dypt reload`. Ему же принадлежит единственная в этом каталоге чужая библиотека, и подключена она **не** зависимостью по пути; почему так, написано в `dypt/README.md`. |

Второй нужен не меньше первого. Точка расширения, у которой единственный
пользователь — демонстрация, — это точка расширения, на которой никто
ничего не пробовал построить.

А третий нужен не меньше второго, и вот почему он появился. До 2.0 API
объявлял двадцать одно событие, а сервер поднимал двенадцать; принимал
декоратор чанков, записывал его и не вызывал ни разу; обещал в
`EventData::args` аргументы команды и всегда клал туда пустоту. Всё это
компилировалось, и всё это было покрыто тестами — со стороны хоста.
Не хватало ровно одного: кого-то по ту сторону границы, кто скажет
«я подписался, и меня не позвали». Теперь это говорит `/sandbox`.

`flight/` заодно показывает, где проходит граница. Полёт **нельзя**
сделать чистым модом на этом сервере: клиент считает гравитацию у себя
(иначе каждый шаг стоил бы круга по сети), и ничто со стороны сервера не
мешает игроку падать. Очевидный обход — телепортировать его вверх каждый
тик — ломается сразу тремя способами, и все три описаны в шапке
`flight/src/lib.rs`. Поэтому движок отрастил возможность, а мод владеет
политикой: *может ли клиент отключить гравитацию* решает движок,
*кому, как быстро и надолго ли* — мод.

## Building the example

`greeter/` is a complete, working mod and is part of the workspace, so a
change to the API that breaks a mod breaks the build rather than
somebody's server.

```text
cargo build -p greeter --release
copy target\release\greeter.dll mods\greeter\
```

(on Linux: `cp target/release/libgreeter.so mods/greeter/`)

Then start the server and look for:

```text
[mods] loaded mod 'greeter' 1.0.0 (API 2.0)
[mods] 1 active
```

`/mods` lists what is loaded, plugins and mods together.

То же самое для полёта:

```text
cargo build -p flight --release
copy target\release\flight.dll mods\flight\
```

...и для языка, которому нужна ещё и вторая библиотека, собранная в
репозитории самого dypt:

```text
cargo build -p dypt --release
copy target\release\dypt.dll mods\dypt\
```

...и для песочницы:

```text
cargo build -p sandbox --release
copy target\release\sandbox.dll mods\sandbox\
```

**Мод поставляется выключенным** (`enabled: false` в
`sandbox/mod.ron`), и это не осторожность, а та же причина, по которой
подписка вообще существует: он подписан на *все* события — иначе он не
скажет, какие не приходят, — а мод, подписанный на всё, это диспатч на
каждый тик, на каждую ячейку, которую сдвинула вода, и на каждую вещь,
которую кто-то поднял. Включили, спросили `/sandbox`, выключили.

`/sandbox` печатает, что ответила каждая функция каждой таблицы;
`/sandbox events` — сколько событий каждого рода пришло, **нулями
вперёд**, потому что ноль здесь и есть новость. `/sandbox write` делает
то же самое с вызовами, которые меняют мир: он выключен по умолчанию
(`allow_writes` в `sandbox/mod.ron`), работает в черновой колонке в трёх
метрах от того, кто его позвал, и убирает за собой. Диагностика не
должна быть катастрофой.

```text
[mods] loaded mod 'flight' 1.0.0 (API 2.0)
[mod] flight ready against API 2.0: 12 b/s, operators only, 0 remembered
```

По умолчанию `/fly` доступен только операторам — см. `operators_only` в
`flight/mod.ron`. Это безопасное значение, а не дружелюбное: мод,
раздающий полёт всем на публичном сервере, изменил игру для всех, кто его
не ставил.

## Writing one

Start from `greeter/src/lib.rs`: it does everything a mod ever needs to
do at least once -- holding the host table, subscribing to events,
reading a setting out of its own manifest, keeping state across a
restart, cancelling an action, and answering a command -- with the reason
for each written beside it.

The contract itself is `primitive_modapi`. Its module documentation is
the reference; the short version:

* **Everything crosses as plain data.** No `String`, no `Vec`, no
  `Result` -- those have no stable layout across compiler versions.
  Text is `Str` (a pointer and a length) and is borrowed **for the
  length of one call**. Copy anything you want to keep.
* **Check the version.** The host checks yours and refuses a mismatch;
  check the host's too, because a mod loaded by an *older* host would
  otherwise read past the end of a table before it got the chance to
  complain.
* **Every out-parameter struct is frozen.** A mod allocates a
  `BlockProperties` or a `PlayerVitals` on its own stack and hands the
  host a pointer to it, so growing one of those would have the host
  write past the end of a buffer sized by an older mod -- a stack smash
  no version check catches, because the check passed. New columns arrive
  as new structs; see `BlockTooling`.
* **Operations, not structures.** There is no call that hands out the
  layout of a chunk or the queue the water simulation is working
  through, and there will not be: an operation survives the inside being
  rewritten and a structure freezes it. Where that rule refused
  something, the refusal is written down at the site -- see the note at
  the top of `primitive_modapi`.
* **A null table means "not in this process".** `render`, `audio` and
  `ui` are null on a dedicated server. Check them.
* **Subscribe in `on_load`, not in the entry point.** At the entry point
  the other mods do not exist yet.

## Turning them off

The server reads `mod_dir` out of `settings.toml`; setting it to an empty
string disables the loader entirely. A single mod can be switched off
with `enabled: false` in its own `mod.ron`, which is an off switch that
does not involve deleting anything.

**Singleplayer loads mods too.** It did not until 1.5: the client
embedded the server with `default-features = false` and a local world
carried no loader at all. What changed the answer was noticing that the
argument for it -- "a local world has no operator to install mods for"
-- was wrong about who installs a mod. It is the person playing, on
their own machine, into a folder they can see.

So the client asks the `mods` feature back by name (and only the `mods`
one: it still links no scripting engine), the folder is looked for
beside the executable the way `assets/` is, and **the only player in a
local world is an operator** -- without that, every mod that sensibly
defaults to operator-only would refuse the one person there is. See
`RunOptions::local_operator`.

Android is the exception, and deliberately: a mod is a `.so` a player
drops into a folder, and an APK has no such folder an ordinary person
can reach.

[Rhai]: https://rhai.rs
