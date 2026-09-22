//! Язык программирования внутри мира.
//!
//! ## Что это
//!
//! Мод, который даёт набрать в чате `/dypt дом.dpt` и увидеть, как в
//! мире вырастает дом. Скрипты пишутся на [dypt] — языке со своей
//! виртуальной машиной, классами, разбором по образцу и байткодом; мод
//! принимает все три вида его входа, `.dpt`, `.dyc` и `.dasm`, потому
//! что их принимает сам язык.
//!
//! Скрипту доступна игра целиком: около сотни функций поверх всех
//! двадцати таблиц API — блоки, игроки, животные, вещи, свет, вода,
//! огонь, печи, рецепты, — и `command()` там, где отдельной функции
//! нет. Список — в [`VOCABULARY`], описание каждой — в README рядом.
//!
//! ## Три границы, а не одна
//!
//! ```text
//! сервер игры ──C ABI──> dypt.dll (этот мод) ──C ABI──> dypt_embed.dll
//!  primitive_modapi                                      сам язык
//! ```
//!
//! Вторая граница выглядит лишней ровно до вопроса «а где живёт dypt».
//! Он живёт в своём репозитории и к игре отношения не имеет. Подключить
//! его сюда зависимостью по пути значит записать в сборку игры
//! абсолютный путь к чужому каталогу: на любой другой машине
//! `cargo build --workspace` после этого не проходит. Поэтому язык
//! собирается у себя в `dypt_embed.dll`, а мод открывает её по имени
//! при загрузке — и если её нет, говорит об этом одной строкой и живёт
//! дальше выключенным, вместо того чтобы уронить сервер.
//!
//! Договор второй границы описан в `embed/src/lib.rs` репозитория dypt
//! и продублирован здесь ([`DyValue`], [`DyHost`], [`dypt_run`]) —
//! дублирование намеренное и того же рода, что и любой заголовочный
//! файл C: две стороны компилируются порознь и обязаны совпадать
//! побайтово. Совпадение проверяется числом: [`DY_ABI`] у обеих сторон
//! должно быть одним, и мод спрашивает чужое первым делом.
//!
//! ## Почему скрипт живёт на своём потоке
//!
//! Обработчик события мода вызывается из тактового цикла сервера.
//! Скрипт, посчитанный там же, это такт, который не закончился, — а
//! скрипт пишет человек в чате, и его первый цикл будет бесконечным.
//! Поэтому `/dypt` только запускает поток и сразу возвращается, а
//! дальше скрипт зовёт игру уже оттуда.
//!
//! Так можно: таблицы хоста устроены как `Arc<Context>` с замками
//! внутри, и сервер сам зовёт мод из потока генератора чанков. Нельзя
//! — только то, что опирается на «кто сейчас на стеке»: настройки,
//! подписка на события и собственный блок сохранения читаются
//! потоко-локальной переменной хоста и с чужого потока ответят
//! пустотой. Поэтому всё это читается один раз в [`load`], до того как
//! появится первый скрипт.
//!
//! ## Два рода скриптов
//!
//! **Разовый** — `/dypt башня.dpt`: запустился, отработал, ушёл.
//! Выполняется байткодовой VM, принимает `.dpt`, `.dyc` и `.dasm`.
//!
//! **Скриптовый мод** — файл в `mods/dypt/mods/`, который загружается
//! при старте сервера и **остаётся жить**. Он объявляет функции с
//! оговорёнными именами — `on_break`, `on_chat`, `on_tick` — и игра
//! зовёт их, когда это происходит. Из тех, что игра называет
//! отменяемыми, `false` означает «не давать»: скрипт из семи строк
//! запрещает ломать бедрок ровно так же, как это делает мод на Rust.
//!
//! Ради этого здесь всё остальное. Мод на Rust надо скомпилировать,
//! положить рядом с манифестом и перезапустить сервер; скриптовый —
//! сохранить в файл и набрать `/dypt reload`. Разница не в удобстве, а
//! в том, сколько человек успевает попробовать за вечер.
//!
//! ## Как события доходят до скрипта
//!
//! Окружение языка держится на `Rc` и не переносится между потоками,
//! поэтому **все живые скрипты принадлежат одному потоку** — тому, что
//! заведён в [`start_worker`]. Обработчик события мода вызывается
//! сервером из тактового цикла, кладёт вызов в очередь этого потока и
//! **ждёт ответа**: у отменяемого события ответ нужен до того, как
//! действие состоится, и никакого «потом перезвоню» тут быть не может.
//!
//! Из ожидания следуют три правила, и каждое из них — дыра, если его
//! не соблюсти.
//!
//! 1. **Ожидание со сроком.** Обработчик, ушедший в бесконечный цикл,
//!    иначе останавливает такт навсегда. По истечении `handler_ms`
//!    доставка выключается целиком и об этом пишется в журнал: поток
//!    скриптов уже не вернётся, и продолжать стучаться в него — значит
//!    класть по секунде на каждое событие в мире.
//! 2. **События, вызванные самим скриптом, скриптам не доходят.**
//!    Обработчик, поставивший блок, поднимает `BlockChanged`, который
//!    пришёл бы в тот же поток, который его и ждёт, — это заклинивание
//!    на первом же `place()`. Проверяется по идентификатору потока.
//!    Заодно это отсекает `on_changed`, который ставит блок и потому
//!    зовётся снова.
//! 3. **Подписка только на то, у чего есть обработчик.** Скрипт без
//!    `on_changed` не должен стоить перехода через границу на каждую
//!    ячейку, которую сдвинула вода.
//!
//! ## Чего скрипт не может
//!
//! **Его нельзя прервать посреди счёта.** Ни в VM, ни в
//! AST-интерпретаторе нет ни счётчика операций, ни точки, где они
//! спросили бы разрешения продолжать. `/dypt stop`, `limit_seconds` и
//! `handler_ms` работают иначе: мод начинает отвечать отказом на вызовы
//! **в игру**, и скрипт падает на первом же. Цикл, который не зовёт
//! игру и ничего не печатает, так не остановить — он доработает сам или
//! не доработает никогда, заняв один поток. Это записано и здесь, и в
//! README, потому что чинится это только со стороны языка.
//!
//! [dypt]: https://github.com/  (репозиторий языка; собирается отдельно)

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use primitive_modapi::{
    ApiVersion, BlockPos, BlockProperties, BlockTooling, BodySlot, ChunkPos, Event, EventData,
    Feasibility, HookResult, HostApi, ItemStack, LogLevel, PlayerId, PlayerVitals, RecipeInfo,
    SpeciesInfo, Status, Str, Vec3, Weather, API_VERSION,
};

// =====================================================================
// Договор с dypt_embed.dll. Копия того, что объявлено на той стороне.
// =====================================================================

/// Версия договора. Обе стороны обязаны назвать одно и то же число.
///
/// 2 — появились `dypt_open`/`dypt_has`/`dypt_call`/`dypt_close`:
/// скрипт, который не уходит после первого запуска, а остаётся живым и
/// отвечает на события.
const DY_ABI: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
struct DyStr {
    ptr: *const u8,
    len: usize,
}

impl DyStr {
    const EMPTY: DyStr = DyStr {
        ptr: std::ptr::null(),
        len: 0,
    };

    fn of(s: &str) -> DyStr {
        DyStr {
            ptr: s.as_ptr(),
            len: s.len(),
        }
    }

    /// # Безопасность
    /// Указатель должен быть жив, а внутри вызова он жив.
    unsafe fn as_str<'a>(self) -> &'a str {
        if self.ptr.is_null() || self.len == 0 {
            return "";
        }
        std::str::from_utf8(std::slice::from_raw_parts(self.ptr, self.len)).unwrap_or("")
    }
}

const DY_NULL: u32 = 0;
const DY_BOOL: u32 = 1;
const DY_INT: u32 = 2;
const DY_FLOAT: u32 = 3;
const DY_STRING: u32 = 4;
const DY_LIST: u32 = 5;
const DY_MAP: u32 = 6;

#[repr(C)]
#[derive(Clone, Copy)]
struct DyValue {
    kind: u32,
    b: u32,
    i: i64,
    f: f64,
    s: DyStr,
    items: *const DyValue,
    count: usize,
}

impl DyValue {
    const NULL: DyValue = DyValue {
        kind: DY_NULL,
        b: 0,
        i: 0,
        f: 0.0,
        s: DyStr::EMPTY,
        items: std::ptr::null(),
        count: 0,
    };
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DyHost {
    user: *mut c_void,
    call: unsafe extern "C" fn(
        user: *mut c_void,
        name: DyStr,
        args: *const DyValue,
        argc: usize,
        out: *mut DyValue,
    ) -> i32,
    print: unsafe extern "C" fn(user: *mut c_void, text: DyStr),
}

const DY_INPUT_PATH: u32 = 0;
const DY_INPUT_SOURCE: u32 = 1;

type DyptAbiFn = unsafe extern "C" fn() -> u32;
type DyptVersionFn = unsafe extern "C" fn() -> DyStr;
#[allow(clippy::type_complexity)] // это подпись из заголовочного файла, а не наша
type DyptRunFn = unsafe extern "C" fn(
    kind: u32,
    input: DyStr,
    base_dir: DyStr,
    names: *const DyStr,
    name_count: usize,
    host: *const DyHost,
    err: *mut u8,
    err_cap: usize,
    err_len: *mut usize,
) -> i32;

#[allow(clippy::type_complexity)]
type DyptOpenFn = unsafe extern "C" fn(
    path: DyStr,
    names: *const DyStr,
    name_count: usize,
    host: *const DyHost,
    err: *mut u8,
    err_cap: usize,
    err_len: *mut usize,
) -> *mut c_void;
type DyptHasFn = unsafe extern "C" fn(script: *mut c_void, name: DyStr) -> i32;
#[allow(clippy::type_complexity)]
type DyptCallFn = unsafe extern "C" fn(
    script: *mut c_void,
    name: DyStr,
    args: *const DyValue,
    argc: usize,
    out: *mut DyValue,
    err: *mut u8,
    err_cap: usize,
    err_len: *mut usize,
) -> i32;
type DyptCloseFn = unsafe extern "C" fn(script: *mut c_void);

// =====================================================================
// Состояние мода
// =====================================================================

/// Хост, на всю жизнь мода. Атомарная переменная, а не `static mut`:
/// сервер зовёт обработчик из тактового цикла, а скрипт зовёт нас со
/// своего потока, и `static mut`, прочитанный из двух потоков, —
/// неопределённое поведение, как бы аккуратно он ни был написан.
static HOST: AtomicU64 = AtomicU64::new(0);

/// Открытая библиотека языка и то, что из неё нужно.
struct Language {
    run: DyptRunFn,
    open: DyptOpenFn,
    has: DyptHasFn,
    call: DyptCallFn,
    close: DyptCloseFn,
    version: String,
    /// **Роняется последней, и это несущее.** Выгрузка библиотеки
    /// обесценивает указатель `run`, поэтому библиотека обязана
    /// пережить его — что и обеспечивает порядок полей, поскольку Rust
    /// роняет поля в порядке объявления.
    _library: libloading::Library,
}

// Безопасно передавать между потоками: это указатель на функцию и
// открытая библиотека, а библиотека живёт, пока живёт мод. Сам вызов
// защищён `RUNNING` — одновременно идёт не больше одного скрипта.
unsafe impl Send for Language {}

static LANGUAGE: Mutex<Option<Language>> = Mutex::new(None);

/// Всё, что прочитано из манифеста один раз при загрузке.
#[derive(Default)]
struct Config {
    /// Папка со скриптами — уже сложенная с папкой мода, которая
    /// выясняется у операционной системы: у API нет вызова «где я
    /// лежу», а пути в манифесте относительные, и относительно мода, а
    /// не рабочего каталога сервера, который у службы бывает каким
    /// угодно.
    scripts: PathBuf,
    /// Папка со скриптовыми модами — теми, что живут постоянно.
    mods: PathBuf,
    operators_only: bool,
    limit: Option<Duration>,
    /// Сколько ждать ответа обработчика, прежде чем считать поток
    /// скриптов потерянным. Ждёт тактовый цикл сервера, поэтому число
    /// маленькое.
    handler_limit: Duration,
    autorun: String,
}

static CONFIG: Mutex<Option<Config>> = Mutex::new(None);

/// Идёт ли сейчас скрипт. Один за раз — намеренно: два скрипта,
/// строящие в одном месте, это не возможность, а способ получить
/// половину дома и половину моста.
static RUNNING: AtomicBool = AtomicBool::new(false);
/// Просили остановиться.
static CANCEL: AtomicBool = AtomicBool::new(false);

/// На что уже подписались. См. [`subscribe_for`].
static SUBSCRIBED: Mutex<Vec<i32>> = Mutex::new(Vec::new());

fn host() -> Option<&'static HostApi> {
    let raw = HOST.load(Ordering::Acquire);
    if raw == 0 {
        return None;
    }
    // Безопасность: пишет сюда только `load`, тем указателем, который
    // дал хост, а хост держит свои таблицы живыми, пока загружен мод.
    Some(unsafe { &*(raw as *const HostApi) })
}

fn log(level: LogLevel, message: &str) {
    let Some(api) = host() else { return };
    if api.core.is_null() {
        return;
    }
    unsafe { ((*api.core).log)(api.handle, level, Str::borrow(message)) }
}

fn tell(player: PlayerId, text: &str) {
    let Some(api) = host() else { return };
    // Ноль — это консоль или автозапуск: писать некому, и строка уходит
    // в журнал. Молчать было бы хуже всего: вывод скрипта, запущенного
    // при старте сервера, иначе исчезал бы целиком.
    if player == 0 || api.network.is_null() {
        log(LogLevel::Info, text);
        return;
    }
    unsafe { ((*api.network).tell)(api.handle, player, Str::borrow(text)) };
}

/// Одна настройка из собственного `mod.ron`.
///
/// Соглашение из двух вызовов, которым пользуется каждый возвращающий
/// текст вызов этого API: сначала спрашиваем длину с нулевой ёмкостью,
/// потом читаем в буфер.
fn setting(key: &str) -> Option<String> {
    let api = host()?;
    if api.core.is_null() {
        return None;
    }
    let mut needed: usize = 0;
    let status = unsafe {
        ((*api.core).setting)(
            api.handle,
            Str::borrow(key),
            std::ptr::null_mut(),
            0,
            &mut needed,
        )
    };
    if status == Status::NotFound || needed == 0 {
        return None;
    }
    let mut buffer = vec![0u8; needed];
    let mut written: usize = 0;
    let status = unsafe {
        ((*api.core).setting)(
            api.handle,
            Str::borrow(key),
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut written,
        )
    };
    if !status.is_ok() {
        return None;
    }
    buffer.truncate(written);
    String::from_utf8(buffer).ok()
}

// =====================================================================
// Где лежит сам мод
// =====================================================================

/// Каталог, из которого загружена эта библиотека.
///
/// В API нет вызова «в какой папке лежит мод», и это не упущение: хост
/// знает ответ, но мод, которому он нужен, до сих пор был один.
///
/// **Угадать этот путь нельзя, и это выяснено опытом, а не рассуждением.**
/// Очевидный способ — сложить рабочий каталог сервера с `mod_dir` —
/// даёт неверный ответ на Windows, и вот почему. Сервер открывает мод
/// **относительным** путём (`mods/dypt/dypt.dll`), а `LoadLibrary`
/// разбирает относительный путь не от рабочего каталога, а по своему
/// порядку поиска, и первым в нём стоит каталог самой программы. На
/// машине, где рядом с `primitive_server.exe` лежит ещё одна копия
/// папки `mods` — а она там лежит у всякого, кто хоть раз собирал
/// поставку, — загружается **та** копия, а не та, которую сервер
/// назвал. Мод, который после этого искал бы свои скрипты от рабочего
/// каталога, читал бы файлы из одной папки, будучи кодом из другой:
/// правки в скриптах не действуют, и причину такого искать больно.
///
/// Спросить операционную систему, откуда взялся *этот самый* код, —
/// двадцать строк, и они отвечают на нужный вопрос при любом порядке
/// поиска и любом рабочем каталоге.
#[cfg(windows)]
fn own_folder() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;

    const GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS: u32 = 0x0000_0004;
    const GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT: u32 = 0x0000_0002;

    extern "system" {
        fn GetModuleHandleExW(flags: u32, address: *const u16, module: *mut *mut c_void) -> i32;
        fn GetModuleFileNameW(module: *mut c_void, filename: *mut u16, size: u32) -> u32;
    }

    let mut module: *mut c_void = std::ptr::null_mut();
    // Адрес любой функции этой библиотеки — по нему система и находит
    // модуль. `own_folder` подходит не хуже прочих.
    let anchor = own_folder as *const u16;
    let ok = unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            anchor,
            &mut module,
        )
    };
    if ok == 0 || module.is_null() {
        return None;
    }
    let mut buffer = vec![0u16; 32768];
    let written = unsafe { GetModuleFileNameW(module, buffer.as_mut_ptr(), buffer.len() as u32) };
    if written == 0 {
        return None;
    }
    buffer.truncate(written as usize);
    let path = PathBuf::from(std::ffi::OsString::from_wide(&buffer));
    path.parent().map(|p| p.to_path_buf())
}

