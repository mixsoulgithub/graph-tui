//! 键位与偏好配置(`~/.config/graph-tui/config.ini`).
//!
//! 两节:`[prefs]` 放程序级偏好,`[keys]` 放键位覆盖(只覆盖 normal 模式的键)。
//! 手写 INI 子集,不引第三方依赖;任何解析错误都带 `path:line`,调用方直接退出。
//!
//! 默认值的唯一来源是这里的 `Action::ALL` 与 `KeyMap::defaults`;`--print-config`
//! 从同一张表生成完整快照,文档只摘录,避免文档里的默认值过期。

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyModifiers};

use crate::lang::Lang;
use crate::model::ArrowStyle;

/// normal 模式下可以绑定的动作(输入行/命令行/菜单里的键继续硬编码)
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Action {
    Escape,
    Help,
    Center,
    Fit,
    FitAll,
    Link,
    BlockSelect,
    CycleLayer,
    EgoToggle,
    ToggleNotes,
    CycleEdge,
    LeapDown,
    LeapUp,
    JumpBack,
    JumpForward,
    MoveLeft,
    MoveRight,
    MoveUp,
    MoveDown,
    NudgeLeft,
    NudgeRight,
    NudgeUp,
    NudgeDown,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ToggleLabels,
    Search,
    NextHit,
    PrevHit,
    CycleLayout,
    Undo,
    Redo,
    Cut,
    CutBox,
    EditOrNew,
    Paste,
    ConfirmSelection,
    EditText,
    ColorMenu,
    StyleMenu,
    Command,
    QuitHint,
}

impl Action {
    /// (动作, 配置里的名字, 中文说明, 英文说明) —— 顺序就是 `--print-config` 的顺序
    pub const ALL: &'static [(Action, &'static str, &'static str, &'static str)] = &[
        (
            Action::Escape,
            "escape",
            "退邻居高亮 / 取消选区",
            "leave ego view / selection",
        ),
        (Action::Help, "help", "键位速查", "key reference"),
        (
            Action::Center,
            "center",
            "把光标框挪到画面正中",
            "centre the cursor box",
        ),
        (
            Action::Fit,
            "fit",
            "铺满聚焦的分组",
            "fit the focused group",
        ),
        (
            Action::FitAll,
            "fit_all",
            "铺满整幅图",
            "fit the whole document",
        ),
        (
            Action::Link,
            "link",
            "两段式连边",
            "link boxes (source, then target)",
        ),
        (
            Action::BlockSelect,
            "block_select",
            "区块选择",
            "block select",
        ),
        (
            Action::CycleLayer,
            "cycle_layer",
            "切换图层",
            "cycle the layer",
        ),
        (
            Action::EgoToggle,
            "ego_toggle",
            "邻居高亮开关",
            "neighbour view on/off",
        ),
        (
            Action::ToggleNotes,
            "toggle_notes",
            "注释总开关",
            "edge notes on/off",
        ),
        (
            Action::CycleEdge,
            "cycle_edge",
            "在相邻边之间轮选",
            "cycle incident edges",
        ),
        (
            Action::LeapDown,
            "leap_down",
            "半屏往下跳一个节点",
            "jump half a screen down",
        ),
        (
            Action::LeapUp,
            "leap_up",
            "半屏往上跳一个节点",
            "jump half a screen up",
        ),
        (
            Action::JumpBack,
            "jump_back",
            "回到上一个跳过的节点",
            "jump back",
        ),
        (
            Action::JumpForward,
            "jump_forward",
            "跳到下一个节点",
            "jump forward",
        ),
        (
            Action::MoveLeft,
            "move_left",
            "光标左移一格(可带计数)",
            "cursor left",
        ),
        (
            Action::MoveRight,
            "move_right",
            "光标右移一格",
            "cursor right",
        ),
        (Action::MoveUp, "move_up", "光标上移一格", "cursor up"),
        (Action::MoveDown, "move_down", "光标下移一格", "cursor down"),
        (
            Action::NudgeLeft,
            "nudge_left",
            "把选中的框左移一格",
            "move boxes left",
        ),
        (
            Action::NudgeRight,
            "nudge_right",
            "把选中的框右移一格",
            "move boxes right",
        ),
        (
            Action::NudgeUp,
            "nudge_up",
            "把选中的框上移一格",
            "move boxes up",
        ),
        (
            Action::NudgeDown,
            "nudge_down",
            "把选中的框下移一格",
            "move boxes down",
        ),
        (Action::ZoomIn, "zoom_in", "放大", "zoom in"),
        (Action::ZoomOut, "zoom_out", "缩小", "zoom out"),
        (Action::ZoomReset, "zoom_reset", "回到 1:1", "back to 1:1"),
        (
            Action::ToggleLabels,
            "toggle_labels",
            "点节点名标注开关",
            "node labels on/off",
        ),
        (
            Action::Search,
            "search",
            "搜索 / 聚焦分组",
            "search / focus a group",
        ),
        (Action::NextHit, "next_hit", "下一个命中", "next hit"),
        (Action::PrevHit, "prev_hit", "上一个命中", "previous hit"),
        (
            Action::CycleLayout,
            "cycle_layout",
            "换下一套布局",
            "cycle the layout",
        ),
        (Action::Undo, "undo", "撤销", "undo"),
        (Action::Redo, "redo", "重做", "redo"),
        (Action::Cut, "cut", "带边剪切", "cut with edges"),
        (
            Action::CutBox,
            "cut_box",
            "只剪框和文字",
            "cut box and text",
        ),
        (
            Action::EditOrNew,
            "edit_or_new",
            "改文字 / 新建框 / 建注释",
            "edit text / new box / note",
        ),
        (Action::Paste, "paste", "粘贴", "paste"),
        (
            Action::ConfirmSelection,
            "confirm_selection",
            "确认选区",
            "confirm the selection",
        ),
        (
            Action::EditText,
            "edit_text",
            "改光标框的文字",
            "edit the cursor box text",
        ),
        (Action::ColorMenu, "color_menu", "颜色菜单", "colour menu"),
        (Action::StyleMenu, "style_menu", "样式菜单", "style menu"),
        (Action::Command, "command", "命令行", "command line"),
        (
            Action::QuitHint,
            "quit_hint",
            "提示怎么退出(退出只认 :q)",
            "hint how to quit (only :q quits)",
        ),
    ];

    pub fn from_name(name: &str) -> Option<Action> {
        Self::ALL
            .iter()
            .find(|(_, n, _, _)| *n == name)
            .map(|(a, _, _, _)| *a)
    }

    pub fn name(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(a, _, _, _)| *a == self)
            .map(|(_, n, _, _)| *n)
            .unwrap_or("")
    }

    pub fn names() -> Vec<&'static str> {
        Self::ALL.iter().map(|(_, n, _, _)| *n).collect()
    }
}

