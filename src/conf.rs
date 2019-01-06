use atty::Stream;
use getopts::{Matches, Options};
use std::env;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::PathBuf;
use std::process::exit;
use std::str::FromStr;
use termcolor::{Color, ColorSpec};

use crate::fmt;
use dirs;
use term_size;
use todo_lib::{terr, tfilter, todo, tsort};
use toml;

const TODOFILE_VAR: &str = "TTDL_FILENAME";
const CONF_FILE: &str = "ttdl.toml";
const TODO_FILE: &str = "todo.txt";
const DONE_FILE: &str = "done.txt";

#[derive(Debug, PartialEq, Clone)]
pub enum RunMode {
    None,
    List,
    Add,
    Done,
    Undone,
    Remove,
    Clean,
    Edit,
    // Start,
    // Stop,
}

#[derive(Debug, Clone)]
pub struct Conf {
    pub mode: RunMode,
    pub verbose: bool,
    pub dry: bool,
    pub wipe: bool,
    pub use_done: bool,
    pub todo_file: PathBuf,
    pub done_file: PathBuf,

    pub todo: todo::Conf,
    pub fmt: fmt::Conf,
    pub flt: tfilter::Conf,
    pub sort: tsort::Conf,
}

impl Default for Conf {
    fn default() -> Conf {
        Conf {
            mode: RunMode::None,
            dry: false,
            verbose: false,
            wipe: false,
            use_done: false,
            todo_file: PathBuf::from(TODO_FILE),
            done_file: PathBuf::from(DONE_FILE),

            fmt: Default::default(),
            todo: Default::default(),
            flt: Default::default(),
            sort: Default::default(),
        }
    }
}

impl Conf {
    fn new() -> Self {
        Default::default()
    }
}