#[cfg(unix)]
fn own_folder() -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;

    #[repr(C)]
    struct DlInfo {
        fname: *const std::ffi::c_char,
        fbase: *mut c_void,
        sname: *const std::ffi::c_char,
        saddr: *mut c_void,
    }
    extern "C" {
        fn dladdr(address: *const c_void, info: *mut DlInfo) -> i32;
    }

    let mut info = DlInfo {
        fname: std::ptr::null(),
        fbase: std::ptr::null_mut(),
        sname: std::ptr::null(),
        saddr: std::ptr::null_mut(),
    };
    let ok = unsafe { dladdr(own_folder as *const c_void, &mut info) };
    if ok == 0 || info.fname.is_null() {
        return None;
    }
    let name = unsafe { std::ffi::CStr::from_ptr(info.fname) };
    let path = PathBuf::from(std::ffi::OsStr::from_bytes(name.to_bytes()));
    path.parent().map(|p| p.to_path_buf())
}

// =====================================================================
// Загрузка
// =====================================================================

/// Вызывается один раз, после того как загружены все моды.
fn load(host_ptr: *const HostApi) -> Status {
    if host_ptr.is_null() {
        return Status::BadArgument;
    }
    HOST.store(host_ptr as u64, Ordering::Release);
    let api = unsafe { &*host_ptr };

    // Проверяем хозяина, хотя он уже проверил нас. Мод, загруженный
    // *старым* хостом, иначе прочитал бы поле за концом таблицы прежде,
    // чем успел бы пожаловаться.
    if !api.version.accepts(API_VERSION) {
        log(
            LogLevel::Error,
            &format!(
                "хост объявляет API {}, мод собран под {} — не загружаюсь",
                api.version, API_VERSION
            ),
        );
        return Status::Refused;
    }

    let folder = own_folder().unwrap_or_else(|| PathBuf::from("mods/dypt"));
    let scripts = folder.join(setting("scripts").unwrap_or_else(|| "scripts".to_string()));
    let limit_seconds = setting("limit_seconds")
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(30);
    let handler_ms = setting("handler_ms")
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(1000);
    let config = Config {
        scripts,
        mods: folder.join(setting("mods").unwrap_or_else(|| "mods".to_string())),
        operators_only: setting("operators_only")
            .map(|s| s.trim() == "true")
            .unwrap_or(true),
        limit: (limit_seconds > 0).then(|| Duration::from_secs(limit_seconds)),
        handler_limit: Duration::from_millis(handler_ms.max(1)),
        autorun: setting("autorun").unwrap_or_default(),
    };

    // Стандартная библиотека языка. dypt ищет её рядом со своим
    // исполняемым файлом, а исполняемый файл здесь — сервер игры, так
    // что искать он будет не там. Переменная окружения — единственный
    // рычаг, который у языка для этого есть, и ставится она один раз,
    // здесь, до того как появится первый скрипт.
    let lib = folder.join(setting("lib").unwrap_or_else(|| "lib".to_string()));
    if lib.is_dir() {
        std::env::set_var("DYPT_LIB", &lib);
    }

    let runtime_name = setting("runtime").unwrap_or_else(|| platform_runtime().to_string());
    let runtime_path = folder.join(&runtime_name);
    match open_language(&runtime_path) {
        Ok(language) => {
            log(
                LogLevel::Info,
                &format!(
                    "dypt {} готов: {} игровых функций, скрипты в {}",
                    language.version,
                    VOCABULARY.len(),
                    config.scripts.display()
                ),
            );
            *LANGUAGE.lock().unwrap_or_else(|e| e.into_inner()) = Some(language);
        }
        Err(reason) => {
            // Не отказ загрузки: мод, который умеет сказать, чего ему не
            // хватает, полезнее мода, которого нет в списке `/mods`.
            log(
                LogLevel::Warn,
                &format!(
                    "язык не открыт ({}): {}. /dypt будет отвечать этим же",
                    runtime_path.display(),
                    reason
                ),
            );
        }
    }

    *CONFIG.lock().unwrap_or_else(|e| e.into_inner()) = Some(config);

    // Подписка и объявление команды — здесь, а не в точке входа: там
    // остальных модов ещё нет. И здесь же, а не на потоке скрипта:
    // обе эти операции хост записывает на «того, кто сейчас на стеке»,
    // а на чужом потоке на стеке нет никого.
    if !api.events.is_null() {
        unsafe {
            ((*api.events).register_command)(
                api.handle,
                Str::borrow("dypt"),
                Str::borrow(
                    "/dypt <файл|eval <код>|list|mods|reload|stop> — программы на dypt",
                ),
            );
            ((*api.events).subscribe)(api.handle, Event::Command);
            ((*api.events).subscribe)(api.handle, Event::ServerStarted);
            ((*api.events).subscribe)(api.handle, Event::ServerStopping);
        }
    }

    // ...и только теперь скриптовые моды: до этой строки нет ни языка,
    // ни настроек, из которых они читаются. Загрузка синхронная — мы
    // ждём поток скриптов, — потому что её итог решает, на что
    // подписываться, а подписаться можно только отсюда.
    start_worker();
    // Минута: тактов ещё нет, ждать некому, а разбор десятка скриптов
    // честно занимает время.
    let report = reload_script_mods(Duration::from_secs(60), false);
    for line in report.lines {
        log(LogLevel::Info, &line);
    }
    subscribe_for(api, &report.events);

    Status::Ok
}

/// Подписывается на события, у которых есть хоть один обработчик.
///
/// Только на них: мод, подписанный на всё подряд, — это переход через
/// границу на каждую ячейку, которую сдвинула вода, даже когда ни один
/// скрипт про воду не спрашивал.
///
/// Отписки нет. Скриптовый мод, потерявший обработчик после
/// `/dypt reload`, оставляет за собой подписку, которая теперь никого
/// не зовёт — это один сравнимый с нулём `match` на событие, и это
/// дешевле, чем отписка, которую пришлось бы согласовывать с другими
/// скриптами, ещё нуждающимися в том же событии.
fn subscribe_for(api: &HostApi, events: &[Event]) {
    if api.events.is_null() {
        return;
    }
    let mut known = SUBSCRIBED.lock().unwrap_or_else(|e| e.into_inner());
    for event in events {
        let key = *event as i32;
        if known.contains(&key) {
            continue;
        }
        known.push(key);
        unsafe { ((*api.events).subscribe)(api.handle, *event) };
    }
}

fn platform_runtime() -> &'static str {
    if cfg!(windows) {
        "dypt_embed.dll"
    } else if cfg!(target_os = "macos") {
        "libdypt_embed.dylib"
    } else {
        "libdypt_embed.so"
    }
}

fn open_language(path: &Path) -> Result<Language, String> {
    if !path.is_file() {
        return Err("файла нет".to_string());
    }
    // Безопасность: открытие библиотеки выполняет её инициализаторы, и
    // это ровно то, чего мы хотим. Проверить содержимое заранее нельзя
    // никак — можно только не давать пути прийти извне, а он приходит
    // из манифеста рядом с самой библиотекой.
    let library = unsafe { libloading::Library::new(path) }.map_err(|e| e.to_string())?;

    let abi: libloading::Symbol<DyptAbiFn> = unsafe { library.get(b"dypt_abi_version\0") }
        .map_err(|_| "нет dypt_abi_version — это не библиотека dypt".to_string())?;
    let theirs = unsafe { abi() };
    if theirs != DY_ABI {
        return Err(format!(
            "версия договора {} против нашей {}: пересоберите обе стороны",
            theirs, DY_ABI
        ));
    }

    let version: libloading::Symbol<DyptVersionFn> = unsafe { library.get(b"dypt_version\0") }
        .map_err(|_| "нет dypt_version".to_string())?;
    let version = unsafe { version().as_str() }.to_string();

    let run: libloading::Symbol<DyptRunFn> =
        unsafe { library.get(b"dypt_run\0") }.map_err(|_| "нет dypt_run".to_string())?;
    let run = *run;
    let open: libloading::Symbol<DyptOpenFn> =
        unsafe { library.get(b"dypt_open\0") }.map_err(|_| "нет dypt_open".to_string())?;
    let open = *open;
    let has: libloading::Symbol<DyptHasFn> =
        unsafe { library.get(b"dypt_has\0") }.map_err(|_| "нет dypt_has".to_string())?;
    let has = *has;
    let call: libloading::Symbol<DyptCallFn> =
        unsafe { library.get(b"dypt_call\0") }.map_err(|_| "нет dypt_call".to_string())?;
    let call = *call;
    let close: libloading::Symbol<DyptCloseFn> =
        unsafe { library.get(b"dypt_close\0") }.map_err(|_| "нет dypt_close".to_string())?;
    let close = *close;

    Ok(Language {
        run,
        open,
        has,
        call,
        close,
        version,
        _library: library,
    })
}

fn unload() {
    // Скрипту говорят остановиться и ждут недолго. Дольше ждать нечем:
    // прервать его нельзя (см. шапку), а сервер сохраняет мир и не
    // может стоять из-за чужого цикла.
    CANCEL.store(true, Ordering::SeqCst);
    let deadline = Instant::now() + Duration::from_millis(500);
    while RUNNING.load(Ordering::SeqCst) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }

    // ...а скриптовым модам дают попрощаться. `on_unload` — то место,
    // где скрипт записывает своё состояние, и вызывается оно здесь,
    // пока мир ещё жив: `ServerStopping` для того и существует.
    let _ = ask(|reply| Job::Quit { reply }, Duration::from_secs(5));
    *JOBS.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

// =====================================================================
// Команда
// =====================================================================

fn on_event(event: Event, data: *const EventData) -> HookResult {
    if data.is_null() {
        return HookResult::Continue;
    }
    let data = unsafe { &*data };

    // Сперва своё дело, потом чужое. `/dypt` никому не передоверяется,
    // а на остановке сервера скриптам надо дать проститься **до** того,
    // как мод закроет их всех.
    match event {
        Event::Command => {
            if unsafe { data.text.as_str() }.eq_ignore_ascii_case("dypt") {
                return command(data);
            }
        }
        Event::ServerStarted => {
            let autorun = with_config(|c| c.autorun.clone()).unwrap_or_default();
            if !autorun.trim().is_empty() {
                begin(0, Source::File(resolve(autorun.trim())), Vec::new());
            }
        }
        Event::ServerStopping => {
            deliver(event, data);
            unload();
            return HookResult::Continue;
        }
        _ => {}
    }

    deliver(event, data)
}

fn command(data: &EventData) -> HookResult {
    let player = data.player;
    let line = unsafe { data.args.as_str() }.trim().to_string();

    // Право спрашивается до всего остального. Скрипту доступен
    // `command()`, то есть любая серверная команда с правами
    // оператора; открыть `/dypt` всем — это раздать права оператора
    // всем, а не «разрешить поиграть со скриптами».
    let operators_only = with_config(|c| c.operators_only).unwrap_or(true);
    if operators_only && player != 0 && !is_operator(player) {
        tell(player, "/dypt — только для операторов");
        return HookResult::Cancel;
    }

    let mut words = line.split_whitespace();
    match words.next() {
        None | Some("help") => {
            for row in [
                "/dypt <файл> [аргументы] — выполнить скрипт из папки scripts",
                "/dypt eval <код>        — выполнить одну строку",
                "/dypt list              — что лежит в папке скриптов",
                "/dypt stop              — остановить то, что идёт",
                "/dypt mods              — какие скриптовые моды живут",
                "/dypt reload            — перечитать скриптовые моды",
                "принимаются .dpt (исходник), .dyc (байткод) и .dasm",
            ] {
                tell(player, row);
            }
        }
        Some("list") => list(player),
        Some("mods") => {
            match ask(|reply| Job::List { reply }, Duration::from_secs(5)) {
                Some(lines) if lines.is_empty() => {
                    tell(player, "скриптовых модов нет")
                }
                Some(lines) => {
                    tell(player, "скриптовые моды:");
                    for line in lines {
                        tell(player, &line);
                    }
                }
                None => tell(player, "поток скриптовых модов не отвечает"),
            }
            if !DISPATCH.load(Ordering::SeqCst) {
                tell(player, "доставка событий выключена — помогает /dypt reload");
            }
        }
        Some("reload") => {
            // Пять секунд, а не минута: эту команду разбирает тактовый
            // цикл сервера, и всё это время мир стоит.
            let report = reload_script_mods(Duration::from_secs(5), true);
            for line in &report.lines {
                // `tell` сам пишет в журнал, когда звал не игрок, а
                // консоль; писать ещё раз значило бы удваивать каждую
                // строку отчёта именно там, где его и читают.
                tell(player, line);
                if player != 0 {
                    log(LogLevel::Info, line);
                }
            }
            // Подписка — только отсюда: хост записывает её на «того, кто
            // сейчас на стеке», а на стеке сейчас мы, потому что это
            // обработчик события. С потока скриптов она бы молча ничего
            // не сделала.
            if let Some(api) = host() {
                subscribe_for(api, &report.events);
            }
        }
        Some("stop") => {
            if RUNNING.load(Ordering::SeqCst) {
                CANCEL.store(true, Ordering::SeqCst);
                tell(player, "прошу остановиться");
            } else {
                tell(player, "ничего не выполняется");
            }
        }
        Some("eval") => {
            let code = line["eval".len()..].trim().to_string();
            if code.is_empty() {
                tell(player, "после eval нужен код: /dypt eval print(2 + 2)");
            } else {
                begin(player, Source::Text(code), Vec::new());
            }
        }
        Some(file) => {
            let arguments: Vec<String> = words.map(|w| w.to_string()).collect();
            begin(player, Source::File(resolve(file)), arguments);
        }
    }
    // Команда обработана: сервер иначе объявит её опечаткой.
    HookResult::Cancel
}

fn is_operator(player: PlayerId) -> bool {
    let Some(api) = host() else { return false };
    if api.players.is_null() {
        return false;
    }
    unsafe { ((*api.players).is_operator)(api.handle, player) }
}

fn with_config<T>(f: impl FnOnce(&Config) -> T) -> Option<T> {
    let guard = CONFIG.lock().unwrap_or_else(|e| e.into_inner());
    guard.as_ref().map(f)
}

/// Имя из чата — в путь.
///
/// Без расширения дописывается `.dpt`: `/dypt дом` должно работать, а
/// `.dpt` — то, что человек пишет руками. Путь всегда внутри папки
/// скриптов, и `..` из него вырезается: `/dypt ../../settings.toml`
/// иначе читал бы что угодно на диске правами сервера.
fn resolve(name: &str) -> PathBuf {
    let mut safe = PathBuf::new();
    for part in Path::new(name).components() {
        if let std::path::Component::Normal(part) = part {
            safe.push(part);
        }
    }
    if safe.extension().is_none() {
        safe.set_extension("dpt");
    }
    let root = with_config(|c| c.scripts.clone()).unwrap_or_else(|| PathBuf::from("scripts"));
    root.join(safe)
}

fn list(player: PlayerId) {
    let root = with_config(|c| c.scripts.clone()).unwrap_or_else(|| PathBuf::from("scripts"));
    let Ok(entries) = std::fs::read_dir(&root) else {
        tell(player, &format!("папки {} нет", root.display()));
        return;
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_file())
        .filter(|e| {
            matches!(
                e.path().extension().and_then(|s| s.to_str()),
                Some("dpt") | Some("dyc") | Some("dasm")
            )
        })
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    if names.is_empty() {
        tell(player, &format!("в {} пусто", root.display()));
        return;
    }
    tell(player, &format!("скрипты в {}:", root.display()));
    for name in names {
        tell(player, &format!("  {}", name));
    }
}