/// 一个键:代码 + 修饰符.字符键忽略 Shift(按出来的字符本身就是大写/符号)
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Key {
    pub code: KeyCode,
    pub ctrl: bool,
    pub alt: bool,
}

impl Key {
    pub fn from_event(code: KeyCode, mods: KeyModifiers) -> Key {
        let ctrl = mods.contains(KeyModifiers::CONTROL);
        let alt = mods.contains(KeyModifiers::ALT);
        Key { code, ctrl, alt }
    }

    /// `ctrl+g` / `E` / `left` / `space` / `f5`
    pub fn parse(spec: &str) -> Result<Key, String> {
        let raw = spec.trim();
        if raw.is_empty() {
            return Err("empty key name".to_string());
        }
        let mut ctrl = false;
        let mut alt = false;
        let mut rest = raw;
        loop {
            let low = rest.to_ascii_lowercase();
            if let Some(tail) = low.strip_prefix("ctrl+") {
                ctrl = true;
                rest = &rest[rest.len() - tail.len()..];
            } else if let Some(tail) = low.strip_prefix("alt+") {
                alt = true;
                rest = &rest[rest.len() - tail.len()..];
            } else if low.starts_with("shift+") {
                return Err(format!(
                    "{raw}: shift+ is not supported for single keys (write the shifted character, e.g. `E`)"
                ));
            } else {
                break;
            }
        }
        if rest.is_empty() {
            return Err(format!("{raw}: missing key after the modifier"));
        }
        let code = parse_code(rest).ok_or_else(|| format!("{raw}: unknown key {rest:?}"))?;
        if ctrl && alt {
            // 两个都按着也不拦,但多半是写错了;提示一下不算错
        }
        Ok(Key { code, ctrl, alt })
    }

    /// 配置里写出来的样子(单字符键不带 shift)
    pub fn display(&self) -> String {
        let mut out = String::new();
        if self.ctrl {
            out.push_str("ctrl+");
        }
        if self.alt {
            out.push_str("alt+");
        }
        out.push_str(&code_name(&self.code));
        out
    }
}

