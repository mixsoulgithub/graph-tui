//! The layout/cycle/side/lang branches of cmd_apply.

use super::cmd::CmdFlow;
use super::*;

impl App {
    pub(super) fn cmd_view_verbs(&mut self, cmd: &str, arg: Option<&str>, _rest: &str) -> CmdFlow {
        match (cmd, arg) {
            ("layout", Some(a)) => match Layout::parse(a) {
                Some(l) => self.apply_layout(l),
                None => {
                    let note = self.view.lang.pick_owned(
                        format!("layout 只认 geo / relax / force / layered,不认 {a}"),
                        format!("layout takes geo / relax / force / layered, not {a}"),
                    );
                    self.view.note = Some(note);
                }
            },
            ("layout", None) => {
                let n = self.view.layout.name();
                let note = self
                    .view
                    .lang
                    .pick_owned(format!("布局 = {n}"), format!("layout = {n}"));
                self.view.note = Some(note);
            }
            ("cycle", Some("auto")) => {
                self.view.cycle = None;
                let note = self.view.lang.pick_owned(
                    "cycle = auto(没聚焦 1.0,聚焦 2.0)".to_string(),
                    "cycle = auto (1.0 unfocused, 2.0 focused)".to_string(),
                );
                self.view.note = Some(note);
            }
            ("cycle", Some(a)) => match a.parse::<f64>() {
                Ok(v) if v.is_finite() => {
                    let v = v.clamp(0.0, 50.0);
                    self.view.cycle = Some(v);
                    let note = self.view.lang.pick_owned(
                        format!("cycle = {v:.2}(同分组那一档最多允许远这么多倍)"),
                        format!("cycle = {v:.2} (same-group tier may be this much farther)"),
                    );
                    self.view.note = Some(note);
                }
                _ => {
                    let note = self.view.lang.pick_owned(
                        format!("cycle:{a} 不是数字(或 auto)"),
                        format!("cycle: {a} is not a number (or auto)"),
                    );
                    self.view.note = Some(note);
                }
            },
            ("cycle", None) => {
                let text = match self.view.cycle {
                    Some(v) => format!("cycle = {v:.2}"),
                    None => self
                        .view
                        .lang
                        .pick(
                            "cycle = auto(没聚焦 1.0,聚焦 2.0)",
                            "cycle = auto (1.0 unfocused, 2.0 focused)",
                        )
                        .to_string(),
                };
                self.view.note = Some(text);
            }
            ("side", Some(a)) => match a.parse::<f64>() {
                Ok(v) if v.is_finite() => {
                    let v = v.clamp(0.0, 20.0);
                    self.view.side = v;
                    let note = self.view.lang.pick_owned(
                        format!("side = {v:.2}(横向偏斜算几倍距离,调小更认远近)"),
                        format!("side = {v:.2} (weight of sideways offset; lower = plain nearest)"),
                    );
                    self.view.note = Some(note);
                }
                _ => {
                    let note = self.view.lang.pick_owned(
                        format!("side:{a} 不是数字"),
                        format!("side: {a} is not a number"),
                    );
                    self.view.note = Some(note);
                }
            },
            ("side", None) => {
                let v = self.view.side;
                self.view.note = Some(format!("side = {v:.2}"));
            }
            ("lang", arg) => {
                let want = match arg {
                    Some(a) => Lang::parse(a),
                    None => Some(self.view.lang.other()),
                };
                match want {
                    Some(l) => {
                        self.view.lang = l;
                        let code = l.code();
                        let note = l.pick_owned(
                            format!("语言 = {code}(zh 中文,en English)"),
                            format!("language = {code} (zh / en)"),
                        );
                        self.view.note = Some(note);
                    }
                    None => {
                        let note = self
                            .view
                            .lang
                            .pick("lang 只认 zh 或 en", "lang takes zh or en")
                            .to_string();
                        self.view.note = Some(note);
                    }
                }
            }
            _ => return CmdFlow::NotMine,
        }
        CmdFlow::Handled
    }
    /// `:rotate`: with no argument rotate by 90 degrees, an angle sets the absolute value, and
    /// auto hands control back to automatic selection
    pub(super) fn cmd_rotate(&mut self, arg: Option<&str>) {
        let Some(o) = arg.map(str::trim).filter(|s| !s.is_empty()) else {
            let next = (self.view.angle + 90).rem_euclid(360);
            let o =
                layout::Orientation::parse(&next.to_string()).unwrap_or(layout::Orientation::Deg0);
            self.rotate_to(o, next);
            return;
        };
        match layout::Orientation::parse(o) {
            Some(layout::Orientation::Auto) => {
                self.view.orientation = layout::Orientation::Auto;
                self.view.note = Some(self.view.lang.pick_owned(
                    "方向 = auto(下次 r / :layout 按窗口重新选)".to_string(),
                    "orientation = auto (next r / :layout picks for the window)".to_string(),
                ));
            }
            Some(o) => {
                let deg = o.degrees();
                self.view.orientation = o;
                self.rotate_to(o, deg);
            }
            None => {
                let note = self.view.lang.pick_owned(
                    format!("rotate 只认 auto / 0 / 90 / 180 / 270,不认 {o}"),
                    format!("rotate takes auto / 0 / 90 / 180 / 270, not {o}"),
                );
                self.view.note = Some(note);
            }
        }
    }

    /// `:edge-labels [on|off]`: master switch for edge labels (same as the a key)
    pub(super) fn cmd_edge_labels(&mut self, arg: Option<&str>) {
        match arg.unwrap_or("").trim().to_ascii_lowercase().as_str() {
            "" => self.view.edge_labels = !self.view.edge_labels,
            "on" | "true" | "1" => self.view.edge_labels = true,
            "off" | "false" | "0" => self.view.edge_labels = false,
            other => {
                let note = self.view.lang.pick_owned(
                    format!("edge-labels 只认 on / off,不认 {other}"),
                    format!("edge-labels takes on / off, not {other}"),
                );
                self.view.note = Some(note);
                return;
            }
        }
        let on = self.view.edge_labels;
        self.view.note = Some(self.view.lang.pick_owned(
            if on {
                "边标签:开".to_string()
            } else {
                "边标签:关".to_string()
            },
            if on {
                "edge labels: on".to_string()
            } else {
                "edge labels: off".to_string()
            },
        ));
    }

    /// `:node-labels [on|off]`: master switch for point node names (same as the B key)
    pub(super) fn cmd_node_labels(&mut self, arg: Option<&str>) {
        match arg.unwrap_or("").trim().to_ascii_lowercase().as_str() {
            "" => self.view.node_labels = !self.view.node_labels,
            "on" | "true" | "1" => self.view.node_labels = true,
            "off" | "false" | "0" => self.view.node_labels = false,
            other => {
                let note = self.view.lang.pick_owned(
                    format!("node-labels 只认 on / off,不认 {other}"),
                    format!("node-labels takes on / off, not {other}"),
                );
                self.view.note = Some(note);
                return;
            }
        }
        let on = self.view.node_labels;
        self.view.note = Some(self.view.lang.pick_owned(
            if on {
                "节点名:开".to_string()
            } else {
                "节点名:关".to_string()
            },
            if on {
                "node labels: on".to_string()
            } else {
                "node labels: off".to_string()
            },
        ));
    }
}