enum Source {
    File(PathBuf),
    Text(String),
}

/// Запускает скрипт на своём потоке.
fn begin(player: PlayerId, source: Source, arguments: Vec<String>) {
    if LANGUAGE.lock().unwrap_or_else(|e| e.into_inner()).is_none() {
        tell(player, "язык не загружен — смотрите журнал сервера при старте");
        return;
    }
    if let Source::File(path) = &source {
        if !path.is_file() {
            tell(player, &format!("нет файла {}", path.display()));
            return;
        }
    }
    // Один скрипт за раз. `compare_exchange`, а не «прочитать и
    // записать»: две команды, набранные в один такт двумя игроками,
    // иначе прошли бы обе.
    if RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        tell(player, "уже выполняется другой скрипт — /dypt stop");
        return;
    }
    CANCEL.store(false, Ordering::SeqCst);

    let limit = with_config(|c| c.limit).unwrap_or(Some(Duration::from_secs(30)));
    let base = with_config(|c| c.scripts.clone()).unwrap_or_default();

    // Свой стек, и большой: разбор и компиляция dypt рекурсивны по
    // дереву программы, а поток, который сервер даёт обработчику
    // события, рассчитан на обработчик события.
    let spawned = std::thread::Builder::new()
        .name("dypt".to_string())
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            run_script(player, source, arguments, base, limit);
            RUNNING.store(false, Ordering::SeqCst);
        });
    if spawned.is_err() {
        RUNNING.store(false, Ordering::SeqCst);
        tell(player, "не удалось создать поток для скрипта");
    }
}

fn run_script(
    player: PlayerId,
    source: Source,
    arguments: Vec<String>,
    base: PathBuf,
    limit: Option<Duration>,
) {
    let run = {
        let guard = LANGUAGE.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(language) => language.run,
            None => return,
        }
    };

    let mut session = Session {
        caller: player,
        label: String::new(),
        arguments,
        deadline: limit.map(|d| Instant::now() + d),
        honour_cancel: true,
        out: Bag::default(),
    };
    let host_table = DyHost {
        user: &mut session as *mut Session as *mut c_void,
        call: host_call,
        print: host_print,
    };

    let names: Vec<DyStr> = VOCABULARY.iter().map(|n| DyStr::of(n)).collect();
    let base_text = base.to_string_lossy().into_owned();
    let (kind, input) = match &source {
        Source::File(path) => (DY_INPUT_PATH, path.to_string_lossy().into_owned()),
        Source::Text(text) => (DY_INPUT_SOURCE, text.clone()),
    };

    let started = Instant::now();
    let mut error = vec![0u8; 4096];
    let mut error_len: usize = 0;
    let code = unsafe {
        run(
            kind,
            DyStr::of(&input),
            DyStr::of(&base_text),
            names.as_ptr(),
            names.len(),
            &host_table,
            error.as_mut_ptr(),
            error.len(),
            &mut error_len,
        )
    };

    if code == 0 {
        tell(
            player,
            &format!("готово за {} мс", started.elapsed().as_millis()),
        );
    } else {
        error.truncate(error_len);
        let message = String::from_utf8_lossy(&error).into_owned();
        // Многострочная ошибка dypt (а она бывает с рамкой и стрелкой)
        // уходит в чат построчно: чат не умеет переводов строки внутри
        // сообщения и склеил бы всё в одну кашу.
        for row in message.lines() {
            tell(player, row);
        }
    }
}

// =====================================================================
// То, что зовёт скрипт
// =====================================================================

/// Состояние одного запуска. Живёт на стеке потока скрипта.
struct Session {
    /// Кому уходит `print` и кого подставляет вызов, которому игрока не
    /// назвали. Ноль у скриптового мода и у автозапуска: там нет
    /// человека, который бы это набрал.
    caller: PlayerId,
    /// Имя скриптового мода — им помечается его вывод в журнале. Пусто
    /// у разового скрипта, потому что его вывод идёт человеку в чат и
    /// подписывать его именем файла незачем.
    label: String,
    arguments: Vec<String>,
    deadline: Option<Instant>,
    /// Слушать ли `/dypt stop`. Разовый скрипт — да, скриптовый мод —
    /// нет: остановка задумана как «прекрати то, что я сейчас
    /// запустил», а не «выключи всё, что живёт на сервере».
    honour_cancel: bool,
    /// Память под возвращаемое значение — заводится заново перед
    /// каждым вызовом. Дольше держать не надо: та сторона
    /// перекладывает значение к себе, не выходя из вызова.
    out: Bag,
}

/// Место, где живёт то, что мод отдаёт скрипту.
///
/// Отдельно от [`Session`], потому что таких мест два: возвращаемое
/// значение живёт в сессии, а доводы обработчика события собираются на
/// стеке того, кто его зовёт, и умирают там же.
#[derive(Default)]
struct Bag {
    blocks: Vec<Box<[DyValue]>>,
    texts: Vec<String>,
}

impl Bag {
    fn block(&mut self, values: Vec<DyValue>) -> (*const DyValue, usize) {
        let boxed = values.into_boxed_slice();
        let ptr = boxed.as_ptr();
        let len = boxed.len();
        self.blocks.push(boxed);
        (ptr, len)
    }

    fn text(&mut self, value: String) -> DyStr {
        self.texts.push(value);
        DyStr::of(self.texts.last().expect("только что положили"))
    }
}

/// Значение, которое мод возвращает скрипту.
///
/// Своё дерево, а не сразу [`DyValue`]: собрать ответ проще, когда
/// строки и списки владеют собой, а разложить владеющее дерево в
/// плоские указатели — двадцать строк в одном месте
/// ([`Bag::flatten`]). Оно же ходит по каналу к потоку скриптов, когда
/// событие несёт доводы обработчику.
#[derive(Clone)]
enum Val {
    Null,
    Bool(bool),
    Int(i64),
    Num(f64),
    Text(String),
    List(Vec<Val>),
    Map(Vec<(&'static str, Val)>),
}

impl Bag {
    fn flatten(&mut self, value: Val) -> DyValue {
        match value {
            Val::Null => DyValue::NULL,
            Val::Bool(b) => DyValue {
                kind: DY_BOOL,
                b: u32::from(b),
                ..DyValue::NULL
            },
            Val::Int(i) => DyValue {
                kind: DY_INT,
                i,
                ..DyValue::NULL
            },
            Val::Num(f) => DyValue {
                kind: DY_FLOAT,
                f,
                ..DyValue::NULL
            },
            Val::Text(t) => {
                let s = self.text(t);
                DyValue {
                    kind: DY_STRING,
                    s,
                    ..DyValue::NULL
                }
            }
            Val::List(items) => {
                let converted: Vec<DyValue> =
                    items.into_iter().map(|v| self.flatten(v)).collect();
                let (items, count) = self.block(converted);
                DyValue {
                    kind: DY_LIST,
                    items,
                    count,
                    ..DyValue::NULL
                }
            }
            Val::Map(pairs) => {
                let count = pairs.len();
                let mut flat = Vec::with_capacity(count * 2);
                for (key, value) in pairs {
                    let key = Val::Text(key.to_string());
                    let key = self.flatten(key);
                    let value = self.flatten(value);
                    flat.push(key);
                    flat.push(value);
                }
                let (items, _) = self.block(flat);
                DyValue {
                    kind: DY_MAP,
                    items,
                    count,
                    ..DyValue::NULL
                }
            }
        }
    }
}

unsafe extern "C" fn host_print(user: *mut c_void, text: DyStr) {
    if user.is_null() {
        return;
    }
    let session = &*(user as *const Session);
    if session.caller != 0 {
        tell(session.caller, text.as_str());
        return;
    }
    // Скриптовому моду писать некому: он живёт сам по себе, а не по
    // чьей-то команде. Его вывод идёт в журнал под его собственным
    // именем — иначе три скрипта, печатающие «готово», неразличимы.
    if session.label.is_empty() {
        log(LogLevel::Info, text.as_str());
    } else {
        log(
            LogLevel::Info,
            &format!("[{}] {}", session.label, text.as_str()),
        );
    }
}

unsafe extern "C" fn host_call(
    user: *mut c_void,
    name: DyStr,
    args: *const DyValue,
    argc: usize,
    out: *mut DyValue,
) -> i32 {
    if user.is_null() || out.is_null() {
        return 1;
    }
    let session = &mut *(user as *mut Session);
    session.out = Bag::default();
    *out = DyValue::NULL;

    let name = name.as_str();
    let args: &[DyValue] = if args.is_null() || argc == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(args, argc)
    };

    // Здесь же и вся остановка, какая у нас есть: скрипт нельзя
    // прервать, но можно перестать ему отвечать. См. шапку файла.
    let refusal = if session.honour_cancel && CANCEL.load(Ordering::SeqCst) {
        Some("остановлено".to_string())
    } else if session.deadline.is_some_and(|d| Instant::now() > d) {
        Some("скрипт идёт дольше отведённого времени (limit_seconds)".to_string())
    } else {
        None
    };
    if let Some(reason) = refusal {
        let value = Val::Text(reason);
        *out = session.out.flatten(value);
        return 1;
    }

    // Паника не должна уйти обратно в библиотеку языка: раскрутка через
    // C ABI — неопределённое поведение. Ошибка в аргументах доходит
    // сюда как `Err`, а вот выход за границу среза — как паника.
    let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        dispatch(session, name, args)
    }));

    match answer {
        Ok(Ok(value)) => {
            *out = session.out.flatten(value);
            0
        }
        Ok(Err(message)) => {
            let value = Val::Text(message);
            *out = session.out.flatten(value);
            1
        }
        Err(_) => {
            let value = Val::Text(format!("{}: внутренняя ошибка мода", name));
            *out = session.out.flatten(value);
            1
        }
    }
}

// =====================================================================
// Словарь
// =====================================================================

/// Каждое имя, которое скрипт может позвать.
///
/// Список отдаётся языку при запуске: имена, которых в нём нет, язык
/// объявлять встроенными не станет. Он же — единственное место, где
/// словарь перечислен, и [`dispatch`] обязан отвечать на каждое имя
/// отсюда; за этим следит тест внизу файла.
///
/// Имена нарочно не похожи на встроенные функции самого dypt (`get`,
/// `set`, `map`, `length`, `type`...): объявить одноимённую — значит
/// закрыть скрипту доступ к той, что была, и человек будет искать
/// причину в своём коде.
pub const VOCABULARY: &[&str] = &[
    // кто и что
    "me",
    "args",
    "say",
    "tell",
    "note",
    "command",
    "stopped",
    // время и погода
    "tick",
    "time",
    "set_time",
    "day_length",
    "tick_rate",
    "weather",
    "set_weather",
    // мир
    "block_at",
    "place",
    "dig",
    "fill_blocks",
    "disturb",
    "surface",
    "terrain_height",
    "biome",
    "climate",
    "temperature",
    "world_seed",
    "spawn_point",
    "chunk_loaded",
    "load_chunk",
    "chunk_count",
    "save_world",
    // свет
    "sky_light",
    "block_light",
    "open_to_sky",
    "max_light",
    // таблица блоков
    "block_id",
    "block_name",
    "block_info",
    "block_tools",
    "block_count",
    "block_by_index",
    "break_time",
    "same_kind",
    "is_solid",
    "liquid_depth",
    // игроки
    "players",
    "player_name",
    "find_player",
    "pos",
    "look",
    "teleport",
    "vitals",
    "hurt",
    "cure",
    "set_health",
    "feed",
    "drink",
    "warm",
    "kick",
    "is_operator",
    "fly",
    "is_flying",
    "on_ground",
    "submerged",
    "respawn",
    "selected",
    "select",
    // инвентарь
    "held",
    "give",
    "take",
    "carrying",
    "slot_at",
    "put_slot",
    "throw_slot",
    "spill",
    "slot_count",
    "used_slots",
    "worn",
    "wear",
    // животные
    "entities",
    "entities_near",
    "entity_count",
    "entity_pos",
    "entity_health",
    "entity_species",
    "species_name",
    "species_id",
    "species_info",
    "species_count",
    "spawn",
    "kill",
    "despawn",
    "hurt_entity",
    "heal_entity",
    // вещи на земле
    "drop_item",
    "items_near",
    "clear_items",
    "item_count",
    // движение и бой
    "raycast",
    "rules",
    "within_reach",
    "weapon_damage",
    // вода
    "is_liquid",
    "is_source",
    "water_place",
    "water_take",
    "water_depth",
    // хранилища
    "container_slots",
    "container_get",
    "container_put",
    "container_take",
    "container_spill",
    "containers",
    // огонь и печи
    "light_fire",
    "feed_fire",
    "extinguish",
    "fuel_left",
    "fuel_of",
    "fire_near",
    "smelting",
    "drying",
    "cures_into",
    // еда
    "is_food",
    "nutrition",
    "eat",
    // рецепты
    "recipe_count",
    "recipe",
    "recipe_name",
    "can_craft",
    "craft",
    // что делает мир сам
    "growth_pending",
    "watch_growth",
    "is_trunk",
];

// ------------------------------------------------------- чтение доводов

fn need(args: &[DyValue], index: usize, name: &str) -> Result<DyValue, String> {
    args.get(index)
        .copied()
        .ok_or_else(|| format!("{}: не хватает довода №{}", name, index + 1))
}

fn as_num(value: &DyValue) -> Option<f64> {
    match value.kind {
        DY_INT => Some(value.i as f64),
        DY_FLOAT => Some(value.f),
        DY_BOOL => Some(f64::from(value.b)),
        _ => None,
    }
}

fn num(args: &[DyValue], index: usize, name: &str) -> Result<f64, String> {
    let value = need(args, index, name)?;
    as_num(&value).ok_or_else(|| format!("{}: довод №{} — не число", name, index + 1))
}

fn int(args: &[DyValue], index: usize, name: &str) -> Result<i64, String> {
    Ok(num(args, index, name)? as i64)
}

fn text(args: &[DyValue], index: usize, name: &str) -> Result<String, String> {
    let value = need(args, index, name)?;
    if value.kind != DY_STRING {
        return Err(format!("{}: довод №{} — не строка", name, index + 1));
    }
    Ok(unsafe { value.s.as_str() }.to_string())
}

/// Необязательный признак: нет довода — берётся значение по умолчанию.
fn flag(args: &[DyValue], index: usize, default: bool) -> bool {
    match args.get(index) {
        Some(value) if value.kind == DY_BOOL => value.b != 0,
        Some(value) => as_num(value).map(|n| n != 0.0).unwrap_or(default),
        None => default,
    }
}

fn opt_num(args: &[DyValue], index: usize) -> Option<f64> {
    args.get(index).and_then(as_num)
}

/// Ячейка мира из трёх доводов.
///
/// **Округление вниз, а не отбрасывание дробной части.** `pos()`
/// возвращает дробные числа, и скрипт передаёт их сюда не задумываясь;
/// а `-0.5 as i32` — это ноль, тогда как игрок с координатой −0,5 стоит
/// в ячейке −1. Ошибка ровно в одну ячейку и только на отрицательной
/// половине мира: искать её потом будут долго.
fn cell(args: &[DyValue], index: usize, name: &str) -> Result<BlockPos, String> {
    Ok(BlockPos {
        x: num(args, index, name)?.floor() as i32,
        y: num(args, index + 1, name)?.floor() as i32,
        z: num(args, index + 2, name)?.floor() as i32,
    })
}

fn point(args: &[DyValue], index: usize, name: &str) -> Result<Vec3, String> {
    Ok(Vec3 {
        x: num(args, index, name)? as f32,
        y: num(args, index + 1, name)? as f32,
        z: num(args, index + 2, name)? as f32,
    })
}

/// Блок — числом или именем.
///
/// Именем, потому что скрипт пишет человек: `place(x, y, z, "stone")`
/// читается, а `place(x, y, z, 12)` — это число, которое надо где-то
/// подсмотреть и которое изменится, когда в таблицу блоков добавят
/// строку выше.
fn block(args: &[DyValue], index: usize, name: &str) -> Result<u16, String> {
    let value = need(args, index, name)?;
    if value.kind == DY_STRING {
        let wanted = unsafe { value.s.as_str() };
        return block_by_name(wanted)
            .ok_or_else(|| format!("{}: блока '{}' нет в таблице", name, wanted));
    }
    as_num(&value)
        .map(|n| n as u16)
        .ok_or_else(|| format!("{}: довод №{} — не блок", name, index + 1))
}