fn parse_code(s: &str) -> Option<KeyCode> {
    let low = s.to_ascii_lowercase();
    let named = match low.as_str() {
        "left" => Some(KeyCode::Left),
        "right" => Some(KeyCode::Right),
        "up" => Some(KeyCode::Up),
        "down" => Some(KeyCode::Down),
        "home" => Some(KeyCode::Home),
        "end" => Some(KeyCode::End),
        "pageup" => Some(KeyCode::PageUp),
        "pagedown" => Some(KeyCode::PageDown),
        "insert" => Some(KeyCode::Insert),
        "delete" => Some(KeyCode::Delete),
        "enter" => Some(KeyCode::Enter),
        "esc" | "escape" => Some(KeyCode::Esc),
        "tab" => Some(KeyCode::Tab),
        "space" => Some(KeyCode::Char(' ')),
        "backspace" => Some(KeyCode::Backspace),
        _ => None,
    };
    if named.is_some() {
        return named;
    }
    if let Some(n) = low.strip_prefix('f').and_then(|d| d.parse::<u8>().ok()) {
        return (1..=12).contains(&n).then_some(KeyCode::F(n));
    }
    let mut chars = s.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return None;
    };
    if c.is_control() {
        return None;
    }
    Some(KeyCode::Char(c))
}

fn code_name(code: &KeyCode) -> String {
    match code {
        KeyCode::Left => "left".into(),
        KeyCode::Right => "right".into(),
        KeyCode::Up => "up".into(),
        KeyCode::Down => "down".into(),
        KeyCode::Home => "home".into(),
        KeyCode::End => "end".into(),
        KeyCode::PageUp => "pageup".into(),
        KeyCode::PageDown => "pagedown".into(),
        KeyCode::Insert => "insert".into(),
        KeyCode::Delete => "delete".into(),
        KeyCode::Enter => "enter".into(),
        KeyCode::Esc => "esc".into(),
        KeyCode::Tab => "tab".into(),
        KeyCode::Backspace => "backspace".into(),
        KeyCode::F(n) => format!("f{n}"),
        KeyCode::Char(' ') => "space".into(),
        KeyCode::Char(c) => c.to_string(),
        other => format!("{other:?}"),
    }
}

/// 键位表:同一个动作可以有几个键,同一个键只认一个动作
#[derive(Clone, Debug, Default)]
pub struct KeyMap {
    map: HashMap<Key, Action>,
}

impl KeyMap {
    /// 默认键位:和 0.1.0 的行为逐条一致(现有按键测试是金标准)
    pub fn defaults() -> KeyMap {
        let mut km = KeyMap::default();
        for (spec, action) in DEFAULT_BINDINGS {
            let key = Key::parse(spec).expect("default key spec");
            km.map.insert(key, *action);
        }
        km
    }

    pub fn get(&self, code: KeyCode, mods: KeyModifiers) -> Option<Action> {
        let key = Key::from_event(code, mods);
        // 鼠标/组合键之外,退格这类没绑的键就是没有
        self.map.get(&key).copied()
    }

    /// `None` = 解绑
    pub fn bind(&mut self, key: Key, action: Option<Action>) {
        match action {
            Some(a) => {
                self.map.insert(key, a);
            }
            None => {
                self.map.remove(&key);
            }
        }
    }

    /// 按 (键, 动作) 排序的当前绑定,给 `--print-config` 用
    pub fn current(&self) -> Vec<(Key, Action)> {
        let mut out: Vec<(Key, Action)> = self.map.iter().map(|(k, a)| (k.clone(), *a)).collect();
        out.sort_by_key(|(k, _)| k.display());
        out
    }
}

