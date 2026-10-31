//! 每张图自己的调整层.
//!
//! 同一份引擎,不同的图要看的东西不一样:有的要分层、有的要力导,有的开局就该聚焦某一组、
//! 停在某个节点、放大到某个倍数.这些旋钮先放在图旁边的 `<名字>.cfg` 里,不往公共层里塞 ——
//! 某个需求在几张图上都要用、语义稳定了,再把它提升进 `Options` / `View` / 命令行.
//!
//! 格式故意做得笨,一行一个 `key = value`,`#` 开头的行是注释:
//!
//! ```text
//! layout = layered        # geo | relax | force | layered | auto
//! labels = on             # on / off
//! at = CLOSED             # 开局光标停在这个节点(名字或别名)
//! find = FIN              # 开局先搜一遍
//! focus = open, close     # 开局聚焦的分组(逗号隔开)
//! zoom = 1.6              # 开局放大倍数
//! center = 12, 4          # 开局视图中心(画布坐标)
//! lang = en               # 界面语言 zh / en
//! ```
//!
//! 未知的键直接报错:调整层要是默默忽略拼错的键,那它就白写了.

use crate::app::{LegendMode, Options};
use crate::lang::Lang;
use crate::layout::Layout;

/// 调整层读出来的东西:每一项都是"没写就是 None"
#[derive(Clone, Debug, Default)]
pub struct Adjust {
    pub layout: Option<String>,
    pub labels: Option<bool>,
    pub legend: Option<LegendMode>,
    /// 边标签:on / off
    pub notes: Option<crate::app::NotesMode>,
    /// 光标画法:real / drawn
    pub cursor: Option<String>,
    /// 撤销步数上限
    pub history: Option<usize>,
    /// 箭头字形风格
    pub arrow: Option<crate::model::ArrowStyle>,
    pub at: Option<String>,
    pub find: Option<String>,
    pub focus: Vec<String>,
    pub zoom: Option<f64>,
    pub center: Option<(f64, f64)>,
    pub lang: Option<String>,
}

/// 认得的键,报错时列给用户看
const KEYS: &[&str] = &[
    "layout", "labels", "legend", "notes", "cursor", "history", "arrow", "at", "find", "focus",
    "zoom", "center", "lang",
];

/// `tcp.dot` -> `tcp.cfg`(放在同一目录下)
pub fn default_path(dot_path: &str) -> String {
    match dot_path.rfind('.') {
        Some(i) if !dot_path[i..].contains('/') => format!("{}.cfg", &dot_path[..i]),
        _ => format!("{dot_path}.cfg"),
    }
}

fn truthy(v: &str) -> Option<bool> {
    match v.to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" | "1" => Some(true),
        "off" | "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

/// 解析调整层文本.报错都带行号,方便对着报错改文件
pub fn parse(text: &str) -> Result<Adjust, String> {
    let mut a = Adjust::default();
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("line {n}: want `key = value`, got `{line}`"))?;
        let key = key.trim();
        let value = value.trim();
        let bad = || {
            format!(
                "line {n}: bad value `{value}` for {key} (known keys: {})",
                KEYS.join(", ")
            )
        };
        match key {
            "layout" => {
                if value != "auto" && Layout::parse(value).is_none() {
                    return Err(bad());
                }
                a.layout = Some(value.to_string());
            }
            "labels" => a.labels = Some(truthy(value).ok_or_else(bad)?),
            "notes" => {
                a.notes = Some(
                    crate::app::NotesMode::parse(value)
                        .ok_or_else(|| format!("notes: expected auto/on/off, got {value:?}"))?,
                )
            }
            "legend" => {
                a.legend = Some(
                    LegendMode::parse(value)
                        .ok_or_else(|| format!("legend: expected auto/on/off, got {value:?}"))?,
                )
            }
            "arrow" => {
                a.arrow =
                    Some(crate::model::ArrowStyle::parse(value).ok_or_else(|| {
                        format!("arrow: ascii / arrows / triangles, got {value:?}")
                    })?);
            }
            "history" => {
                let n: usize = value.parse().map_err(|_| bad())?;
                if n == 0 {
                    return Err(bad());
                }
                a.history = Some(n);
            }
            "cursor" => match value {
                "real" => a.cursor = Some(value.to_string()),
                "drawn" => a.cursor = Some(value.to_string()),
                _ => return Err(bad()),
            },
            "at" => a.at = Some(value.to_string()),
            "find" => a.find = Some(value.to_string()),
            "focus" => {
                a.focus = value
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect()
            }
            "zoom" => {
                let v: f64 = value.parse().map_err(|_| bad())?;
                if !(v.is_finite() && v > 0.0) {
                    return Err(bad());
                }
                a.zoom = Some(v);
            }
            "center" => {
                let (x, y) = value.split_once(',').ok_or_else(bad)?;
                let f = |s: &str| s.trim().parse::<f64>().map_err(|_| bad());
                a.center = Some((f(x)?, f(y)?));
            }
            "lang" => a.lang = Some(Lang::parse(value).ok_or_else(bad)?.code().to_string()),
            other => {
                return Err(format!(
                    "line {n}: unknown key `{other}` (known keys: {})",
                    KEYS.join(", ")
                ))
            }
        }
    }
    Ok(a)
}

/// 读一个调整层文件
pub fn load(path: &str) -> Result<Adjust, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    parse(&text).map_err(|e| format!("{path}: {e}"))
}