/// Игрок — числом, именем или ничем (тогда это тот, кто позвал).
fn player(session: &Session, args: &[DyValue], index: usize, name: &str) -> Result<PlayerId, String> {
    match args.get(index) {
        None => {
            if session.caller == 0 {
                Err(format!("{}: некого взять по умолчанию — скрипт запущен не игроком", name))
            } else {
                Ok(session.caller)
            }
        }
        Some(value) if value.kind == DY_STRING => {
            let wanted = unsafe { value.s.as_str() };
            find_player(wanted).ok_or_else(|| format!("{}: игрока '{}' здесь нет", name, wanted))
        }
        Some(value) => as_num(value)
            .map(|n| n as u64)
            .ok_or_else(|| format!("{}: довод №{} — не игрок", name, index + 1)),
    }
}

// --------------------------------------------------- обёртки над хостом

fn api() -> Result<&'static HostApi, String> {
    host().ok_or_else(|| "хост потерян".to_string())
}

fn ok(status: Status, what: &str) -> Result<Val, String> {
    if status.is_ok() {
        Ok(Val::Bool(true))
    } else {
        Err(format!("{}: {}", what, status_name(status)))
    }
}

fn status_name(status: Status) -> &'static str {
    match status {
        Status::Ok => "получилось",
        Status::NotFound => "не найдено",
        Status::BadArgument => "неверный довод",
        Status::Refused => "отказано",
        Status::Unavailable => "недоступно в этом процессе",
    }
}

/// Соглашение из двух вызовов для всего, что возвращает текст.
fn read_text(mut call: impl FnMut(*mut u8, usize, *mut usize) -> Status) -> Option<String> {
    let mut needed: usize = 0;
    call(std::ptr::null_mut(), 0, &mut needed);
    if needed == 0 {
        return None;
    }
    let mut buffer = vec![0u8; needed];
    let mut written: usize = 0;
    if !call(buffer.as_mut_ptr(), buffer.len(), &mut written).is_ok() {
        return None;
    }
    buffer.truncate(written);
    String::from_utf8(buffer).ok()
}

fn block_name(id: u16) -> Option<String> {
    let api = host()?;
    if api.blocks.is_null() {
        return None;
    }
    read_text(|out, cap, written| unsafe {
        ((*api.blocks).name)(api.handle, id, out, cap, written)
    })
}

fn block_by_name(name: &str) -> Option<u16> {
    let api = host()?;
    if api.blocks.is_null() {
        return None;
    }
    let mut id: u16 = 0;
    let status = unsafe { ((*api.blocks).by_name)(api.handle, Str::borrow(name), &mut id) };
    status.is_ok().then_some(id)
}

fn player_name(id: PlayerId) -> Option<String> {
    let api = host()?;
    if api.players.is_null() {
        return None;
    }
    read_text(|out, cap, written| unsafe {
        ((*api.players).name)(api.handle, id, out, cap, written)
    })
}

fn all_players() -> Vec<PlayerId> {
    let Some(api) = host() else { return Vec::new() };
    if api.players.is_null() {
        return Vec::new();
    }
    let count = unsafe { ((*api.players).count)(api.handle) } as usize;
    let mut buffer = vec![0u64; count.max(1)];
    let mut written: usize = 0;
    let status = unsafe {
        ((*api.players).all)(api.handle, buffer.as_mut_ptr(), buffer.len(), &mut written)
    };
    if !status.is_ok() {
        return Vec::new();
    }
    buffer.truncate(written);
    buffer
}

fn find_player(name: &str) -> Option<PlayerId> {
    all_players().into_iter().find(|id| {
        player_name(*id)
            .map(|n| n.eq_ignore_ascii_case(name))
            .unwrap_or(false)
    })
}

fn species_name(index: u32) -> Option<String> {
    let api = host()?;
    if api.entities.is_null() {
        return None;
    }
    read_text(|out, cap, written| unsafe {
        ((*api.entities).species_name)(api.handle, index, out, cap, written)
    })
}

fn species_by_name(name: &str) -> Option<u32> {
    let api = host()?;
    if api.entities.is_null() {
        return None;
    }
    let count = unsafe { ((*api.entities).species_count)(api.handle) };
    (0..count).find(|index| {
        species_name(*index)
            .map(|n| n.eq_ignore_ascii_case(name))
            .unwrap_or(false)
    })
}

/// Вид — числом или именем, как и блок.
fn species(args: &[DyValue], index: usize, name: &str) -> Result<u32, String> {
    let value = need(args, index, name)?;
    if value.kind == DY_STRING {
        let wanted = unsafe { value.s.as_str() };
        return species_by_name(wanted)
            .ok_or_else(|| format!("{}: вида '{}' нет", name, wanted));
    }
    as_num(&value)
        .map(|n| n as u32)
        .ok_or_else(|| format!("{}: довод №{} — не вид", name, index + 1))
}

fn stack_map(stack: ItemStack) -> Val {
    Val::Map(vec![
        ("block", Val::Int(i64::from(stack.block))),
        (
            "name",
            block_name(stack.block).map(Val::Text).unwrap_or(Val::Null),
        ),
        ("count", Val::Int(i64::from(stack.count))),
        ("damage", Val::Int(i64::from(stack.damage))),
    ])
}

fn vec3_list(v: Vec3) -> Val {
    Val::List(vec![
        Val::Num(f64::from(v.x)),
        Val::Num(f64::from(v.y)),
        Val::Num(f64::from(v.z)),
    ])
}

fn cell_list(p: BlockPos) -> Val {
    Val::List(vec![
        Val::Int(i64::from(p.x)),
        Val::Int(i64::from(p.y)),
        Val::Int(i64::from(p.z)),
    ])
}

fn body_slot(name: &str) -> Result<BodySlot, String> {
    match name.trim().to_lowercase().as_str() {
        "head" | "голова" => Ok(BodySlot::Head),
        "chest" | "торс" | "грудь" => Ok(BodySlot::Chest),
        "legs" | "ноги" => Ok(BodySlot::Legs),
        "feet" | "ступни" | "обувь" => Ok(BodySlot::Feet),
        other => Err(format!(
            "часть тела '{}' — бывают head, chest, legs, feet",
            other
        )),
    }
}

fn weather_name(weather: Weather) -> &'static str {
    match weather {
        Weather::Clear => "clear",
        Weather::Rain => "rain",
        Weather::Storm => "storm",
    }
}

fn weather_by_name(name: &str) -> Result<Weather, String> {
    match name.trim().to_lowercase().as_str() {
        "clear" | "ясно" => Ok(Weather::Clear),
        "rain" | "дождь" => Ok(Weather::Rain),
        "storm" | "гроза" | "буря" => Ok(Weather::Storm),
        other => Err(format!("погода '{}' — бывает clear, rain, storm", other)),
    }
}

fn feasibility_name(value: Feasibility) -> &'static str {
    match value {
        Feasibility::Ready => "ready",
        Feasibility::MissingIngredients => "missing",
        Feasibility::NoRoom => "no_room",
        Feasibility::NeedsFire => "needs_fire",
        Feasibility::NeedsForge => "needs_forge",
        Feasibility::NeedsBloomery => "needs_bloomery",
        Feasibility::NeedsWorkshop => "needs_workshop",
    }
}

// =====================================================================
// Разбор вызова
// =====================================================================