/// 默认绑定表(`键 = 动作`),也是 `--print-config` 里 `[keys]` 的来源
pub const DEFAULT_BINDINGS: &[(&str, Action)] = &[
    ("esc", Action::Escape),
    ("?", Action::Help),
    ("z", Action::Center),
    ("space", Action::Center),
    ("f", Action::Fit),
    ("F", Action::FitAll),
    ("e", Action::Link),
    ("v", Action::BlockSelect),
    ("V", Action::BlockSelect),
    ("ctrl+q", Action::BlockSelect),
    ("L", Action::CycleLayer),
    ("E", Action::EgoToggle),
    ("a", Action::ToggleNotes),
    ("ctrl+e", Action::CycleEdge),
    ("ctrl+d", Action::LeapDown),
    ("ctrl+u", Action::LeapUp),
    ("ctrl+o", Action::JumpBack),
    ("tab", Action::JumpForward),
    ("ctrl+i", Action::JumpForward),
    ("h", Action::MoveLeft),
    ("left", Action::MoveLeft),
    ("l", Action::MoveRight),
    ("right", Action::MoveRight),
    ("k", Action::MoveUp),
    ("up", Action::MoveUp),
    ("j", Action::MoveDown),
    ("down", Action::MoveDown),
    ("ctrl+h", Action::NudgeLeft),
    ("ctrl+l", Action::NudgeRight),
    ("ctrl+k", Action::NudgeUp),
    ("ctrl+j", Action::NudgeDown),
    ("+", Action::ZoomIn),
    ("=", Action::ZoomIn),
    ("-", Action::ZoomOut),
    ("_", Action::ZoomOut),
    ("0", Action::ZoomReset),
    ("b", Action::ToggleLabels),
    ("/", Action::Search),
    ("n", Action::NextHit),
    ("N", Action::PrevHit),
    ("r", Action::CycleLayout),
    ("u", Action::Undo),
    ("ctrl+r", Action::Redo),
    ("d", Action::Cut),
    ("x", Action::CutBox),
    ("i", Action::EditOrNew),
    ("p", Action::Paste),
    ("enter", Action::ConfirmSelection),
    ("t", Action::EditText),
    ("c", Action::ColorMenu),
    ("s", Action::StyleMenu),
    (":", Action::Command),
    ("q", Action::QuitHint),
];

/// 程序级偏好(文档的 `view` 不在这里,避免第二套真相)
#[derive(Clone, Debug, Default)]
pub struct Prefs {
    pub arrow: Option<ArrowStyle>,
    pub history: Option<usize>,
    pub lang: Option<Lang>,
    /// true = 把光标画进画面
    pub drawn: Option<bool>,
    pub mouse: Option<bool>,
}

/// 解析出来的配置:偏好覆盖 + 键位覆盖(按文件顺序)
#[derive(Clone, Debug, Default)]
pub struct Config {
    pub prefs: Prefs,
    pub keys: Vec<(Key, Option<Action>)>,
}

pub const PREFS_KEYS: &[&str] = &["arrow", "history", "lang", "cursor", "mouse"];

impl Config {
    /// 解析 INI 子集.错误带 `path:line:`
    pub fn parse(text: &str, path: &str) -> Result<Config, String> {
        let mut cfg = Config::default();
        let mut section: Option<&str> = None;
        let mut seen: HashMap<Key, usize> = HashMap::new();
        for (i, raw) in text.lines().enumerate() {
            let line = i + 1;
            let body = match raw.split_once('#') {
                Some((head, _)) => head,
                None => raw,
            }
            .trim();
            if body.is_empty() {
                continue;
            }
            let err = |m: String| format!("{path}:{line}: {m}");
            if let Some(name) = body.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
                let name = name.trim();
                match name {
                    "prefs" | "keys" => {
                        section = Some(if name == "prefs" { "prefs" } else { "keys" })
                    }
                    other => {
                        return Err(err(format!(
                            "unknown section [{other}] (only [prefs] and [keys])"
                        )))
                    }
                }
                continue;
            }
            // 用最后一个 `=` 切:键本身可能就是 `=`(放大),动作名里没有 `=`
            let Some((k, v)) = body.rsplit_once('=') else {
                return Err(err("expected `key = value`".to_string()));
            };
            let (k, v) = (k.trim(), v.trim());
            match section {
                None => return Err(err("a line outside [prefs] / [keys]".to_string())),
                Some("prefs") => {
                    cfg.set_pref(k, v).map_err(err)?;
                }
                Some(_) => {
                    let key = Key::parse(k).map_err(err)?;
                    if let Some(prev) = seen.insert(key.clone(), line) {
                        return Err(err(format!(
                            "duplicate key {k:?} (already bound on line {prev})"
                        )));
                    }
                    let action = if v.eq_ignore_ascii_case("none") {
                        None
                    } else {
                        Some(Action::from_name(v).ok_or_else(|| {
                            err(format!(
                                "unknown action {v:?}; known: {}",
                                Action::names().join(", ")
                            ))
                        })?)
                    };
                    cfg.keys.push((key, action));
                }
            }
        }
        Ok(cfg)
    }

    fn set_pref(&mut self, k: &str, v: &str) -> Result<(), String> {
        match k {
            "arrow" => {
                self.prefs.arrow = Some(
                    ArrowStyle::parse(v)
                        .ok_or_else(|| format!("arrow: ascii | arrows | triangles, not {v:?}"))?,
                );
            }
            "history" => {
                let n: usize = v
                    .parse()
                    .map_err(|_| format!("history: a positive integer, not {v:?}"))?;
                self.prefs.history = Some(n.clamp(1, 100_000));
            }
            "lang" => {
                self.prefs.lang =
                    Some(Lang::parse(v).ok_or_else(|| format!("lang: zh | en, not {v:?}"))?);
            }
            "cursor" => {
                self.prefs.drawn = Some(match v.to_ascii_lowercase().as_str() {
                    "drawn" => true,
                    "real" => false,
                    _ => return Err(format!("cursor: real | drawn, not {v:?}")),
                });
            }
            "mouse" => {
                self.prefs.mouse = Some(match v.to_ascii_lowercase().as_str() {
                    "true" | "on" | "1" => true,
                    "false" | "off" | "0" => false,
                    _ => return Err(format!("mouse: true | false, not {v:?}")),
                });
            }
            other => {
                return Err(format!(
                    "unknown pref {other:?} (known: {})",
                    PREFS_KEYS.join(", ")
                ))
            }
        }
        Ok(())
    }
}