impl Adjust {
    /// 把调整层里写了的项盖到开局设置上(命令行显式给的参数之后再盖回来)
    pub fn apply(&self, opts: &mut Options) -> Result<(), String> {
        if let Some(v) = &self.layout {
            opts.layout = if v == "auto" {
                None
            } else {
                Some(Layout::parse(v).ok_or_else(|| format!("unknown layout {v}"))?)
            };
        }
        if let Some(v) = self.legend {
            opts.legend = v;
        }
        if let Some(v) = self.notes {
            opts.notes = v;
        }
        if let Some(v) = &self.cursor {
            opts.drawn = v == "drawn";
        }
        if let Some(v) = self.history {
            opts.history = Some(v);
        }
        if let Some(v) = self.arrow {
            opts.arrow = Some(v);
        }
        if let Some(v) = self.labels {
            opts.labels = v;
        }
        if let Some(v) = &self.at {
            opts.at = Some(v.clone());
        }
        if let Some(v) = &self.find {
            opts.find = Some(v.clone());
        }
        if !self.focus.is_empty() {
            opts.focus = self.focus.clone();
        }
        opts.zoom = self.zoom;
        opts.center = self.center;
        if let Some(v) = &self.lang {
            opts.lang = Lang::parse(v).ok_or_else(|| format!("unknown language {v}"))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 同目录的_cfg_路径() {
        assert_eq!(
            default_path("examples/real/tcp.dot"),
            "examples/real/tcp.cfg"
        );
        assert_eq!(default_path("tcp.dot"), "tcp.cfg");
        assert_eq!(default_path("/a.b/c.dot"), "/a.b/c.cfg");
    }

    #[test]
    fn 读得出每一项() {
        let a = parse(
            "# 注释\n\
             layout = layered\n\
             labels = on\n\
             at = CLOSED\n\
             find = FIN\n\
             focus = open, close ,, passive\n\
             zoom = 1.5\n\
             center = 12, -4\n\
             legend = off\n\
             lang = en\n",
        )
        .unwrap();
        assert_eq!(a.layout.as_deref(), Some("layered"));
        assert_eq!(a.labels, Some(true));
        assert_eq!(a.at.as_deref(), Some("CLOSED"));
        assert_eq!(a.find.as_deref(), Some("FIN"));
        assert_eq!(a.focus, vec!["open", "close", "passive"]);
        assert_eq!(a.zoom, Some(1.5));
        assert_eq!(a.center, Some((12.0, -4.0)));
        assert_eq!(a.lang.as_deref(), Some("en"));
        assert_eq!(a.legend, Some(LegendMode::Off));
    }

    /// legend 认 auto / on / off:auto 就是"听文档的"
    /// notes 也认 auto / on / off
    #[test]
    fn notes_三态() {
        use crate::app::NotesMode;
        assert_eq!(parse("notes = auto\n").unwrap().notes, Some(NotesMode::Auto));
        assert_eq!(parse("notes = on\n").unwrap().notes, Some(NotesMode::On));
        assert_eq!(parse("notes = off\n").unwrap().notes, Some(NotesMode::Off));
        let e = parse("notes = maybe\n").unwrap_err();
        assert!(e.contains("notes"), "{e}");
    }

    #[test]
    fn legend_三态() {
        assert_eq!(
            parse("legend = auto\n").unwrap().legend,
            Some(LegendMode::Auto)
        );
        assert_eq!(parse("legend = on\n").unwrap().legend, Some(LegendMode::On));
        assert_eq!(
            parse("legend = off\n").unwrap().legend,
            Some(LegendMode::Off)
        );
        let e = parse("legend = maybe\n").unwrap_err();
        assert!(e.contains("legend"), "{e}");
    }

    #[test]
    fn 空文件就是什么都不改() {
        let a = parse("\n# 只有注释\n").unwrap();
        let mut opts = Options::default();
        a.apply(&mut opts).unwrap();
        assert!(opts.layout.is_none() && opts.at.is_none() && opts.focus.is_empty());
    }

    #[test]
    fn 拼错的键与坏值都报行号() {
        let e = parse("layout = force\nnope = 1\n").unwrap_err();
        assert!(e.contains("line 2") && e.contains("nope"), "{e}");
        let e = parse("\n\nlabels = maybe\n").unwrap_err();
        assert!(e.contains("line 3") && e.contains("maybe"), "{e}");
        let e = parse("zoom = 0\n").unwrap_err();
        assert!(e.contains("zoom"), "{e}");
        let e = parse("center = 1\n").unwrap_err();
        assert!(e.contains("center"), "{e}");
        let e = parse("layout = spiral\n").unwrap_err();
        assert!(e.contains("spiral"), "{e}");
        assert!(parse("layout = auto\n").is_ok());
    }

    #[test]
    fn 盖设置时只盖写了的项() {
        let mut opts = Options {
            labels: true,
            at: Some("keep".to_string()),
            ..Options::default()
        };
        parse("layout = layered\nzoom = 2\n")
            .unwrap()
            .apply(&mut opts)
            .unwrap();
        assert_eq!(opts.layout, Some(Layout::Layered));
        assert!(opts.labels, "没写 labels 就不该动它");
        assert_eq!(opts.at.as_deref(), Some("keep"));
        assert_eq!(opts.zoom, Some(2.0));
    }
}