/// Одна игровая функция.
///
/// Большой `match`, и это осознанно: словарь — плоский список имён, а
/// таблица имён с указателями на функции здесь была бы тем же самым
/// списком, только разложенным по трём местам, которые расходятся.
#[allow(clippy::too_many_lines)] // словарь целиком, по одной строке на имя
fn dispatch(session: &mut Session, name: &str, args: &[DyValue]) -> Result<Val, String> {
    let api = api()?;
    let handle = api.handle;

    // Одна проверка на все таблицы, которых этот словарь касается.
    // Пустой указатель здесь — это «в этом процессе такого нет» (так
    // выглядят `render`, `audio` и `ui` на выделенном сервере), и
    // разыменовать его значило бы уронить сервер строкой из чата.
    // Проверять в каждой ветке по отдельности было бы честнее ровно на
    // одно сообщение об ошибке и на сотню строк длиннее.
    for (table, table_name) in [
        (api.core as *const c_void, "core"),
        (api.world as *const c_void, "world"),
        (api.generation as *const c_void, "generation"),
        (api.blocks as *const c_void, "blocks"),
        (api.items as *const c_void, "items"),
        (api.entities as *const c_void, "entities"),
        (api.players as *const c_void, "players"),
        (api.inventory as *const c_void, "inventory"),
        (api.physics as *const c_void, "physics"),
        (api.crafting as *const c_void, "crafting"),
        (api.lighting as *const c_void, "lighting"),
        (api.food as *const c_void, "food"),
        (api.combat as *const c_void, "combat"),
        (api.fluid as *const c_void, "fluid"),
        (api.containers as *const c_void, "containers"),
        (api.stations as *const c_void, "stations"),
        (api.simulation as *const c_void, "simulation"),
    ] {
        if table.is_null() {
            return Err(format!(
                "{}: таблицы '{}' нет в этом процессе",
                name, table_name
            ));
        }
    }

    match name {
        // ---------------------------------------------------- служебное
        "me" => Ok(Val::Int(session.caller as i64)),
        "args" => Ok(Val::List(
            session
                .arguments
                .iter()
                .map(|a| Val::Text(a.clone()))
                .collect(),
        )),
        "stopped" => Ok(Val::Bool(CANCEL.load(Ordering::SeqCst))),
        "say" => {
            let message = text(args, 0, name)?;
            if api.network.is_null() {
                return Err("сети нет в этом процессе".to_string());
            }
            ok(
                unsafe { ((*api.network).broadcast)(handle, Str::borrow(&message)) },
                name,
            )
        }
        "tell" => {
            let who = player(session, args, 0, name)?;
            let message = text(args, 1, name)?;
            if api.network.is_null() {
                return Err("сети нет в этом процессе".to_string());
            }
            ok(
                unsafe { ((*api.network).tell)(handle, who, Str::borrow(&message)) },
                name,
            )
        }
        "note" => {
            let message = text(args, 0, name)?;
            log(LogLevel::Info, &message);
            Ok(Val::Null)
        }
        "command" => {
            // Тупой инструмент, и он здесь именно поэтому: всё, чему в
            // этом словаре не нашлось отдельного имени, делается им.
            // Выполняется с правами оператора — как из консоли.
            let line = text(args, 0, name)?;
            if api.core.is_null() {
                return Err("ядра нет".to_string());
            }
            ok(
                unsafe { ((*api.core).run_command)(handle, Str::borrow(&line)) },
                name,
            )
        }

        // ------------------------------------------------ время, погода
        "tick" => Ok(Val::Int(unsafe { ((*api.core).tick)(handle) } as i64)),
        "time" => Ok(Val::Num(f64::from(unsafe {
            ((*api.core).time_of_day)(handle)
        }))),
        "set_time" => ok(
            unsafe { ((*api.core).set_time_of_day)(handle, num(args, 0, name)? as f32) },
            name,
        ),
        "day_length" => Ok(Val::Num(f64::from(unsafe {
            ((*api.core).day_length_seconds)(handle)
        }))),
        "tick_rate" => Ok(Val::Num(f64::from(unsafe {
            ((*api.core).tick_rate_hz)(handle)
        }))),
        "weather" => Ok(Val::Text(
            weather_name(unsafe { ((*api.world).weather)(handle) }).to_string(),
        )),
        "set_weather" => {
            let wanted = weather_by_name(&text(args, 0, name)?)?;
            ok(unsafe { ((*api.world).set_weather)(handle, wanted) }, name)
        }

        // ---------------------------------------------------------- мир
        "block_at" => {
            let at = cell(args, 0, name)?;
            let mut id: u16 = 0;
            let status = unsafe { ((*api.world).get_block)(handle, at, &mut id) };
            // Незагруженный чанк — это `null`, а не ошибка: скрипт,
            // обходящий большую область, иначе падал бы на её краю.
            Ok(if status.is_ok() {
                Val::Int(i64::from(id))
            } else {
                Val::Null
            })
        }
        "place" => {
            let at = cell(args, 0, name)?;
            let what = block(args, 3, name)?;
            ok(unsafe { ((*api.world).set_block)(handle, at, what) }, name)
        }
        "dig" => {
            let at = cell(args, 0, name)?;
            // По умолчанию — как удар игрока: с выпадением. Стереть
            // начисто можно, но об этом надо попросить.
            let drop = flag(args, 3, true);
            ok(
                unsafe { ((*api.world).break_block)(handle, at, drop) },
                name,
            )
        }
        "fill_blocks" => {
            let from = cell(args, 0, name)?;
            let to = cell(args, 3, name)?;
            let what = block(args, 6, name)?;
            let mut written: u32 = 0;
            let status = unsafe { ((*api.world).fill)(handle, from, to, what, &mut written) };
            if status.is_ok() {
                Ok(Val::Int(i64::from(written)))
            } else {
                Err(format!("{}: {}", name, status_name(status)))
            }
        }
        "disturb" => ok(
            unsafe { ((*api.world).disturb)(handle, cell(args, 0, name)?) },
            name,
        ),
        "surface" => {
            let x = int(args, 0, name)? as i32;
            let z = int(args, 1, name)? as i32;
            let mut y: i32 = 0;
            let status = unsafe { ((*api.world).surface_at)(handle, x, z, &mut y) };
            Ok(if status.is_ok() {
                Val::Int(i64::from(y))
            } else {
                Val::Null
            })
        }
        "terrain_height" => {
            let x = int(args, 0, name)? as i32;
            let z = int(args, 1, name)? as i32;
            Ok(Val::Int(i64::from(unsafe {
                ((*api.generation).height_at)(handle, x, z)
            })))
        }
        "biome" => {
            let x = int(args, 0, name)? as i32;
            let z = int(args, 1, name)? as i32;
            let index = unsafe { ((*api.generation).biome_at)(handle, x, z) };
            let named = read_text(|out, cap, written| unsafe {
                ((*api.generation).biome_name)(handle, index, out, cap, written)
            });
            Ok(named.map(Val::Text).unwrap_or(Val::Int(i64::from(index))))
        }
        "climate" => {
            let at = cell(args, 0, name)?;
            let mut warmth = 0.0f32;
            let mut humidity = 0.0f32;
            let status =
                unsafe { ((*api.generation).climate_at)(handle, at, &mut warmth, &mut humidity) };
            if !status.is_ok() {
                return Err(format!("{}: {}", name, status_name(status)));
            }
            Ok(Val::Map(vec![
                ("warmth", Val::Num(f64::from(warmth))),
                ("humidity", Val::Num(f64::from(humidity))),
            ]))
        }
        "temperature" => {
            let at = point(args, 0, name)?;
            let mut degrees = 0.0f32;
            let status = unsafe { ((*api.world).temperature_at)(handle, at, &mut degrees) };
            if status.is_ok() {
                Ok(Val::Num(f64::from(degrees)))
            } else {
                Err(format!("{}: {}", name, status_name(status)))
            }
        }
        "world_seed" => Ok(Val::Int(i64::from(unsafe {
            ((*api.world).world_seed)(handle)
        }))),
        "spawn_point" => Ok(vec3_list(unsafe { ((*api.world).spawn_point)(handle) })),
        "chunk_loaded" => {
            let at = ChunkPos {
                x: int(args, 0, name)? as i32,
                z: int(args, 1, name)? as i32,
            };
            Ok(Val::Bool(unsafe {
                ((*api.world).is_chunk_loaded)(handle, at)
            }))
        }
        "load_chunk" => {
            let at = ChunkPos {
                x: int(args, 0, name)? as i32,
                z: int(args, 1, name)? as i32,
            };
            ok(unsafe { ((*api.world).request_chunk)(handle, at) }, name)
        }
        "chunk_count" => Ok(Val::Int(i64::from(unsafe {
            ((*api.world).loaded_chunk_count)(handle)
        }))),
        "save_world" => {
            if api.save.is_null() {
                return Err("сохранения нет".to_string());
            }
            ok(unsafe { ((*api.save).save_world)(handle) }, name)
        }

        // --------------------------------------------------------- свет
        "sky_light" | "block_light" => {
            if api.lighting.is_null() {
                return Err("света нет в этом процессе".to_string());
            }
            let at = cell(args, 0, name)?;
            let mut level: u8 = 0;
            let status = if name == "sky_light" {
                unsafe { ((*api.lighting).sky_light)(handle, at, &mut level) }
            } else {
                unsafe { ((*api.lighting).block_light)(handle, at, &mut level) }
            };
            Ok(if status.is_ok() {
                Val::Int(i64::from(level))
            } else {
                Val::Null
            })
        }
        "open_to_sky" => Ok(Val::Bool(unsafe {
            ((*api.lighting).open_to_sky)(handle, cell(args, 0, name)?)
        })),
        "max_light" => Ok(Val::Int(i64::from(unsafe {
            ((*api.lighting).max_light)(handle)
        }))),

        // ----------------------------------------------- таблица блоков
        "block_id" => Ok(block_by_name(&text(args, 0, name)?)
            .map(|id| Val::Int(i64::from(id)))
            .unwrap_or(Val::Null)),
        "block_name" => Ok(block_name(block(args, 0, name)?)
            .map(Val::Text)
            .unwrap_or(Val::Null)),
        "block_count" => Ok(Val::Int(i64::from(unsafe {
            ((*api.blocks).count)(handle)
        }))),
        "block_by_index" => {
            let index = int(args, 0, name)? as u32;
            let mut id: u16 = 0;
            let status = unsafe { ((*api.blocks).id_at)(handle, index, &mut id) };
            Ok(if status.is_ok() {
                Val::Int(i64::from(id))
            } else {
                Val::Null
            })
        }
        "block_info" => {
            let id = block(args, 0, name)?;
            let mut props = BlockProperties {
                id: 0,
                hardness: 0.0,
                opacity: 0,
                emission: 0,
                solid: false,
                placeable: false,
                falls: false,
                container: false,
                weight_kg: 0.0,
                stack_limit: 0,
                drops: 0,
            };
            let status = unsafe { ((*api.blocks).properties)(handle, id, &mut props) };
            if !status.is_ok() {
                return Ok(Val::Null);
            }
            Ok(Val::Map(vec![
                ("id", Val::Int(i64::from(props.id))),
                (
                    "name",
                    block_name(props.id).map(Val::Text).unwrap_or(Val::Null),
                ),
                ("hardness", Val::Num(f64::from(props.hardness))),
                ("opacity", Val::Int(i64::from(props.opacity))),
                ("emission", Val::Int(i64::from(props.emission))),
                ("solid", Val::Bool(props.solid)),
                ("placeable", Val::Bool(props.placeable)),
                ("falls", Val::Bool(props.falls)),
                ("container", Val::Bool(props.container)),
                ("weight", Val::Num(f64::from(props.weight_kg))),
                ("stack_limit", Val::Int(i64::from(props.stack_limit))),
                ("drops", Val::Int(i64::from(props.drops))),
            ]))
        }
        "block_tools" => {
            let id = block(args, 0, name)?;
            let mut tooling = BlockTooling {
                needs: primitive_modapi::Tier::Hand,
                work: primitive_modapi::Work::Any,
                tool: primitive_modapi::Tier::Hand,
                is_tool: false,
                durability: 0,
                felled: 0.0,
                leaves_behind: 0,
                drag: 1.0,
                grip: 1.0,
                thickness: 0,
            };
            let status = unsafe { ((*api.blocks).tooling)(handle, id, &mut tooling) };
            if !status.is_ok() {
                return Ok(Val::Null);
            }
            Ok(Val::Map(vec![
                ("needs", Val::Int(tooling.needs as i64)),
                ("work", Val::Int(tooling.work as i64)),
                ("tool", Val::Int(tooling.tool as i64)),
                ("is_tool", Val::Bool(tooling.is_tool)),
                ("durability", Val::Int(i64::from(tooling.durability))),
                ("drag", Val::Num(f64::from(tooling.drag))),
                ("grip", Val::Num(f64::from(tooling.grip))),
                ("thickness", Val::Int(i64::from(tooling.thickness))),
                (
                    "leaves_behind",
                    Val::Int(i64::from(tooling.leaves_behind)),
                ),
            ]))
        }
        "break_time" => {
            let what = block(args, 0, name)?;
            let tool = match args.get(1) {
                Some(_) => block(args, 1, name)?,
                None => 0,
            };
            let mut seconds = 0.0f32;
            let status = unsafe { ((*api.blocks).break_seconds)(handle, what, tool, &mut seconds) };
            Ok(if status.is_ok() {
                Val::Num(f64::from(seconds))
            } else {
                // Отказ — это «этим инструментом в этот блок не войти»,
                // и это ответ, а не поломка.
                Val::Null
            })
        }
        "same_kind" => Ok(Val::Bool(unsafe {
            ((*api.blocks).same_kind)(handle, block(args, 0, name)?, block(args, 1, name)?)
        })),
        "is_solid" => Ok(Val::Bool(unsafe {
            ((*api.physics).is_solid)(handle, block(args, 0, name)?)
        })),
        "liquid_depth" => Ok(Val::Num(f64::from(unsafe {
            ((*api.physics).liquid_depth)(handle, block(args, 0, name)?)
        }))),

        // ------------------------------------------------------- игроки
        "players" => Ok(Val::List(
            all_players().into_iter().map(|id| Val::Int(id as i64)).collect(),
        )),
        "player_name" => Ok(player_name(player(session, args, 0, name)?)
            .map(Val::Text)
            .unwrap_or(Val::Null)),
        "find_player" => Ok(find_player(&text(args, 0, name)?)
            .map(|id| Val::Int(id as i64))
            .unwrap_or(Val::Null)),
        "pos" => {
            let who = player(session, args, 0, name)?;
            let mut at = Vec3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            };
            let status = unsafe { ((*api.players).position)(handle, who, &mut at) };
            Ok(if status.is_ok() {
                vec3_list(at)
            } else {
                Val::Null
            })
        }
        "look" => {
            let who = player(session, args, 0, name)?;
            let mut direction = Vec3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            };
            let status = unsafe { ((*api.players).look)(handle, who, &mut direction) };
            Ok(if status.is_ok() {
                vec3_list(direction)
            } else {
                Val::Null
            })
        }
        "teleport" => {
            let who = player(session, args, 0, name)?;
            let to = point(args, 1, name)?;
            ok(unsafe { ((*api.players).teleport)(handle, who, to) }, name)
        }
        "vitals" => {
            let who = player(session, args, 0, name)?;
            let mut vitals = PlayerVitals {
                health: 0.0,
                max_health: 0.0,
                nourishment: 0.0,
                hydration: 0.0,
                breath: 0.0,
                body_temperature_c: 0.0,
                ambient_c: 0.0,
                wetness: 0.0,
                carried_kg: 0.0,
                dead: false,
            };
            let status = unsafe { ((*api.players).vitals)(handle, who, &mut vitals) };
            if !status.is_ok() {
                return Ok(Val::Null);
            }
            Ok(Val::Map(vec![
                ("health", Val::Num(f64::from(vitals.health))),
                ("max_health", Val::Num(f64::from(vitals.max_health))),
                ("nourishment", Val::Num(f64::from(vitals.nourishment))),
                ("hydration", Val::Num(f64::from(vitals.hydration))),
                ("breath", Val::Num(f64::from(vitals.breath))),
                // Температура **тела**, а не воздуха вокруг: живое тело
                // теплее среды, и это разные числа. Второе рядом.
                ("body_c", Val::Num(f64::from(vitals.body_temperature_c))),
                ("ambient_c", Val::Num(f64::from(vitals.ambient_c))),
                ("wetness", Val::Num(f64::from(vitals.wetness))),
                ("carried_kg", Val::Num(f64::from(vitals.carried_kg))),
                ("dead", Val::Bool(vitals.dead)),
            ]))
        }
        "hurt" => {
            let who = player(session, args, 0, name)?;
            let amount = num(args, 1, name)? as f32;
            let cause = args
                .get(2)
                .and_then(|_| text(args, 2, name).ok())
                .unwrap_or_else(|| "скрипт".to_string());
            ok(
                unsafe { ((*api.players).damage)(handle, who, amount, Str::borrow(&cause)) },
                name,
            )
        }
        "cure" => {
            let who = player(session, args, 0, name)?;
            ok(
                unsafe { ((*api.players).heal)(handle, who, num(args, 1, name)? as f32) },
                name,
            )
        }
        "set_health" => {
            let who = player(session, args, 0, name)?;
            ok(
                unsafe { ((*api.players).set_health)(handle, who, num(args, 1, name)? as f32) },
                name,
            )
        }
        "feed" => {
            let who = player(session, args, 0, name)?;
            ok(
                unsafe { ((*api.players).feed)(handle, who, num(args, 1, name)? as f32) },
                name,
            )
        }
        "drink" => {
            let who = player(session, args, 0, name)?;
            ok(
                unsafe { ((*api.players).water)(handle, who, num(args, 1, name)? as f32) },
                name,
            )
        }
        "warm" => {
            let who = player(session, args, 0, name)?;
            let degrees = num(args, 1, name)? as f32;
            let wetness = opt_num(args, 2).unwrap_or(0.0) as f32;
            ok(
                unsafe { ((*api.players).set_warmth)(handle, who, degrees, wetness) },
                name,
            )
        }
        "kick" => {
            let who = player(session, args, 0, name)?;
            let reason = text(args, 1, name).unwrap_or_else(|_| "скрипт".to_string());
            ok(
                unsafe { ((*api.players).kick)(handle, who, Str::borrow(&reason)) },
                name,
            )
        }
        "is_operator" => Ok(Val::Bool(is_operator(player(session, args, 0, name)?))),
        "fly" => {
            let who = player(session, args, 0, name)?;
            let on = flag(args, 1, true);
            let speed = opt_num(args, 2).unwrap_or(0.0) as f32;
            ok(
                unsafe { ((*api.players).set_flying)(handle, who, on, speed) },
                name,
            )
        }
        "is_flying" => Ok(Val::Bool(unsafe {
            ((*api.players).is_flying)(handle, player(session, args, 0, name)?)
        })),
        "on_ground" => Ok(Val::Bool(unsafe {
            ((*api.players).on_ground)(handle, player(session, args, 0, name)?)
        })),
        "submerged" => Ok(Val::Bool(unsafe {
            ((*api.players).is_submerged)(handle, player(session, args, 0, name)?)
        })),
        "respawn" => ok(
            unsafe { ((*api.players).respawn)(handle, player(session, args, 0, name)?) },
            name,
        ),
        "selected" => {
            let who = player(session, args, 0, name)?;
            let mut slot: u32 = 0;
            let status = unsafe { ((*api.players).selected_slot)(handle, who, &mut slot) };
            Ok(if status.is_ok() {
                Val::Int(i64::from(slot))
            } else {
                Val::Null
            })
        }
        "select" => {
            let who = player(session, args, 0, name)?;
            let slot = int(args, 1, name)? as u32;
            ok(
                unsafe { ((*api.players).set_selected_slot)(handle, who, slot) },
                name,
            )
        }

        // ---------------------------------------------------- инвентарь
        "held" => {
            let who = player(session, args, 0, name)?;
            let mut stack = ItemStack::EMPTY;
            let status = unsafe { ((*api.inventory).held)(handle, who, &mut stack) };
            Ok(if status.is_ok() && stack.count > 0 {
                stack_map(stack)
            } else {
                Val::Null
            })
        }
        "give" => {
            let who = player(session, args, 0, name)?;
            let what = block(args, 1, name)?;
            let count = opt_num(args, 2).unwrap_or(1.0) as u32;
            let mut left: u32 = 0;
            let stack = ItemStack {
                block: what,
                count,
                damage: 0,
            };
            let status = unsafe { ((*api.inventory).give)(handle, who, stack, &mut left) };
            if !status.is_ok() {
                return Err(format!("{}: {}", name, status_name(status)));
            }
            // Сколько **не** влезло: класть в рюкзак, который полон,
            // скрипт будет по одному, и ему нужен именно этот остаток.
            Ok(Val::Int(i64::from(left)))
        }
        "take" => {
            let who = player(session, args, 0, name)?;
            let what = block(args, 1, name)?;
            let count = opt_num(args, 2).unwrap_or(1.0) as u32;
            let mut taken: u32 = 0;
            let status = unsafe { ((*api.inventory).take)(handle, who, what, count, &mut taken) };
            if !status.is_ok() {
                return Err(format!("{}: {}", name, status_name(status)));
            }
            Ok(Val::Int(i64::from(taken)))
        }
        "carrying" => {
            let who = player(session, args, 0, name)?;
            let what = block(args, 1, name)?;
            let mut count: u32 = 0;
            let status = unsafe { ((*api.inventory).count_of)(handle, who, what, &mut count) };
            if !status.is_ok() {
                return Err(format!("{}: {}", name, status_name(status)));
            }
            Ok(Val::Int(i64::from(count)))
        }
        "slot_at" => {
            let who = player(session, args, 0, name)?;
            let slot = int(args, 1, name)? as u32;
            let mut stack = ItemStack::EMPTY;
            let status = unsafe { ((*api.inventory).get_slot)(handle, who, slot, &mut stack) };
            Ok(if status.is_ok() && stack.count > 0 {
                stack_map(stack)
            } else {
                Val::Null
            })
        }
        "put_slot" => {
            let who = player(session, args, 0, name)?;
            let slot = int(args, 1, name)? as u32;
            let what = block(args, 2, name)?;
            let count = opt_num(args, 3).unwrap_or(1.0) as u32;
            let stack = ItemStack {
                block: what,
                count,
                damage: 0,
            };
            ok(
                unsafe { ((*api.inventory).set_slot)(handle, who, slot, stack) },
                name,
            )
        }
        "throw_slot" => {
            let who = player(session, args, 0, name)?;
            let slot = int(args, 1, name)? as u32;
            let whole = flag(args, 2, true);
            ok(
                unsafe { ((*api.inventory).drop_slot)(handle, who, slot, whole) },
                name,
            )
        }
        "spill" => {
            let who = player(session, args, 0, name)?;
            let mut dropped: u32 = 0;
            let status = unsafe { ((*api.inventory).spill)(handle, who, &mut dropped) };
            if !status.is_ok() {
                return Err(format!("{}: {}", name, status_name(status)));
            }
            Ok(Val::Int(i64::from(dropped)))
        }
        "slot_count" => Ok(Val::Int(i64::from(unsafe {
            ((*api.inventory).slot_count)(handle)
        }))),
        "used_slots" => {
            let who = player(session, args, 0, name)?;
            let mut used: u32 = 0;
            let status = unsafe { ((*api.inventory).used_slots)(handle, who, &mut used) };
            if !status.is_ok() {
                return Err(format!("{}: {}", name, status_name(status)));
            }
            Ok(Val::Int(i64::from(used)))
        }
        "worn" => {
            let who = player(session, args, 0, name)?;
            let slot = body_slot(&text(args, 1, name)?)?;
            let mut stack = ItemStack::EMPTY;
            let status =
                unsafe { ((*api.inventory).get_equipment)(handle, who, slot, &mut stack) };
            Ok(if status.is_ok() && stack.count > 0 {
                stack_map(stack)
            } else {
                Val::Null
            })
        }
        "wear" => {
            let who = player(session, args, 0, name)?;
            let slot = body_slot(&text(args, 1, name)?)?;
            let what = block(args, 2, name)?;
            let stack = ItemStack {
                block: what,
                count: 1,
                damage: 0,
            };
            ok(
                unsafe { ((*api.inventory).set_equipment)(handle, who, slot, stack) },
                name,
            )
        }

        // ----------------------------------------------------- животные
        "entities" => {
            let count = unsafe { ((*api.entities).count)(handle) } as usize;
            let mut buffer = vec![0u64; count.max(1)];
            let mut written: usize = 0;
            let status = unsafe {
                ((*api.entities).all)(handle, buffer.as_mut_ptr(), buffer.len(), &mut written)
            };
            if !status.is_ok() {
                return Err(format!("{}: {}", name, status_name(status)));
            }
            buffer.truncate(written);
            Ok(Val::List(
                buffer.into_iter().map(|id| Val::Int(id as i64)).collect(),
            ))
        }
        "entities_near" => {
            let at = point(args, 0, name)?;
            let radius = num(args, 3, name)? as f32;
            let mut buffer = vec![0u64; 1024];
            let mut written: usize = 0;
            let status = unsafe {
                ((*api.entities).near)(
                    handle,
                    at,
                    radius,
                    buffer.as_mut_ptr(),
                    buffer.len(),
                    &mut written,
                )
            };
            if !status.is_ok() {
                return Err(format!("{}: {}", name, status_name(status)));
            }
            buffer.truncate(written.min(buffer.len()));
            Ok(Val::List(
                buffer.into_iter().map(|id| Val::Int(id as i64)).collect(),
            ))
        }
        "entity_count" => Ok(Val::Int(i64::from(unsafe {
            ((*api.entities).count)(handle)
        }))),
        "entity_pos" => {
            let id = int(args, 0, name)? as u64;
            let mut at = Vec3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            };
            let status = unsafe { ((*api.entities).position)(handle, id, &mut at) };
            Ok(if status.is_ok() {
                vec3_list(at)
            } else {
                Val::Null
            })
        }
        "entity_health" => {
            let id = int(args, 0, name)? as u64;
            let mut health = 0.0f32;
            let status = unsafe { ((*api.entities).health)(handle, id, &mut health) };
            Ok(if status.is_ok() {
                Val::Num(f64::from(health))
            } else {
                Val::Null
            })
        }
        "entity_species" => {
            let id = int(args, 0, name)? as u64;
            let mut index: u32 = 0;
            let status = unsafe { ((*api.entities).species)(handle, id, &mut index) };
            if !status.is_ok() {
                return Ok(Val::Null);
            }
            Ok(species_name(index).map(Val::Text).unwrap_or(Val::Int(i64::from(index))))
        }
        "species_name" => Ok(species_name(int(args, 0, name)? as u32)
            .map(Val::Text)
            .unwrap_or(Val::Null)),
        "species_id" => Ok(species_by_name(&text(args, 0, name)?)
            .map(|i| Val::Int(i64::from(i)))
            .unwrap_or(Val::Null)),
        "species_count" => Ok(Val::Int(i64::from(unsafe {
            ((*api.entities).species_count)(handle)
        }))),
        "species_info" => {
            let index = species(args, 0, name)?;
            let mut info = SpeciesInfo {
                max_health: 0.0,
                damage: 0.0,
                walk_speed: 0.0,
                run_speed: 0.0,
                hostile: false,
                awareness: 0.0,
                provoke_range: 0.0,
                height: 0.0,
                width: 0.0,
                length: 0.0,
                drop_count: 0,
            };
            let status = unsafe { ((*api.entities).species_info)(handle, index, &mut info) };
            if !status.is_ok() {
                return Ok(Val::Null);
            }
            let mut drops = Vec::new();
            for which in 0..info.drop_count {
                let mut stack = ItemStack::EMPTY;
                let status =
                    unsafe { ((*api.entities).species_drop)(handle, index, which, &mut stack) };
                if status.is_ok() {
                    drops.push(stack_map(stack));
                }
            }
            Ok(Val::Map(vec![
                (
                    "name",
                    species_name(index).map(Val::Text).unwrap_or(Val::Null),
                ),
                ("max_health", Val::Num(f64::from(info.max_health))),
                ("damage", Val::Num(f64::from(info.damage))),
                ("walk_speed", Val::Num(f64::from(info.walk_speed))),
                ("run_speed", Val::Num(f64::from(info.run_speed))),
                ("hostile", Val::Bool(info.hostile)),
                ("awareness", Val::Num(f64::from(info.awareness))),
                ("provoke_range", Val::Num(f64::from(info.provoke_range))),
                ("height", Val::Num(f64::from(info.height))),
                ("width", Val::Num(f64::from(info.width))),
                ("length", Val::Num(f64::from(info.length))),
                ("drops", Val::List(drops)),
            ]))
        }
        "spawn" => {
            let which = species(args, 0, name)?;
            let at = point(args, 1, name)?;
            let mut id: u64 = 0;
            let status = unsafe { ((*api.entities).spawn)(handle, which, at, &mut id) };
            if !status.is_ok() {
                return Err(format!("{}: {}", name, status_name(status)));
            }
            Ok(Val::Int(id as i64))
        }
        "kill" => ok(
            unsafe { ((*api.entities).kill)(handle, int(args, 0, name)? as u64) },
            name,
        ),
        "despawn" => ok(
            unsafe { ((*api.entities).remove)(handle, int(args, 0, name)? as u64) },
            name,
        ),
        "hurt_entity" => ok(
            unsafe {
                ((*api.entities).damage)(
                    handle,
                    int(args, 0, name)? as u64,
                    num(args, 1, name)? as f32,
                )
            },
            name,
        ),
        "heal_entity" => ok(
            unsafe {
                ((*api.entities).heal)(
                    handle,
                    int(args, 0, name)? as u64,
                    num(args, 1, name)? as f32,
                )
            },
            name,
        ),

        // ------------------------------------------------ вещи на земле
        "drop_item" => {
            let at = point(args, 0, name)?;
            let what = block(args, 3, name)?;
            let count = opt_num(args, 4).unwrap_or(1.0) as u32;
            let stack = ItemStack {
                block: what,
                count,
                damage: 0,
            };
            let still = Vec3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            };
            let mut id: u64 = 0;
            let status = unsafe { ((*api.items).spawn)(handle, at, still, stack, &mut id) };
            if !status.is_ok() {
                return Err(format!("{}: {}", name, status_name(status)));
            }
            Ok(Val::Int(id as i64))
        }
        "items_near" => {
            let at = point(args, 0, name)?;
            let radius = num(args, 3, name)? as f32;
            let mut buffer = vec![ItemStack::EMPTY; 512];
            let mut written: usize = 0;
            let status = unsafe {
                ((*api.items).near)(
                    handle,
                    at,
                    radius,
                    buffer.as_mut_ptr(),
                    buffer.len(),
                    &mut written,
                )
            };
            if !status.is_ok() {
                return Err(format!("{}: {}", name, status_name(status)));
            }
            buffer.truncate(written.min(buffer.len()));
            Ok(Val::List(buffer.into_iter().map(stack_map).collect()))
        }
        "clear_items" => {
            let at = point(args, 0, name)?;
            let radius = num(args, 3, name)? as f32;
            let mut removed: u32 = 0;
            let status = unsafe { ((*api.items).clear_near)(handle, at, radius, &mut removed) };
            if !status.is_ok() {
                return Err(format!("{}: {}", name, status_name(status)));
            }
            Ok(Val::Int(i64::from(removed)))
        }
        "item_count" => Ok(Val::Int(i64::from(unsafe {
            ((*api.items).count)(handle)
        }))),

        // ------------------------------------------------ движение и бой
        "raycast" => {
            let from = point(args, 0, name)?;
            let direction = point(args, 3, name)?;
            let distance = opt_num(args, 6).unwrap_or(8.0) as f32;
            let mut hit = BlockPos { x: 0, y: 0, z: 0 };
            let mut normal = BlockPos { x: 0, y: 0, z: 0 };
            let status = unsafe {
                ((*api.physics).raycast)(handle, from, direction, distance, &mut hit, &mut normal)
            };
            if !status.is_ok() {
                return Ok(Val::Null);
            }
            Ok(Val::Map(vec![
                ("hit", cell_list(hit)),
                ("normal", cell_list(normal)),
            ]))
        }
        "rules" => Ok(Val::Map(vec![
            (
                "gravity",
                Val::Num(f64::from(unsafe { ((*api.physics).gravity)(handle) })),
            ),
            (
                "walk_speed",
                Val::Num(f64::from(unsafe { ((*api.physics).walk_speed)(handle) })),
            ),
            (
                "sprint_speed",
                Val::Num(f64::from(unsafe { ((*api.physics).sprint_speed)(handle) })),
            ),
            (
                "jump_speed",
                Val::Num(f64::from(unsafe { ((*api.physics).jump_speed)(handle) })),
            ),
            (
                "terminal_velocity",
                Val::Num(f64::from(unsafe {
                    ((*api.physics).terminal_velocity)(handle)
                })),
            ),
            (
                "carry_capacity_kg",
                Val::Num(f64::from(unsafe {
                    ((*api.physics).carry_capacity_kg)(handle)
                })),
            ),
            (
                "safe_fall_blocks",
                Val::Num(f64::from(unsafe {
                    ((*api.physics).safe_fall_blocks)(handle)
                })),
            ),
            (
                "melee_reach",
                Val::Num(f64::from(unsafe { ((*api.combat).melee_reach)(handle) })),
            ),
            (
                "melee_damage",
                Val::Num(f64::from(unsafe { ((*api.combat).melee_damage)(handle) })),
            ),
            (
                "melee_cooldown",
                Val::Num(f64::from(unsafe {
                    ((*api.combat).melee_cooldown_seconds)(handle)
                })),
            ),
        ])),
        "within_reach" => {
            let from = point(args, 0, name)?;
            let to = point(args, 3, name)?;
            Ok(Val::Bool(unsafe {
                ((*api.combat).within_reach)(handle, from, to)
            }))
        }
        "weapon_damage" => {
            let what = block(args, 0, name)?;
            let mut damage = 0.0f32;
            let status = unsafe { ((*api.combat).weapon_damage)(handle, what, &mut damage) };
            Ok(if status.is_ok() {
                Val::Num(f64::from(damage))
            } else {
                Val::Null
            })
        }

        // -------------------------------------------------------- вода
        "is_liquid" => Ok(Val::Bool(unsafe {
            ((*api.fluid).is_liquid)(handle, block(args, 0, name)?)
        })),
        "is_source" => Ok(Val::Bool(unsafe {
            ((*api.fluid).is_source)(handle, block(args, 0, name)?)
        })),
        "water_place" => ok(
            unsafe { ((*api.fluid).place_source)(handle, cell(args, 0, name)?) },
            name,
        ),
        "water_take" => ok(
            unsafe { ((*api.fluid).remove)(handle, cell(args, 0, name)?) },
            name,
        ),
        "water_depth" => {
            let at = cell(args, 0, name)?;
            let mut depth = 0.0f32;
            let status = unsafe { ((*api.fluid).column_depth)(handle, at, &mut depth) };
            Ok(if status.is_ok() {
                Val::Num(f64::from(depth))
            } else {
                Val::Null
            })
        }

        // --------------------------------------------------- хранилища
        "container_slots" => {
            let at = cell(args, 0, name)?;
            let mut count: u32 = 0;
            let status = unsafe { ((*api.containers).slot_count)(handle, at, &mut count) };
            Ok(if status.is_ok() {
                Val::Int(i64::from(count))
            } else {
                Val::Null
            })
        }
        "container_get" => {
            let at = cell(args, 0, name)?;
            let slot = int(args, 3, name)? as u32;
            let mut stack = ItemStack::EMPTY;
            let status = unsafe { ((*api.containers).get_slot)(handle, at, slot, &mut stack) };
            Ok(if status.is_ok() && stack.count > 0 {
                stack_map(stack)
            } else {
                Val::Null
            })
        }
        "container_put" => {
            let at = cell(args, 0, name)?;
            let slot = int(args, 3, name)? as u32;
            let what = block(args, 4, name)?;
            let count = opt_num(args, 5).unwrap_or(1.0) as u32;
            let stack = ItemStack {
                block: what,
                count,
                damage: 0,
            };
            ok(
                unsafe { ((*api.containers).set_slot)(handle, at, slot, stack) },
                name,
            )
        }
        "container_take" => {
            let at = cell(args, 0, name)?;
            let what = block(args, 3, name)?;
            let count = opt_num(args, 4).unwrap_or(1.0) as u32;
            let mut taken: u32 = 0;
            let status =
                unsafe { ((*api.containers).take)(handle, at, what, count, &mut taken) };
            if !status.is_ok() {
                return Err(format!("{}: {}", name, status_name(status)));
            }
            Ok(Val::Int(i64::from(taken)))
        }
        "container_spill" => ok(
            unsafe { ((*api.containers).spill)(handle, cell(args, 0, name)?) },
            name,
        ),
        "containers" => {
            let mut buffer = vec![BlockPos { x: 0, y: 0, z: 0 }; 4096];
            let mut written: usize = 0;
            let status = unsafe {
                ((*api.containers).all)(handle, buffer.as_mut_ptr(), buffer.len(), &mut written)
            };
            if !status.is_ok() {
                return Err(format!("{}: {}", name, status_name(status)));
            }
            buffer.truncate(written.min(buffer.len()));
            Ok(Val::List(buffer.into_iter().map(cell_list).collect()))
        }

        // ------------------------------------------------- огонь и печи
        "light_fire" => ok(
            unsafe { ((*api.stations).light_fire)(handle, cell(args, 0, name)?) },
            name,
        ),
        "feed_fire" => {
            let at = cell(args, 0, name)?;
            let seconds = num(args, 3, name)? as f32;
            ok(
                unsafe { ((*api.stations).feed_fire)(handle, at, seconds) },
                name,
            )
        }
        "extinguish" => ok(
            unsafe { ((*api.stations).extinguish)(handle, cell(args, 0, name)?) },
            name,
        ),
        "fuel_left" => {
            let at = cell(args, 0, name)?;
            let mut seconds = 0.0f32;
            let status = unsafe { ((*api.stations).fire_fuel_left)(handle, at, &mut seconds) };
            Ok(if status.is_ok() {
                Val::Num(f64::from(seconds))
            } else {
                Val::Null
            })
        }
        "fuel_of" => {
            let what = block(args, 0, name)?;
            let mut seconds = 0.0f32;
            let status = unsafe { ((*api.stations).fuel_seconds)(handle, what, &mut seconds) };
            Ok(if status.is_ok() {
                Val::Num(f64::from(seconds))
            } else {
                Val::Null
            })
        }
        "fire_near" => {
            let at = point(args, 0, name)?;
            let range = num(args, 3, name)? as f32;
            Ok(Val::Bool(unsafe {
                ((*api.stations).fire_within)(handle, at, range)
            }))
        }
        "smelting" | "drying" => {
            let at = cell(args, 0, name)?;
            let mut progress = 0.0f32;
            let status = if name == "smelting" {
                unsafe { ((*api.stations).smelting_progress)(handle, at, &mut progress) }
            } else {
                unsafe { ((*api.stations).drying_progress)(handle, at, &mut progress) }
            };
            Ok(if status.is_ok() {
                Val::Num(f64::from(progress))
            } else {
                Val::Null
            })
        }
        "cures_into" => {
            let what = block(args, 0, name)?;
            let mut into: u16 = 0;
            let status = unsafe { ((*api.stations).cures_into)(handle, what, &mut into) };
            Ok(if status.is_ok() {
                Val::Int(i64::from(into))
            } else {
                Val::Null
            })
        }

        // --------------------------------------------------------- еда
        "is_food" => Ok(Val::Bool(unsafe {
            ((*api.food).is_food)(handle, block(args, 0, name)?)
        })),
        "nutrition" => {
            let what = block(args, 0, name)?;
            let mut value = 0.0f32;
            let status = unsafe { ((*api.food).nutrition)(handle, what, &mut value) };
            Ok(if status.is_ok() {
                Val::Num(f64::from(value))
            } else {
                Val::Null
            })
        }
        "eat" => {
            let who = player(session, args, 0, name)?;
            let slot = int(args, 1, name)? as u32;
            ok(unsafe { ((*api.food).eat)(handle, who, slot) }, name)
        }

        // ----------------------------------------------------- рецепты
        "recipe_count" => Ok(Val::Int(i64::from(unsafe {
            ((*api.crafting).recipe_count)(handle)
        }))),
        "recipe_name" => {
            let index = int(args, 0, name)? as u32;
            Ok(read_text(|out, cap, written| unsafe {
                ((*api.crafting).recipe_name)(handle, index, out, cap, written)
            })
            .map(Val::Text)
            .unwrap_or(Val::Null))
        }
        "recipe" => {
            let index = int(args, 0, name)? as u32;
            let mut info = RecipeInfo {
                output: 0,
                output_count: 0,
                station: primitive_modapi::Station::Hands,
                input_count: 0,
                return_count: 0,
            };
            let status = unsafe { ((*api.crafting).recipe)(handle, index, &mut info) };
            if !status.is_ok() {
                return Ok(Val::Null);
            }
            let mut inputs = Vec::new();
            for which in 0..info.input_count {
                let mut stack = ItemStack::EMPTY;
                if unsafe { ((*api.crafting).recipe_input)(handle, index, which, &mut stack) }
                    .is_ok()
                {
                    inputs.push(stack_map(stack));
                }
            }
            let mut returns = Vec::new();
            for which in 0..info.return_count {
                let mut stack = ItemStack::EMPTY;
                if unsafe { ((*api.crafting).recipe_return)(handle, index, which, &mut stack) }
                    .is_ok()
                {
                    returns.push(stack_map(stack));
                }
            }
            Ok(Val::Map(vec![
                ("index", Val::Int(i64::from(index))),
                (
                    "name",
                    read_text(|out, cap, written| unsafe {
                        ((*api.crafting).recipe_name)(handle, index, out, cap, written)
                    })
                    .map(Val::Text)
                    .unwrap_or(Val::Null),
                ),
                ("output", Val::Int(i64::from(info.output))),
                ("output_count", Val::Int(i64::from(info.output_count))),
                ("station", Val::Int(info.station as i64)),
                ("inputs", Val::List(inputs)),
                ("returns", Val::List(returns)),
            ]))
        }
        "can_craft" => {
            let who = player(session, args, 0, name)?;
            let index = int(args, 1, name)? as u32;
            let mut answer = Feasibility::Ready;
            let status = unsafe { ((*api.crafting).feasibility)(handle, who, index, &mut answer) };
            if !status.is_ok() {
                return Err(format!("{}: {}", name, status_name(status)));
            }
            Ok(Val::Text(feasibility_name(answer).to_string()))
        }
        "craft" => {
            let who = player(session, args, 0, name)?;
            let index = int(args, 1, name)? as u32;
            let times = opt_num(args, 2).unwrap_or(1.0) as u32;
            let mut made: u32 = 0;
            let status = unsafe { ((*api.crafting).craft)(handle, who, index, times, &mut made) };
            if !status.is_ok() {
                return Err(format!("{}: {}", name, status_name(status)));
            }
            Ok(Val::Int(i64::from(made)))
        }

        // ------------------------------------------- мир сам по себе
        "growth_pending" => Ok(Val::Int(i64::from(unsafe {
            ((*api.simulation).growth_pending)(handle)
        }))),
        "watch_growth" => ok(
            unsafe { ((*api.simulation).watch_growth)(handle, point(args, 0, name)?) },
            name,
        ),
        "is_trunk" => Ok(Val::Bool(unsafe {
            ((*api.simulation).is_standing_trunk)(handle, block(args, 0, name)?)
        })),

        other => Err(format!("игра не знает функции '{}'", other)),
    }
}