/// 默认配置文件路径:`$XDG_CONFIG_HOME/graph-tui/config.ini`,
/// Windows 下退到 `%APPDATA%`,再退 `~/.config`
pub fn default_config_path() -> String {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("APPDATA").map(std::path::PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    base.join("graph-tui")
        .join("config.ini")
        .to_string_lossy()
        .to_string()
}

/// 按 `显式 > $GRAPH_TUI_CONFIG > XDG 默认` 找配置并解析.
/// 显式给的文件不存在算错;默认路径不存在就是没有配置
pub fn load_config(explicit: Option<&str>) -> Result<Option<Config>, String> {
    let (path, required) = match explicit {
        Some(p) => (crate::paths::expand(p), true),
        None => match std::env::var("GRAPH_TUI_CONFIG") {
            Ok(v) if !v.trim().is_empty() => (crate::paths::expand(v.trim()), true),
            _ => (default_config_path(), false),
        },
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => Config::parse(&text, &path).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !required => Ok(None),
        Err(e) => Err(format!("cannot read {path}: {e}")),
    }
}

/// 完整默认配置(所有 prefs + 所有键位),给 `--print-config` 与 `--init-config`
pub fn default_text() -> String {
    let mut out = String::new();
    out.push_str("# graph-tui config\n");
    out.push_str("#\n");
    out.push_str("# 文件只覆盖内置默认:写你想改的行就行。--print-config 打印的是完整快照,\n");
    out.push_str("# 想要一份显式配置就整份照抄。解析错误会带 path:line 并直接退出;\n");
    out.push_str("# 配置写坏了用 `--no-config` 启动。\n");
    out.push_str("\n[prefs]\n");
    out.push_str("arrow = ascii          # ascii | arrows | triangles\n");
    out.push_str("history = 200          # 撤销步数上限\n");
    out.push_str("lang = zh              # zh | en\n");
    out.push_str("cursor = real          # real(终端光标) | drawn(画进画面)\n");
    out.push_str("mouse = true           # 关掉后鼠标事件全部忽略\n");
    out.push_str("\n[keys]\n");
    out.push_str("# 只覆盖 normal 模式;输入行 / 命令行 / 搜索 / 菜单里的键不受影响。\n");
    out.push_str("# 写法:`键 = 动作`,删掉某个默认键就写成 `none`。\n");
    for (spec, action) in DEFAULT_BINDINGS {
        out.push_str(&format!("{spec} = {}\n", action.name()));
    }
    out.push_str("\n# 可用动作:\n");
    for (_, name, zh, en) in Action::ALL {
        out.push_str(&format!("#   {name:<18} {zh} / {en}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn km() -> KeyMap {
        KeyMap::defaults()
    }

    #[test]
    fn 默认键位表能解析且无重复键() {
        let mut seen: HashMap<Key, &str> = HashMap::new();
        for (spec, action) in DEFAULT_BINDINGS {
            let key = Key::parse(spec).unwrap_or_else(|e| panic!("{spec}: {e}"));
            if let Some(prev) = seen.insert(key.clone(), action.name()) {
                panic!("{spec} 绑了两次:{prev} 与 {}", action.name());
            }
        }
        for (a, _, _, _) in Action::ALL {
            assert!(
                DEFAULT_BINDINGS.iter().any(|(_, b)| *b == *a),
                "动作 {} 没有默认键",
                a.name()
            );
        }
    }

    #[test]
    fn 查键区分大小写与修饰符() {
        let km = km();
        let none = KeyModifiers::NONE;
        assert_eq!(km.get(KeyCode::Char('e'), none), Some(Action::Link));
        assert_eq!(km.get(KeyCode::Char('E'), none), Some(Action::EgoToggle));
        assert_eq!(
            km.get(KeyCode::Char('e'), KeyModifiers::CONTROL),
            Some(Action::CycleEdge)
        );
        assert_eq!(
            km.get(KeyCode::Char('d'), KeyModifiers::CONTROL),
            Some(Action::LeapDown)
        );
        // 大写字母带 SHIFT 也认(终端就是这么报的)
        assert_eq!(
            km.get(KeyCode::Char('E'), KeyModifiers::SHIFT),
            Some(Action::EgoToggle)
        );
        assert_eq!(km.get(KeyCode::Char('Z'), none), None);
    }

    #[test]
    fn 配置覆盖_解绑_重复与未知动作() {
        let cfg = Config::parse("[keys]\nh = move_down\n\nx = none\n", "cfg.ini").unwrap();
        let mut km = km();
        for (k, a) in &cfg.keys {
            km.bind(k.clone(), *a);
        }
        assert_eq!(
            km.get(KeyCode::Char('h'), KeyModifiers::NONE),
            Some(Action::MoveDown)
        );
        assert_eq!(km.get(KeyCode::Char('x'), KeyModifiers::NONE), None);

        let e = Config::parse("[keys]\nh = move_left\nh = move_right\n", "c.ini").unwrap_err();
        assert!(e.contains("c.ini:3") && e.contains("duplicate"), "{e}");

        let e = Config::parse("[keys]\nh = nope\n", "c.ini").unwrap_err();
        assert!(e.contains("unknown action"), "{e}");
        assert!(e.contains("move_left"), "错误里该列出合法动作:{e}");

        let e = Config::parse("[other]\nx = 1\n", "c.ini").unwrap_err();
        assert!(
            e.contains("c.ini:1") && e.contains("unknown section"),
            "{e}"
        );

        let e = Config::parse("[prefs]\nhistory = -3\n", "c.ini").unwrap_err();
        assert!(e.contains("c.ini:2"), "{e}");
    }

    /// 文档里的动作表不能漂:每个动作名都要在 docs/config.md 里出现
    #[test]
    fn 文档列出所有动作() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/docs/config.md");
        let doc = std::fs::read_to_string(path).expect("docs/config.md");
        for (_, name, _, _) in Action::ALL {
            assert!(
                doc.contains(&format!("`{name}`")),
                "docs/config.md 少了动作 {name}"
            );
        }
    }

    #[test]
    fn 偏好解析() {
        let cfg = Config::parse(
            "[prefs]\narrow = arrows\nhistory = 50\nlang = en\ncursor = drawn\nmouse = false\n",
            "c.ini",
        )
        .unwrap();
        assert_eq!(cfg.prefs.arrow, Some(ArrowStyle::Arrows));
        assert_eq!(cfg.prefs.history, Some(50));
        assert_eq!(cfg.prefs.lang, Some(Lang::En));
        assert_eq!(cfg.prefs.drawn, Some(true));
        assert_eq!(cfg.prefs.mouse, Some(false));
    }

    #[test]
    fn 打印出来的配置能原样读回() {
        let text = default_text();
        let cfg = Config::parse(&text, "print").expect("默认配置该能读");
        assert_eq!(cfg.keys.len(), DEFAULT_BINDINGS.len());
        for (k, a) in cfg.keys {
            let expect = DEFAULT_BINDINGS
                .iter()
                .find(|(spec, _)| Key::parse(spec).unwrap() == k)
                .map(|(_, a)| *a);
            assert_eq!(Some(a.unwrap()), expect, "{} 的动作对不上", k.display());
        }
        assert_eq!(cfg.prefs.history, Some(200));
    }
}