fn print_usage(program: &str, opts: &Options) {
    let brief = format!(
        "Usage: {} command [ID or ID range] [subject] [filter] [new values] [extra options]",
        program
    );
    print!("{}", opts.usage(&brief));

    println!("\n\n[ID or ID range] - ID is the order number of a todo starting from 1. The range is inclusive. It is OK to use non-existing IDs - all invalid IDs are skipped while processing the command
");

    let filter = r#"Filter options include:
    --all | -a, --complete | -A, --rec, --due, --pri, --regex | -e, --threshold
    +project - select todos which are related to project "project"; if more than one project name is defined in command line, they are combined with OR;
    @context - select todos which have context "project"; if more than one context is set, they are combined with OR;
    "#;

    let newones = r#"Modifying options are:
    --set-pri, --set-due, --set-rec, --set-proj, --set-ctx, --del-proj, --del-ctx, --repl-proj, --repl-ctx, --set-threshold
    "#;

    let extras = r#"Extra options:
    --dry-run, --sort | -s, --sort-rev, --wrap, --short, --width, --local, --no-colors
    "#;
    let commands = r#"Available commands:
    list | l - list todos
        `ttdl l -s=proj,pri` - show all incomplete todos sorted by their project and by priority inside each project
        `ttdl l "car*"` - list all todos which have substring `car*` in their subject, project or context
        `ttdl l "car*" -e` - list all todos which have subject, project or context matched regular expression `car*`
        `ttdl l "car"` - list all todos which have substring `car` in their subject, project or context
        `ttdl l --pri=a` - show all incomplete todos with the highest priority A
        `ttdl l --pri=b+` - show all incomplete todos with priority B and higher (only A and B in this case)
        `ttdl l +car +train` - show all incomplete todos which related either to `car` or to `train` projects
        `ttdl l +my* @*tax` - show all incomplete todos that have a project tag starts with `my` and a context ends with `tax`
        `ttdl l --due=tomorrow -a` - show all todos that are due tomorrow
        `ttdl l --due=soon` - show all incomplete todos which are due are due in less a few days, including overdue ones (the range is configurable and default value is 7 days)
        `ttdl l --due=overdue` - show all incomplete overdue todos
        `ttdl l --due=today -a` - show all todos that are due today
        `ttdl l -a +myproj @ui @rest` - show both incomplete and done todos related to project 'myproj' which contains either 'ui' or 'rest' context
    add | a - add a new todo
        `ttdl a "send tax declaration +personal @finance @tax due:2018-04-01 rec:1y"` - add a new recurrent todo(yearly todo) with a due date first of April every year
    done | d - mark regular incomplete todos completed, pushes due date for recurrent todos to their next date
        `ttdl d 2-5` - mark todos with IDs from 2 through 5 done
    undone - remove finish date and completion mark from completed todos
    clean | c | archive | arc - move all completed todos to `done.txt`. If option `--wipe` is set then completed todos are removed instead of moving
    remove | rm - delete selected todos. Warning: by default completed todos are not selected, so be careful
        `ttdl rm 2-5` - delete incomplete todos with IDs from 2 thorough 5
        `ttdl rm 2-5 -a` - delete both done and incomplete todos with IDs from 2 through 5
        `ttdl rm 2-5 -A` - delete all done todos with IDs from 2 through 5. The command does the same as `ttdl clean 2-5 --wipe` does
    edit | e - modifies selected todos. Warninig: if you try to change a subject of a few todos, only the first todo would be changed. It is by design
        `ttdl e 2-5 "new subject"` - only the first incomplete todo with ID between 2 and 5 changes its subject
        `ttdl e +proj --repl-ctx=bug1010@bug1020` - replace context `bug1010` with `bug1020` for all incomplete todos that related to project `proj`
        `ttdl e @customer_acme --set-due=2018-12-31` - set due date 2018-12-31 for all incomplete todos that has `customer_acme` context
        `ttdl e @customer_acme --set-due=none` - remove due date 2018-12-31 for all incomplete todos that has `customer_acme` context
        `ttdl e --pri=none --set-pri=z` - set the lowest priority for all incomplete todos which do not have a priority set`
        `ttdl e @bug1000 --set-pri=+` - increase priority for all incomplete todos which have context `bug1000`, todos which did not have priority set get the lowest priority `z`
    "#;
    println!("{}\n\n{}\n\n{}\n\n{}", commands, filter, newones, extras);
}

fn str_to_mode(s: &str) -> RunMode {
    match s {
        "l" | "list" => RunMode::List,
        "a" | "add" => RunMode::Add,
        "d" | "done" => RunMode::Done,
        "u" | "undone" => RunMode::Undone,
        "c" | "clean" | "arc" | "archive" => RunMode::Clean,
        "e" | "edit" => RunMode::Edit,
        "rm" | "remove" => RunMode::Remove,
        // "start" => RunMode::Start,
        // "stop" => RunMode::Stop,
        _ => RunMode::None,
    }
}

fn split_filter(orig: &str) -> (String, tfilter::ValueSpan) {
    if orig.starts_with('+') || orig.ends_with('+') {
        return (orig.trim_matches('+').to_string(), tfilter::ValueSpan::Higher);
    } else if orig.starts_with('-') || orig.ends_with('-') {
        return (orig.trim_matches('-').to_string(), tfilter::ValueSpan::Lower);
    }

    (orig.to_string(), tfilter::ValueSpan::Equal)
}

fn parse_filter(matches: &Matches, c: &mut tfilter::Conf) -> Result<(), terr::TodoError> {
    if matches.opt_present("a") {
        c.all = tfilter::TodoStatus::All;
    }
    if matches.opt_present("A") {
        c.all = tfilter::TodoStatus::Done;
    }
    if matches.opt_present("e") {
        c.use_regex = true;
    }
    // if matches.opt_present("t") {
    //     c.active = true;
    // }
    if matches.opt_present("pri") {
        let s = match matches.opt_str("pri") {
            None => String::new(),
            Some(s_orig) => s_orig.to_lowercase(),
        };
        match s.as_str() {
            "-" | "none" => {
                c.pri = Some(tfilter::Priority {
                    value: todo::NO_PRIORITY,
                    span: tfilter::ValueSpan::None,
                });
            }
            "any" | "+" => {
                c.pri = Some(tfilter::Priority {
                    value: todo::NO_PRIORITY,
                    span: tfilter::ValueSpan::Any,
                });
            }
            _ => {
                let (s, modif) = split_filter(&s);
                if s.len() != 1 {
                    return Err(terr::TodoError::from(terr::TodoErrorKind::InvalidValue {
                        value: s,
                        name: "priority".to_string(),
                    }));
                }
                let p = s.as_bytes()[0];
                if p < b'a' || p > b'z' {
                    return Err(terr::TodoError::from(terr::TodoErrorKind::InvalidValue {
                        value: s,
                        name: "priority".to_string(),
                    }));
                }
                c.pri = Some(tfilter::Priority {
                    value: p - b'a',
                    span: modif,
                });
            }
        }
    }
    if matches.opt_present("rec") {
        let rstr = match matches.opt_str("rec") {
            None => String::new(),
            Some(s) => s.to_lowercase(),
        };
        match rstr.as_str() {
            "" => {}
            "-" | "none" => {
                c.rec = Some(tfilter::Recurrence {
                    span: tfilter::ValueSpan::None,
                })
            }
            "+" | "any" => {
                c.rec = Some(tfilter::Recurrence {
                    span: tfilter::ValueSpan::Any,
                })
            }
            // TODO: add equal?
            _ => {
                return Err(terr::TodoError::from(terr::TodoErrorKind::InvalidValue {
                    value: rstr,
                    name: "recurrence".to_string(),
                }))
            }
        }
    }
    if matches.opt_present("due") {
        let dstr = match matches.opt_str("due") {
            None => String::new(),
            Some(s) => s.to_lowercase(),
        };
        match dstr.as_str() {
            "-" | "none" => {
                c.due = Some(tfilter::Due {
                    days: Default::default(),
                    span: tfilter::ValueSpan::None,
                });
            }
            "any" | "+" => {
                c.due = Some(tfilter::Due {
                    days: Default::default(),
                    span: tfilter::ValueSpan::Any,
                });
            }
            "over" | "overdue" => {
                c.due = Some(tfilter::Due {
                    days: tfilter::ValueRange { low: -1, high: 0 },
                    span: tfilter::ValueSpan::Lower,
                });
            }
            "soon" => {
                c.due = Some(tfilter::Due {
                    days: tfilter::ValueRange { low: 0, high: 7 },
                    span: tfilter::ValueSpan::Range,
                });
            }
            "today" => {
                c.due = Some(tfilter::Due {
                    days: tfilter::ValueRange { low: 0, high: 0 },
                    span: tfilter::ValueSpan::Range,
                });
            }
            "tomorrow" => {
                c.due = Some(tfilter::Due {
                    days: tfilter::ValueRange { low: 0, high: 1 },
                    span: tfilter::ValueSpan::Range,
                });
            }
            _ => {
                return Err(terr::TodoError::from(terr::TodoErrorKind::InvalidValue {
                    value: dstr,
                    name: "date range".to_string(),
                }))
            }
        }
    }
    if matches.opt_present("threshold") {
        let dstr = match matches.opt_str("threshold") {
            None => String::new(),
            Some(s) => s.to_lowercase(),
        };
        match dstr.as_str() {
            "-" | "none" => {
                c.thr = Some(tfilter::Due {
                    days: Default::default(),
                    span: tfilter::ValueSpan::None,
                });
            }
            "any" | "+" => {
                c.thr = Some(tfilter::Due {
                    days: Default::default(),
                    span: tfilter::ValueSpan::Any,
                });
            }
            _ => {
                return Err(terr::TodoError::from(terr::TodoErrorKind::InvalidValue {
                    value: dstr,
                    name: "date range".to_string(),
                }))
            }
        }
    }

    Ok(())
}

fn parse_todo(matches: &Matches, c: &mut todo::Conf) -> Result<(), terr::TodoError> {
    if let Some(s) = matches.opt_str("set-pri") {
        let s = s.to_lowercase();
        match s.as_str() {
            "-" => {
                c.priority_act = todo::Action::Decrease;
            }
            "+" => {
                c.priority_act = todo::Action::Increase;
            }
            "none" => {
                c.priority_act = todo::Action::Delete;
            }
            _ => {
                let p = s.as_bytes()[0];
                if p < b'a' || p > b'z' {
                    return Err(terr::TodoError::from(terr::TodoErrorKind::InvalidValue {
                        value: s,
                        name: "priority".to_string(),
                    }));
                }
                c.priority = p - b'a';
                c.priority_act = todo::Action::Set;
            }
        }
    }

    if let Some(s) = matches.opt_str("set-rec") {
        let s = s.to_lowercase();
        match s.as_str() {
            "-" | "none" => {
                c.recurrence_act = todo::Action::Delete;
            }
            _ => match todo_txt::task::Recurrence::from_str(&s) {
                Ok(r) => {
                    c.recurrence = Some(r);
                    c.recurrence_act = todo::Action::Set;
                }
                Err(_) => {
                    return Err(terr::TodoError::from(terr::TodoErrorKind::InvalidValue {
                        value: s,
                        name: "recurrence".to_string(),
                    }))
                }
            },
        }
    }

    if let Some(s) = matches.opt_str("set-due") {
        match s.as_str() {
            "-" | "none" => {
                c.due_act = todo::Action::Delete;
            }
            _ => match chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
                Ok(d) => {
                    c.due = Some(d);
                    c.due_act = todo::Action::Set;
                }
                Err(_) => {
                    return Err(terr::TodoError::from(terr::TodoErrorKind::InvalidValue {
                        value: s,
                        name: "date".to_string(),
                    }))
                }
            },
        }
    }

    if let Some(s) = matches.opt_str("set-threshold") {
        match s.as_str() {
            "-" | "none" => {
                c.recurrence_act = todo::Action::Delete;
            }
            _ => match chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
                Ok(d) => {
                    c.thr = Some(d);
                    c.thr_act = todo::Action::Set;
                }
                Err(_) => {
                    return Err(terr::TodoError::from(terr::TodoErrorKind::InvalidValue {
                        value: s,
                        name: "date".to_string(),
                    }))
                }
            },
        }
    }

    if let Some(s) = matches.opt_str("set-proj") {
        for st in s.split(',') {
            c.projects.push(st.to_string());
        }
        c.project_act = todo::Action::Set;
    }

    if let Some(s) = matches.opt_str("del-proj") {
        for st in s.split(',') {
            c.projects.push(st.to_string());
        }
        c.project_act = todo::Action::Delete;
    }

    if let Some(s) = matches.opt_str("repl-proj") {
        for st in s.split(',') {
            c.projects.push(st.to_string());
        }
        c.project_act = todo::Action::Replace;
    }

    if let Some(s) = matches.opt_str("set-ctx") {
        for st in s.split(',') {
            c.contexts.push(st.to_string());
        }
        c.context_act = todo::Action::Set;
    }

    if let Some(s) = matches.opt_str("del-ctx") {
        for st in s.split(',') {
            c.contexts.push(st.to_string());
        }
        c.context_act = todo::Action::Delete;
    }

    if let Some(s) = matches.opt_str("repl-ctx") {
        for st in s.split(',') {
            c.contexts.push(st.to_string());
        }
        c.context_act = todo::Action::Replace;
    }

    Ok(())
}

fn parse_sort(matches: &Matches, c: &mut tsort::Conf) -> Result<(), terr::TodoError> {
    if matches.opt_present("s") {
        // TODO: check for valid
        match matches.opt_str("s") {
            None => c.fields = Some("priority".to_owned()),
            Some(s) => c.fields = Some(s),
        }
    }
    if matches.opt_present("sort-rev") {
        c.rev = true;
    }

    Ok(())
}

fn parse_fmt(matches: &Matches, c: &mut fmt::Conf) -> Result<(), terr::TodoError> {
    if matches.opt_present("short") {
        c.fmt = fmt::Format::Short;
    }
    if matches.opt_present("wrap") {
        c.long = fmt::LongLine::WordWrap;
    }
    c.human = matches.opt_present("human");
    c.atty = atty::is(Stream::Stdout);

    if matches.opt_present("no-colors") || !c.atty {
        c.color_term = fmt::TermColorType::None;
    }

    if matches.opt_present("compact") {
        c.compact = true;
        c.long = fmt::LongLine::Cut;
    }
    if let Some(s) = matches.opt_str("width") {
        if let Ok(n) = s.parse::<u16>() {
            c.width = n;
        }
    }

    if !c.atty && c.width == 0 {
        c.long = fmt::LongLine::Simple;
    } else if c.atty {
        // TODO: sane limit?
        if c.width > 2000 || c.width < 20 {
            if let Some((w, _)) = term_size::dimensions() {
                c.width = (w % 65536) as u16;
            }
        }
    }

    if let Some(s) = matches.opt_str("fields") {
        if s.find(',').is_some() {
            c.fields = s.split(',').map(|s| s.to_string()).collect();
        } else {
            c.fields = s.split(':').map(|s| s.to_string()).collect();
        }
    }

    Ok(())
}

fn detect_filenames(matches: &Matches, conf: &mut Conf) {
    if let Ok(val) = env::var(TODOFILE_VAR) {
        if !val.is_empty() {
            conf.todo_file = PathBuf::from(val);
        }
    }
    if matches.opt_present("local") {
        conf.todo_file = PathBuf::from(TODO_FILE);
    }

    if conf.todo_file.is_dir() {
        conf.todo_file.push(TODO_FILE);
    }

    conf.done_file = conf.todo_file.with_file_name(DONE_FILE);
}

fn read_color(info: &toml::Value, name: &str) -> ColorSpec {
    let c = match info.get(name) {
        Some(cv) => cv,
        None => return fmt::default_color(),
    };

    let s = match c.as_str() {
        Some(ss) => ss,
        None => return fmt::default_color(),
    };

    color_from_str(s)
}

fn update_colors_from_conf(cls: &toml::Value, conf: &mut Conf) {
    conf.fmt.colors.overdue = read_color(cls, "overdue");
    conf.fmt.colors.top = read_color(cls, "top");
    conf.fmt.colors.important = read_color(cls, "important");
    conf.fmt.colors.today = read_color(cls, "today");
    conf.fmt.colors.soon = read_color(cls, "soon");
    conf.fmt.colors.done = read_color(cls, "done");
    conf.fmt.colors.threshold = read_color(cls, "threshold");

    if conf.fmt.color_term == fmt::TermColorType::Auto {
        if let Some(cv) = cls.get("color_term") {
            if let Some(cs) = cv.as_str() {
                conf.fmt.color_term = match cs.to_lowercase().as_str() {
                    "ansi" => fmt::TermColorType::Ansi,
                    "none" => fmt::TermColorType::None,
                    _ => fmt::TermColorType::Auto,
                }
            }
        }
    }
}

fn update_ranges_from_conf(rng: &toml::Value, conf: &mut Conf) {
    if let Some(imp_v) = rng.get("important") {
        if let Some(imp) = imp_v.as_str() {
            if imp.len() == 1 {
                let lowst = imp.to_lowercase();
                let p = lowst.as_bytes()[0];
                if p >= b'a' || p <= b'z' {
                    conf.fmt.colors.important_limit = p - b'a';
                }
            }
        }
    }

    if let Some(soon_v) = rng.get("soon") {
        if let Some(soon) = soon_v.as_integer() {
            if soon > 0 && soon < 256 {
                conf.fmt.colors.soon_days = soon as u8;
            }
        }
    }
}

fn update_global_from_conf(global: &toml::Value, conf: &mut Conf) {
    if let Some(fn_v) = global.get("filename") {
        if let Some(fname) = fn_v.as_str() {
            if !fname.is_empty() {
                conf.todo_file = PathBuf::from(fname);
            }
        }
    }
    if let Some(crd) = global.get("creation_date_auto") {
        if let Some(auto_date) = crd.as_bool() {
            conf.todo.auto_create_date = auto_date;
        }
    }
    if conf.fmt.fields.is_empty() {
        if let Some(fv) = global.get("fields") {
            if let Some(fs) = fv.as_str() {
                if fs.find(',').is_some() {
                    conf.fmt.fields = fs.split(',').map(|s| s.to_string()).collect();
                } else if fs.find(':').is_some() {
                    conf.fmt.fields = fs.split(':').map(|s| s.to_string()).collect();
                }
            }
        }
    }
}

fn load_from_config(conf: &mut Conf) {
    let mut path: PathBuf = PathBuf::from(CONF_FILE);
    if !path.exists() {
        path = match dirs::config_dir() {
            None => return,
            Some(s) => s.to_owned(),
        };
        path.push(CONF_FILE);
        if !path.exists() {
            return;
        }
    }

    if conf.verbose {
        println!("Loading configuration from: {:?}", path);
    }

    let inp = match File::open(path) {
        Err(_) => return,
        Ok(f) => f,
    };
    let mut buffered = BufReader::new(inp);
    let mut data = String::new();
    if buffered.read_to_string(&mut data).is_err() {
        return;
    }

    let info: toml::Value = match toml::from_str(&data) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Failed to parse config file: {:?}", e);
            return;
        }
    };
    if let Some(cls) = info.get("colors") {
        update_colors_from_conf(&cls, conf);
    }
    if let Some(rng) = info.get("ranges") {
        update_ranges_from_conf(&rng, conf);
    }
    if let Some(global) = info.get("global") {
        update_global_from_conf(&global, conf);
    }
}

pub fn parse_args(args: &[String]) -> Result<Conf, terr::TodoError> {
    let program = args[0].clone();
    let mut conf = Conf::new();

    let mut opts = Options::new();
    opts.optflag("h", "help", "Show this help");
    opts.optflag("a", "all", "Select all todos including completed ones");
    opts.optflag("A", "completed", "Select only completed todos");
    // opts.optflag("t", "active", "Only active records");
    opts.optflag(
        "",
        "dry-run",
        "Dry run: do not change todo list, only show which todos would be changed",
    );
    opts.optflag("v", "verbose", "Display extra information (used file names etc)");
    opts.optflag(
        "e",
        "regex",
        "Treat the search string as regular expression. By default simple case-insensitive substring search is done",
    );
    opts.optflag(
        "",
        "wipe",
        "'clean' command deletes todos instead of moving them to 'done.txt'",
    );
    opts.optflagopt(
        "s",
        "sort",
        "sort todos by the list of fields(if the list is empty todos are sorted by their priority)",
        "FIELD1,FIELD2",
    );
    opts.optflag(
        "",
        "sort-rev",
        "Reverse todo list after sorting. It works only if the option 'sort' is set",
    );
    opts.optopt(
        "",
        "rec",
        "Select only recurrent(any) or non-recurrent(none) todos",
        "any | none",
    );
    opts.optopt("", "due", "Select records without due date(none), with any due date(any), overdue todos(overdue), today's todos(today), tomorrow's ones(tomorrow), or which are due in a few days(soon)", "any | none | today| tomorrow | soon");
    opts.optopt(
        "",
        "threshold",
        "Select records without threshold date(none), with any threshold date(any)",
        "any | none",
    );
    opts.optopt("", "pri", "Select todos without prioriry(none), with any priority(any), with a given priority, with a priority equal to or higer/lower than the given priority", "none | any | a | b+ | c-");
    opts.optopt(
        "",
        "set-pri",
        "Change priority for selected todos: remove priority, set exact priorty, increase or decrease it",
        "none | A-Z | + | -",
    );
    opts.optopt(
        "",
        "set-rec",
        "Change recurrence for selected todos: remove recurrence, or set a new one",
        "none | 1m | 15d",
    );
    opts.optopt(
        "",
        "set-due",
        "Change due date for selected todos: remove due date or set a new one",
        "none | YYYY-MM-DD",
    );
    opts.optopt(
        "",
        "set-threshold",
        "Change threshold date for selected todos: remove threshold date or set a new one",
        "none | YYYY-MM-DD",
    );
    opts.optopt("", "set-proj", "Add projects to selected todos", "PROJ1,PROJ2");
    opts.optopt("", "set-ctx", "Add contexts to selected todos", "CTX1,CTX2");
    opts.optopt("", "del-proj", "Remove projects from selected todos", "PROJ");
    opts.optopt("", "del-ctx", "Remove contexts from selected todos", "CTX");
    opts.optopt(
        "",
        "repl-proj",
        "Replace projects for selected todos: a list of comma separated pairs of old and new project values. Old value and new value are separated with '+'",
        "PROJ1+PROJECT1,PROJ2+PROJECT2",
    );
    opts.optopt(
        "",
        "repl-ctx",
        "Replace contexts for selected todos: a list of comma separated pairs of old and new context values. Old value and new value are separated with '@'",
        "CT1@CTX,CT2@ANOTHER",
    );
    opts.optflag("", "short", "Show only ID, priority and subject for todos");
    opts.optflag("", "wrap", "Word wrap a long subject within subject column");
    opts.optopt("w", "width", "Set terminal width. The application detects terminal width automatically but it is possible to limit the output width manually", "WIDTH");
    opts.optflag(
        "",
        "human",
        "Show relative date(for due and thresold dates) instead of default YYYY-MM-DD. Examples: 'today', '4d overdue', or 'in 2m'",
    );
    opts.optflag("", "compact", "Show relative date in compact mode: without 'in' or 'overdue', overdue and future dates are distinguished by their colors");
    opts.optopt(
        "",
        "fields",
        "Set custom list of fields to display(ID is always visible). The list defines visibility of fields but not their order. The order cannot be changed",
        "FIELD2,FIELD1",
    );
    opts.optflag("", "local", "Use todo from the current working directory. It is the default mode. But if you set environment variable or modified config, the option can be used to override those values and use todo.txt from current working directory");
    opts.optflag("", "no-colors", "Disable all colors");
    opts.optflag(
        "",
        "done",
        "Use file of completed todos - done.txt. In this mode the only available command is 'list'",
    );

    let matches: Matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(e) => {
            println!("{}", e);
            print_usage(&program, &opts);
            exit(1);
        }
    };

    if matches.opt_present("h") || matches.free.is_empty() {
        print_usage(&program, &opts);
        exit(0);
    }

    parse_filter(&matches, &mut conf.flt)?;
    parse_todo(&matches, &mut conf.todo)?;
    parse_sort(&matches, &mut conf.sort)?;
    parse_fmt(&matches, &mut conf.fmt)?;

    conf.dry = matches.opt_present("dry-run");
    conf.verbose = matches.opt_present("verbose");
    conf.wipe = matches.opt_present("wipe");
    conf.use_done = matches.opt_present("done");
    if conf.use_done && conf.flt.all == tfilter::TodoStatus::Active {
        conf.flt.all = tfilter::TodoStatus::Done;
    }

    load_from_config(&mut conf);
    detect_filenames(&matches, &mut conf);
    if conf.verbose {
        println!(
            "Using main file: {:?}\n   archive file: {:?}",
            conf.todo_file, conf.done_file
        );
    }

    // TODO: check validity before return
    let mut idx: usize = 0;
    if idx >= matches.free.len() {
        conf.mode = RunMode::List;
        return Ok(conf);
    }

    // first should be command
    let mode = str_to_mode(&matches.free[idx]);
    if mode == RunMode::None {
        conf.mode = RunMode::List;
    } else {
        conf.mode = mode;
        idx += 1;
    }
    if idx >= matches.free.len() {
        // TODO: validity check
        return Ok(conf);
    }

    // second should be a range
    if matches.free[idx].find(|c: char| !c.is_digit(10)).is_none() {
        // a single ID
        if let Ok(id) = matches.free[idx].parse::<usize>() {
            conf.flt.range = tfilter::ItemRange::One(id - 1);
            idx += 1;
        }
    } else if matches.free[idx]
        .find(|c: char| !c.is_digit(10) && c != '-' && c != ':')
        .is_none()
    {
        // a range in a form "ID1-ID2" or "ID1:ID2"
        let w: Vec<&str> = if matches.free[idx].find('-').is_none() {
            matches.free[idx].split(':').collect()
        } else {
            matches.free[idx].split('-').collect()
        };
        if w.len() != 2 {
            return Err(terr::TodoError::from(terr::TodoErrorKind::InvalidValue {
                value: matches.free[idx].to_owned(),
                name: "ID range".to_string(),
            }));
        }
        match (w[0].parse::<usize>(), w[1].parse::<usize>()) {
            (Ok(id1), Ok(id2)) => {
                conf.flt.range = tfilter::ItemRange::Range(id1 - 1, id2 - 1);
                idx += 1;
            }
            (_, _) => {}
        }
    }

    while idx < matches.free.len() {
        if matches.free[idx].starts_with('@') {
            let context = matches.free[idx].trim_start_matches('@');
            conf.flt.contexts.push(context.to_owned().to_lowercase());
        } else if matches.free[idx].starts_with('+') {
            let project = matches.free[idx].trim_start_matches('+');
            conf.flt.projects.push(project.to_owned().to_lowercase());
        } else if conf.mode == RunMode::Add || conf.mode == RunMode::Edit {
            conf.todo.subject = Some(matches.free[idx].clone());
        } else {
            conf.flt.regex = Some(matches.free[idx].clone());
        }

        idx += 1;
    }

    // TODO: validate
    Ok(conf)
}

fn color_from_str(s: &str) -> ColorSpec {
    let mut spc = ColorSpec::new();

    let lows = s.to_lowercase();
    for clr in lows.split_whitespace() {
        match clr {
            "black" => {
                spc.set_fg(Some(Color::Black));
            }
            "white" => {
                spc.set_fg(Some(Color::White));
            }
            "red" => {
                spc.set_fg(Some(Color::Red));
            }
            "green" => {
                spc.set_fg(Some(Color::Green));
            }
            "yellow" => {
                spc.set_fg(Some(Color::Yellow));
            }
            "blue" => {
                spc.set_fg(Some(Color::Blue));
            }
            "cyan" => {
                spc.set_fg(Some(Color::Cyan));
            }
            "magenta" => {
                spc.set_fg(Some(Color::Magenta));
            }
            "bright" | "intense" => {
                spc.set_intense(true);
            }
            "underline" => {
                spc.set_underline(true);
            }
            "bold" => {
                spc.set_bold(true);
            }
            _ => {}
        };
    }

    spc
}