// =====================================================================
// Скриптовые моды
// =====================================================================

/// Один обработчик: как он называется в скрипте и что в игре ему
/// соответствует.
struct Handler {
    /// Имя функции, которую объявляет скрипт.
    name: &'static str,
    event: Event,
    /// Что решает возвращённое значение.
    decides: Decides,
}

/// Что скрипт может решить, вернув значение из обработчика.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Decides {
    /// Ничего: событие уже произошло, а возвращённое значение —
    /// вежливость.
    Nothing,
    /// `false` — не давать. Только у тех событий, которые игра сама
    /// объявляет отменяемыми: отмена «смерти игрока» должна быть
    /// пустышкой, а не исключением.
    Cancels,
    /// `true` — «я это сделал». Единственное место, где смысл
    /// обратный, и оно ровно одно: у команды нечего отменять, а вот
    /// сказать «дальше не ищите» надо.
    Claims,
}

/// Имена, которые зовёт не событие, а сам загрузчик.
const ON_LOAD: &str = "on_load";
const ON_UNLOAD: &str = "on_unload";

/// Всё, на что скрипт может отозваться.
///
/// Одна строка — одно событие; двух обработчиков на событие нет, и
/// поиск по таблице идёт в обе стороны. Порядок — как у [`Event`], а
/// не по алфавиту: искать здесь будут по событию, а не по букве.
const HANDLERS: &[Handler] = &[
    Handler { name: "on_start", event: Event::ServerStarted, decides: Decides::Nothing },
    Handler { name: "on_stop", event: Event::ServerStopping, decides: Decides::Nothing },
    Handler { name: "on_tick", event: Event::Tick, decides: Decides::Nothing },
    Handler { name: "on_join", event: Event::PlayerJoined, decides: Decides::Nothing },
    Handler { name: "on_leave", event: Event::PlayerLeft, decides: Decides::Nothing },
    Handler { name: "on_chat", event: Event::PlayerChat, decides: Decides::Cancels },
    Handler { name: "on_death", event: Event::PlayerDied, decides: Decides::Nothing },
    Handler { name: "on_hurt", event: Event::PlayerHurt, decides: Decides::Cancels },
    Handler { name: "on_healed", event: Event::PlayerHealed, decides: Decides::Nothing },
    Handler { name: "on_ate", event: Event::PlayerAte, decides: Decides::Cancels },
    Handler { name: "on_drank", event: Event::PlayerDrank, decides: Decides::Nothing },
    Handler { name: "on_water_in", event: Event::PlayerEnteredWater, decides: Decides::Nothing },
    Handler { name: "on_water_out", event: Event::PlayerLeftWater, decides: Decides::Nothing },
    Handler { name: "on_slot", event: Event::HeldSlotChanged, decides: Decides::Nothing },
    Handler { name: "on_place", event: Event::BlockPlace, decides: Decides::Cancels },
    Handler { name: "on_break", event: Event::BlockBreak, decides: Decides::Cancels },
    Handler { name: "on_changed", event: Event::BlockChanged, decides: Decides::Nothing },
    Handler { name: "on_tree", event: Event::TreeFelled, decides: Decides::Cancels },
    Handler { name: "on_chunk", event: Event::ChunkGenerated, decides: Decides::Nothing },
    Handler { name: "on_spawn", event: Event::EntitySpawned, decides: Decides::Nothing },
    Handler { name: "on_gone", event: Event::EntityRemoved, decides: Decides::Nothing },
    Handler { name: "on_craft", event: Event::ItemCrafted, decides: Decides::Cancels },
    Handler { name: "on_pickup", event: Event::ItemPickedUp, decides: Decides::Nothing },
    Handler { name: "on_dropped", event: Event::ItemDropped, decides: Decides::Cancels },
    Handler { name: "on_tool_broke", event: Event::ToolBroke, decides: Decides::Nothing },
    Handler { name: "on_cured", event: Event::HideCured, decides: Decides::Nothing },
    Handler { name: "on_equip", event: Event::EquipmentChanged, decides: Decides::Nothing },
    Handler { name: "on_open", event: Event::ContainerOpened, decides: Decides::Cancels },
    Handler { name: "on_close", event: Event::ContainerClosed, decides: Decides::Nothing },
    Handler { name: "on_smelted", event: Event::SmeltingFinished, decides: Decides::Nothing },
    Handler { name: "on_growth", event: Event::GrowthStep, decides: Decides::Nothing },
    Handler { name: "on_fire_out", event: Event::FireDied, decides: Decides::Nothing },
    Handler { name: "on_weather", event: Event::WeatherChanged, decides: Decides::Nothing },
    Handler { name: "on_time", event: Event::TimeChanged, decides: Decides::Nothing },
    Handler { name: "on_command", event: Event::Command, decides: Decides::Claims },
];

fn handler_for(event: Event) -> Option<&'static Handler> {
    HANDLERS.iter().find(|h| h.event as i32 == event as i32)
}

/// Что получает обработчик.
///
/// Поля [`EventData`] значат разное у разных событий, и разбирать это
/// в скрипте было бы переизобретением таблицы, которая и так есть в
/// `primitive_modapi`. Поэтому разбор здесь, по одному ряду на
/// событие, а скрипт получает доводы под своими именами.
fn arguments_for(event: Event, data: &EventData) -> Vec<Val> {
    let player = Val::Int(data.player as i64);
    let entity = Val::Int(data.entity as i64);
    let block = Val::Int(i64::from(data.block));
    let count = Val::Int(i64::from(data.count));
    let slot = Val::Int(i64::from(data.slot));
    let float = Val::Num(f64::from(data.float));
    let text = Val::Text(unsafe { data.text.as_str() }.to_string());
    let x = Val::Int(i64::from(data.pos.x));
    let y = Val::Int(i64::from(data.pos.y));
    let z = Val::Int(i64::from(data.pos.z));

    match event {
        Event::ServerStarted | Event::ServerStopping => Vec::new(),
        Event::Tick => vec![Val::Int(data.tick as i64)],
        Event::PlayerJoined | Event::PlayerLeft | Event::PlayerDied => vec![player, text],
        Event::PlayerChat => vec![player, text],
        Event::PlayerHurt => vec![player, float, text],
        Event::PlayerHealed => vec![player, float],
        Event::PlayerAte => vec![player, block, slot],
        Event::PlayerDrank => vec![player],
        Event::PlayerEnteredWater | Event::PlayerLeftWater => vec![player, x, y, z],
        Event::HeldSlotChanged => vec![player, slot, block],
        Event::BlockPlace | Event::BlockBreak => vec![player, x, y, z, block],
        Event::BlockChanged | Event::GrowthStep | Event::FireDied => vec![x, y, z, block],
        Event::TreeFelled => vec![player, x, y, z, count],
        // У этого события `y` всегда ноль, и довода под него нет: ряд
        // из трёх чисел, среднее из которых всегда ноль, — это
        // приглашение перепутать его с ячейкой.
        Event::ChunkGenerated => vec![x, z],
        Event::EntitySpawned => vec![entity, x, y, z],
        Event::EntityRemoved => vec![entity],
        Event::ItemCrafted => vec![player, block, count, Val::Int(data.value)],
        Event::ItemPickedUp => vec![player, block, count, x, y, z],
        Event::ItemDropped => vec![player, block, count],
        Event::ToolBroke | Event::EquipmentChanged => vec![player, block, slot],
        Event::HideCured => vec![player, x, y, z],
        Event::ContainerOpened => vec![player, x, y, z, block],
        Event::ContainerClosed => vec![player, x, y, z],
        Event::SmeltingFinished => vec![x, y, z],
        Event::WeatherChanged => vec![Val::Text(
            weather_name(match data.value {
                1 => Weather::Rain,
                2 => Weather::Storm,
                _ => Weather::Clear,
            })
            .to_string(),
        )],
        Event::TimeChanged => vec![float],
        Event::Command => vec![
            player,
            text,
            Val::List(
                unsafe { data.args.as_str() }
                    .split_whitespace()
                    .map(|word| Val::Text(word.to_string()))
                    .collect(),
            ),
        ],
    }
}

// --------------------------------------------------- поток со скриптами

/// Что можно попросить у потока со скриптами.
enum Job {
    /// Закрыть всё и загрузить заново.
    Reload { reply: std::sync::mpsc::Sender<Report> },
    /// Позвать обработчик у всех, кто его объявил.
    Fire {
        handler: &'static str,
        decides: Decides,
        args: Vec<Val>,
        reply: std::sync::mpsc::Sender<bool>,
    },
    /// Что загружено и что оно умеет.
    List {
        reply: std::sync::mpsc::Sender<Vec<String>>,
    },
    /// Позвать `on_unload`, закрыть всё и уйти.
    Quit { reply: std::sync::mpsc::Sender<()> },
}

/// Итог загрузки: что сказать в журнал и на что подписываться.
struct Report {
    lines: Vec<String>,
    events: Vec<Event>,
}

static JOBS: Mutex<Option<std::sync::mpsc::Sender<Job>>> = Mutex::new(None);
/// Поток, которому принадлежат все живые скрипты. Нужен не для того,
/// чтобы им управлять, а чтобы **узнать себя**: событие, поднятое самим
/// скриптом, приходит на этот же поток, и класть его в очередь, которую
/// этот поток и разбирает, — заклинивание навсегда.
static WORKER: Mutex<Option<std::thread::ThreadId>> = Mutex::new(None);
/// Доставлять ли события. Выключается насовсем, если поток скриптов
/// перестал отвечать: стучаться в него дальше — значит класть по
/// секунде такта на каждое событие в мире.
static DISPATCH: AtomicBool = AtomicBool::new(true);

fn start_worker() {
    let mut slot = JOBS.lock().unwrap_or_else(|e| e.into_inner());
    if slot.is_some() {
        return;
    }
    let (sender, jobs) = std::sync::mpsc::channel::<Job>();
    // Стек больше обычного: AST-интерпретатор идёт по дереву рекурсией,
    // и на каждый вызов в скрипте приходится несколько кадров Rust.
    let spawned = std::thread::Builder::new()
        .name("dypt-mods".to_string())
        .stack_size(64 * 1024 * 1024)
        .spawn(move || worker(jobs));
    match spawned {
        Ok(handle) => {
            // Идентификатор берётся у дескриптора, а не изнутри потока:
            // иначе первое событие может прийти раньше, чем поток
            // успеет записать себя, и «узнать себя» не сработает
            // ровно там, где это опаснее всего.
            *WORKER.lock().unwrap_or_else(|e| e.into_inner()) = Some(handle.thread().id());
            *slot = Some(sender);
        }
        Err(e) => log(
            LogLevel::Error,
            &format!("не создать поток для скриптовых модов: {}", e),
        ),
    }
}

fn ask<T>(make: impl FnOnce(std::sync::mpsc::Sender<T>) -> Job, wait: Duration) -> Option<T> {
    let sender = {
        let slot = JOBS.lock().unwrap_or_else(|e| e.into_inner());
        slot.clone()?
    };
    let (reply, answer) = std::sync::mpsc::channel::<T>();
    sender.send(make(reply)).ok()?;
    answer.recv_timeout(wait).ok()
}

/// Перечитать все скриптовые моды.
///
/// Ради этой строки всё и затевалось: правка в файле становится живой
/// без сборки и без перезапуска сервера.
///
/// `wait` — сколько ждать. При загрузке сервера ждать можно долго:
/// тактов ещё нет, и разбор десятка скриптов честно занимает время.
/// По команде `/dypt reload` ждёт **тактовый цикл**, и там срок
/// короткий, а по его истечении поток бросают и заводят новый: иначе
/// команда, которой чинят зависший скрипт, сама вешает сервер на
/// минуту.
fn reload_script_mods(wait: Duration, may_restart: bool) -> Report {
    // Доставка включается обратно **после** удачной загрузки, а не
    // перед ней. Включённая заранее, она за время ожидания успевает
    // выключиться снова: события, которые пришли, пока мы ждём,
    // стучатся в тот самый поток, из-за которого всё и затевалось.
    if let Some(report) = ask(|reply| Job::Reload { reply }, wait) {
        DISPATCH.store(true, Ordering::SeqCst);
        return report;
    }
    if !may_restart {
        return Report {
            lines: vec!["поток скриптовых модов не отвечает".to_string()],
            events: Vec::new(),
        };
    }

    // Поток занят навсегда, и убить его нечем: в Rust нет способа
    // прервать чужой поток, а в языке нет места, где он спросил бы
    // разрешения продолжать. Значит, его бросают — со всеми скриптами,
    // которые в нём жили, — и заводят новый. Брошенный поток остаётся
    // жечь одно ядро до конца жизни сервера, и об этом говорится
    // вслух: это плата за скрипт с бесконечным циклом, и оператор
    // должен знать, что она взята.
    log(
        LogLevel::Error,
        "поток скриптовых модов не ответил и брошен — он занимает одно ядро до перезапуска сервера. Виноват скрипт с бесконечным циклом",
    );
    restart_worker();
    match ask(|reply| Job::Reload { reply }, wait) {
        Some(report) => {
            DISPATCH.store(true, Ordering::SeqCst);
            report
        }
        None => Report {
            lines: vec!["и новый поток не отвечает — дело не в скриптах".to_string()],
            events: Vec::new(),
        },
    }
}

/// Забыть прежний поток со скриптами и завести новый.
fn restart_worker() {
    // Отправитель роняется первым: прежний поток, если он всё-таки
    // когда-нибудь досчитает, выйдет из своего цикла на закрытом
    // канале, а не останется висеть в `recv` навсегда.
    *JOBS.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *WORKER.lock().unwrap_or_else(|e| e.into_inner()) = None;
    start_worker();
}

fn worker(jobs: std::sync::mpsc::Receiver<Job>) {
    let mut loaded: Vec<ScriptMod> = Vec::new();
    while let Ok(job) = jobs.recv() {
        match job {
            Job::Reload { reply } => {
                close_all(&mut loaded);
                let report = open_all(&mut loaded);
                let _ = reply.send(report);
            }
            Job::Fire {
                handler,
                decides,
                args,
                reply,
            } => {
                let allow = fire(&mut loaded, handler, decides, &args);
                let _ = reply.send(allow);
            }
            Job::List { reply } => {
                let lines = loaded
                    .iter()
                    .map(|m| format!("  {} — {}", m.name, m.handlers.join(", ")))
                    .collect();
                let _ = reply.send(lines);
            }
            Job::Quit { reply } => {
                close_all(&mut loaded);
                let _ = reply.send(());
                break;
            }
        }
    }
}

/// Один живой скрипт.
struct ScriptMod {
    name: String,
    /// Непрозрачный указатель из `dypt_open`. Принадлежит этому потоку
    /// и никакому другому — см. шапку.
    handle: *mut c_void,
    /// **Адрес обязан быть устойчивым.** Он лежит в таблице хозяина,
    /// которую библиотека языка забрала себе на всю жизнь скрипта, так
    /// что сессия живёт в куче и не переезжает.
    session: Box<Session>,
    handlers: Vec<&'static str>,
}

/// Указатели на функции языка, скопированные из-под замка.
///
/// Копия, а не замок на время вызова: скрипт внутри `dypt_call` зовёт
/// игру, игра может позвать `/dypt`, а `/dypt` берёт этот же замок.
fn language_calls() -> Option<(DyptOpenFn, DyptHasFn, DyptCallFn, DyptCloseFn)> {
    let guard = LANGUAGE.lock().unwrap_or_else(|e| e.into_inner());
    let language = guard.as_ref()?;
    Some((
        language.open,
        language.has,
        language.call,
        language.close,
    ))
}

fn open_all(loaded: &mut Vec<ScriptMod>) -> Report {
    let mut report = Report {
        lines: Vec::new(),
        events: Vec::new(),
    };
    let Some((open, has, call, _close)) = language_calls() else {
        report.lines.push("язык не открыт — скриптовых модов нет".to_string());
        return report;
    };
    let folder = with_config(|c| c.mods.clone()).unwrap_or_default();
    let Ok(entries) = std::fs::read_dir(&folder) else {
        report
            .lines
            .push(format!("папки {} нет — скриптовых модов нет", folder.display()));
        return report;
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("dpt"))
        .collect();
    // По алфавиту: порядок загрузки решает, чей `on_break` скажет
    // «нет» первым, и он обязан быть одним и тем же на всех машинах.
    files.sort();

    let names: Vec<DyStr> = VOCABULARY.iter().map(|n| DyStr::of(n)).collect();
    for path in files {
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "?".to_string());
        let mut session = Box::new(Session {
            caller: 0,
            label: name.clone(),
            arguments: Vec::new(),
            deadline: None,
            honour_cancel: false,
            out: Bag::default(),
        });
        let host = DyHost {
            user: &mut *session as *mut Session as *mut c_void,
            call: host_call,
            print: host_print,
        };
        let text = path.to_string_lossy().into_owned();
        let mut error = vec![0u8; 4096];
        let mut error_len: usize = 0;
        let handle = unsafe {
            open(
                DyStr::of(&text),
                names.as_ptr(),
                names.len(),
                &host,
                error.as_mut_ptr(),
                error.len(),
                &mut error_len,
            )
        };
        if handle.is_null() {
            error.truncate(error_len);
            report.lines.push(format!(
                "мод '{}' не загружен: {}",
                name,
                String::from_utf8_lossy(&error).replace('\n', " ")
            ));
            continue;
        }

        let mut handlers = Vec::new();
        for handler in HANDLERS {
            if unsafe { has(handle, DyStr::of(handler.name)) } != 0 {
                handlers.push(handler.name);
                if !report.events.iter().any(|e| *e as i32 == handler.event as i32) {
                    report.events.push(handler.event);
                }
            }
        }
        for extra in [ON_LOAD, ON_UNLOAD] {
            if unsafe { has(handle, DyStr::of(extra)) } != 0 {
                handlers.push(extra);
            }
        }

        report.lines.push(if handlers.is_empty() {
            format!("мод '{}' загружен, но ничего не ловит", name)
        } else {
            format!("мод '{}': {}", name, handlers.join(", "))
        });

        let mut script = ScriptMod {
            name,
            handle,
            session,
            handlers,
        };
        if script.handlers.contains(&ON_LOAD) {
            if let Err(reason) = invoke(call, &mut script, ON_LOAD, &[]) {
                report
                    .lines
                    .push(format!("  on_load: {}", reason.replace('\n', " ")));
            }
        }
        loaded.push(script);
    }

    if loaded.is_empty() && report.lines.is_empty() {
        report
            .lines
            .push(format!("скриптовых модов в {} нет", folder.display()));
    }
    report
}

fn close_all(loaded: &mut Vec<ScriptMod>) {
    let Some((_open, _has, call, close)) = language_calls() else {
        loaded.clear();
        return;
    };
    for script in loaded.iter_mut() {
        if script.handlers.contains(&ON_UNLOAD) {
            let _ = invoke(call, script, ON_UNLOAD, &[]);
        }
    }
    for script in loaded.drain(..) {
        unsafe { close(script.handle) };
    }
}

/// Позвать один обработчик у всех, кто его объявил.
///
/// Возвращает «можно ли»: `false` останавливает действие. Отказавших
/// может быть несколько, и опрашиваются все — скрипт, который считает
/// сломанные блоки, обязан досчитать, даже если соседний скрипт уже
/// сказал «нет».
fn fire(loaded: &mut [ScriptMod], handler: &'static str, decides: Decides, args: &[Val]) -> bool {
    let Some((_open, _has, call, _close)) = language_calls() else {
        return true;
    };
    let limit = with_config(|c| c.handler_limit).unwrap_or(Duration::from_millis(1000));
    let mut allow = true;
    for script in loaded.iter_mut() {
        if !script.handlers.contains(&handler) {
            continue;
        }
        script.session.deadline = Some(Instant::now() + limit);
        match invoke(call, script, handler, args) {
            Ok(answer) => match decides {
                Decides::Nothing => {}
                // `null` — не отказ. Обработчик, забывший `return`,
                // возвращает именно его, и считать это запретом значит
                // сломать мир человеку, который просто печатал в
                // журнал.
                Decides::Cancels => {
                    if answer.kind == DY_BOOL && answer.b == 0 {
                        allow = false;
                    }
                }
                Decides::Claims => {
                    if answer.kind == DY_BOOL && answer.b != 0 {
                        allow = false;
                    }
                }
            },
            Err(reason) => log(
                LogLevel::Warn,
                &format!(
                    "[{}] {}: {}",
                    script.name,
                    handler,
                    reason.replace('\n', " ")
                ),
            ),
        }
    }
    allow
}

/// Один вызов в скрипт, со всей канителью буферов.
fn invoke(
    call: DyptCallFn,
    script: &mut ScriptMod,
    handler: &str,
    args: &[Val],
) -> Result<DyValue, String> {
    // Доводы живут на этом стеке ровно до конца вызова, и этого
    // достаточно: язык перекладывает их к себе, не выходя из него.
    let mut bag = Bag::default();
    let converted: Vec<DyValue> = args.iter().cloned().map(|v| bag.flatten(v)).collect();
    let mut answer = DyValue::NULL;
    let mut error = vec![0u8; 4096];
    let mut error_len: usize = 0;
    let code = unsafe {
        call(
            script.handle,
            DyStr::of(handler),
            converted.as_ptr(),
            converted.len(),
            &mut answer,
            error.as_mut_ptr(),
            error.len(),
            &mut error_len,
        )
    };
    if code == 0 {
        return Ok(answer);
    }
    error.truncate(error_len);
    Err(String::from_utf8_lossy(&error).into_owned())
}

// ------------------------------------------------------------ доставка

fn on_worker_thread() -> bool {
    let guard = WORKER.lock().unwrap_or_else(|e| e.into_inner());
    guard.is_some_and(|id| id == std::thread::current().id())
}

/// Отдать событие скриптам и дождаться, что они скажут.
fn deliver(event: Event, data: &EventData) -> HookResult {
    let Some(handler) = handler_for(event) else {
        return HookResult::Continue;
    };
    if !DISPATCH.load(Ordering::SeqCst) {
        return HookResult::Continue;
    }
    // Событие, поднятое самим скриптом. Ждать здесь означало бы ждать
    // самого себя; см. шапку файла.
    if on_worker_thread() {
        return HookResult::Continue;
    }

    let limit = with_config(|c| c.handler_limit).unwrap_or(Duration::from_millis(1000));
    let args = arguments_for(event, data);
    let decides = handler.decides;
    let name = handler.name;
    match ask(
        move |reply| Job::Fire {
            handler: name,
            decides,
            args,
            reply,
        },
        limit,
    ) {
        Some(true) => HookResult::Continue,
        Some(false) => HookResult::Cancel,
        None => {
            // Ответа нет и не будет: поток занят навсегда. Выключаем
            // доставку целиком — и говорим об этом **один раз**: пока
            // мы ждали, в тот же тупик успели упереться и другие
            // потоки, и три одинаковые строки в журнале выглядят как
            // три разные беды.
            if DISPATCH.swap(false, Ordering::SeqCst) {
                log(
                LogLevel::Error,
                &format!(
                    "обработчик {} не ответил за {} мс — доставка событий в скрипты выключена, спасайте сервер через /dypt reload",
                    name,
                    limit.as_millis()
                ),
                );
            }
            HookResult::Continue
        }
    }
}

// =====================================================================
// Точка входа
// =====================================================================

primitive_modapi::declare_mod! {
    name: "dypt",
    version: "1.0.0",
    load: load,
    event: on_event,
}

/// Версия договора этого мода, для тестов и для журнала.
pub const MOD_API_VERSION: ApiVersion = API_VERSION;

#[cfg(test)]
mod tests {
    use super::*;

    /// Имена встроенных функций самого dypt на день, когда писался
    /// словарь. Список короткий и меняется редко; когда он изменится,
    /// сломается тест ниже, а не чей-то скрипт.
    const LANGUAGE_BUILTINS: &[&str] = &[
        "add", "and", "append", "array", "byte_length", "call_bin", "chr", "clock", "contains",
        "exists", "exit", "free", "free_bin", "get", "get_reg", "get_tuple", "has", "has_item",
        "intersect", "join", "keys", "len", "length", "load_bin", "lower", "malloc", "map",
        "mem_read", "mem_write", "not", "or", "ord", "peek", "peek32", "poke", "pop", "print",
        "push", "read", "readln", "remove", "replace", "set", "set_reg", "shl", "shr", "sleep",
        "slice", "split", "sreadln", "tonumber", "tuple", "type", "union", "upper", "values",
        "write",
    ];

    #[test]
    fn no_game_call_hides_a_function_the_language_already_has() {
        // Объявить `set` или `map` игровой функцией — значит закрыть
        // скрипту доступ к той, что была: имя разрешается в одной
        // таблице, и побеждает то, что объявлено последним. Человек
        // после этого ищет причину в своём коде, а её там нет.
        for name in VOCABULARY {
            assert!(
                !LANGUAGE_BUILTINS.contains(name),
                "'{name}' — уже встроенная функция dypt; возьмите другое имя"
            );
        }
    }

    #[test]
    fn the_vocabulary_names_nothing_twice() {
        let mut sorted: Vec<&str> = VOCABULARY.to_vec();
        sorted.sort_unstable();
        let before = sorted.len();
        sorted.dedup();
        assert_eq!(before, sorted.len(), "в словаре есть повтор");
    }

    #[test]
    fn a_map_of_two_pairs_crosses_as_four_values() {
        // Договор о словаре — единственное место, где число элементов и
        // длина массива не совпадают: `count` считает пары, а `items`
        // хранит ключ и значение подряд. Ошибиться здесь вдвое — значит
        // прочитать половину чужой памяти.
        let mut bag = Bag::default();
        let value = bag.flatten(Val::Map(vec![
            ("a", Val::Int(1)),
            ("b", Val::Text("два".to_string())),
        ]));
        assert_eq!(value.kind, DY_MAP);
        assert_eq!(value.count, 2);
        let items = unsafe { std::slice::from_raw_parts(value.items, value.count * 2) };
        assert_eq!(items[0].kind, DY_STRING);
        assert_eq!(unsafe { items[0].s.as_str() }, "a");
        assert_eq!(items[1].i, 1);
        assert_eq!(unsafe { items[3].s.as_str() }, "два");
    }

    #[test]
    fn a_script_name_cannot_climb_out_of_the_scripts_folder() {
        // `/dypt ../../settings.toml` иначе читал бы что угодно на
        // диске правами сервера — а команду набирает человек в чате.
        let climbed = resolve("../../settings.toml");
        assert!(
            !climbed.to_string_lossy().contains(".."),
            "путь вылез наружу: {}",
            climbed.display()
        );
        // ...и расширение дописывается, потому что руками пишут имя.
        assert_eq!(
            resolve("дом").extension().and_then(|s| s.to_str()),
            Some("dpt")
        );
    }

    #[test]
    fn every_event_has_at_most_one_handler_and_every_handler_one_event() {
        // `handler_for` ищет по событию и берёт первое совпадение, а
        // `fire` зовёт по имени. Два ряда с одним событием означали бы,
        // что второй обработчик объявлен, виден в `/dypt mods` и не
        // вызывается никогда — худшая форма поломки, потому что
        // выглядит она как работающая.
        let mut events: Vec<i32> = HANDLERS.iter().map(|h| h.event as i32).collect();
        let before = events.len();
        events.sort_unstable();
        events.dedup();
        assert_eq!(before, events.len(), "два обработчика на одно событие");

        let mut names: Vec<&str> = HANDLERS.iter().map(|h| h.name).collect();
        let before = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(before, names.len(), "два события на одно имя");
    }

    #[test]
    fn only_the_events_the_game_calls_cancellable_can_be_refused() {
        // Отмена события, которое игра отменять не умеет, молча ничего
        // не делает: скрипт вернул `false`, игрок всё равно умер, и
        // объяснить это человеку нечем. Список — из документации
        // `primitive_modapi::Event`, и он здесь переписан руками
        // именно затем, чтобы расхождение с ней стало красным тестом.
        const CANCELLABLE: &[i32] = &[
            Event::PlayerChat as i32,
            Event::PlayerHurt as i32,
            Event::PlayerAte as i32,
            Event::BlockPlace as i32,
            Event::BlockBreak as i32,
            Event::TreeFelled as i32,
            Event::ItemCrafted as i32,
            Event::ItemDropped as i32,
            Event::ContainerOpened as i32,
        ];
        for handler in HANDLERS {
            if handler.decides == Decides::Cancels {
                assert!(
                    CANCELLABLE.contains(&(handler.event as i32)),
                    "{} обещает отмену события, которое игра не отменяет",
                    handler.name
                );
            }
        }
        // ...и наоборот: отменяемое событие без отмены — это возможность,
        // о которой никто не узнает.
        for event in CANCELLABLE {
            let found = HANDLERS
                .iter()
                .find(|h| h.event as i32 == *event)
                .unwrap_or_else(|| panic!("отменяемое событие {event} без обработчика"));
            assert_eq!(
                found.decides,
                Decides::Cancels,
                "{} могло бы отменять и не отменяет",
                found.name
            );
        }
    }

    #[test]
    fn a_handler_name_cannot_be_a_game_call() {
        // Скрипт объявляет обработчики как обычные функции. Имя,
        // совпадающее с игровым вызовом, закрыло бы этот вызов всему
        // скрипту — и тому, кто его объявил, и тем, кто просто читает.
        for handler in HANDLERS {
            assert!(
                !VOCABULARY.contains(&handler.name),
                "'{}' — и обработчик, и игровая функция",
                handler.name
            );
        }
    }

    #[test]
    fn the_two_sides_of_the_bridge_agree_about_the_size_of_a_value() {
        // Обе стороны компилируются порознь и обязаны разложить
        // `DyValue` одинаково. Размер — не доказательство совпадения, но
        // единственная проверка, которую можно выполнить, не имея под
        // рукой второй библиотеки; расхождение здесь означает, что
        // кто-то поменял договор, не тронув `DY_ABI`.
        use std::mem::size_of;
        assert_eq!(size_of::<DyStr>(), 2 * size_of::<usize>());
        assert_eq!(DY_ABI, 2);
        assert!(size_of::<DyValue>() >= size_of::<DyStr>() + 24);
    }
}
