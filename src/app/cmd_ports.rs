//! cmd_apply 的 fromside/toside/port 分支.

use super::cmd::CmdFlow;
use super::*;

impl App {
    pub(super) fn cmd_ports_verbs(&mut self, cmd: &str, arg: Option<&str>, rest: &str) -> CmdFlow {
        match (cmd, arg) {
            // 端口:强制某条边从哪一侧进出(auto = 交回自动)
            ("fromside", _) | ("toside", _) => {
                let Some(e) = self.view.pick.edge() else {
                    let note = self.view.lang.pick_owned(
                        "先选中一条边".to_string(),
                        "select an edge first".to_string(),
                    );
                    self.view.note = Some(note);
                    return CmdFlow::ReturnNow;
                };
                let arg = rest.trim();
                let side = if arg.is_empty() || arg == "auto" {
                    None
                } else {
                    match crate::model::Side::parse(arg) {
                        Some(s) => Some(s),
                        None => {
                            let note = self.view.lang.pick_owned(
                                format!("端口只认 auto|left|right|up|down,不认 {arg}"),
                                format!("ports take auto|left|right|up|down, not {arg}"),
                            );
                            self.view.note = Some(note);
                            return CmdFlow::ReturnNow;
                        }
                    }
                };
                let from = cmd == "fromside";
                // 点节点没有边框:强制侧没地方落,明说一句再照做(渲染会忽略)
                if side.is_some() {
                    if let Some(bi) = self.doc.edge_ends(e).map(|(a, b)| if from { a } else { b }) {
                        if self.doc.boxes[bi].style.is_point() {
                            let note = self.view.lang.pick_owned(
                                "点节点没有边框:端口会被忽略".to_string(),
                                "point nodes have no border: the port is ignored".to_string(),
                            );
                            self.view.note = Some(note);
                        }
                    }
                }
                self.begin_edit("改端口");
                if let Some(edge) = self.doc.edges.get_mut(e) {
                    if from {
                        edge.from_side = side;
                    } else {
                        edge.to_side = side;
                    }
                }
                self.doc.rebuild();
                self.end_edit();
                let name = self
                    .view
                    .lang
                    .pick_owned("起点".to_string(), "start".to_string());
                let name = if from {
                    name
                } else {
                    self.view
                        .lang
                        .pick_owned("终点".to_string(), "end".to_string())
                };
                let side_name = side.map(|s| s.name()).unwrap_or("auto");
                let note = self.view.lang.pick_owned(
                    format!("边 {e} {name}端口 = {side_name}"),
                    format!("edge {e} {name} port = {side_name}"),
                );
                self.view.note = Some(note);
            }
            // :port from=left to=top —— 一次设两端
            ("port", _) => {
                let Some(e) = self.view.pick.edge() else {
                    let note = self.view.lang.pick_owned(
                        "先选中一条边".to_string(),
                        "select an edge first".to_string(),
                    );
                    self.view.note = Some(note);
                    return CmdFlow::ReturnNow;
                };
                let text = rest.trim();
                let mut from: Option<Option<crate::model::Side>> = None;
                let mut to: Option<Option<crate::model::Side>> = None;
                for part in text.split_whitespace() {
                    let Some((k, v)) = part.split_once('=') else {
                        let note = self.view.lang.pick_owned(
                            format!("{part} 要写成 名字=值"),
                            format!("{part} should be name=value"),
                        );
                        self.view.note = Some(note);
                        return CmdFlow::ReturnNow;
                    };
                    let side = if v == "auto" {
                        None
                    } else {
                        match crate::model::Side::parse(v) {
                            Some(s) => Some(s),
                            None => {
                                let note = self.view.lang.pick_owned(
                                    format!("端口只认 auto|left|right|up|down,不认 {v}"),
                                    format!("ports take auto|left|right|up|down, not {v}"),
                                );
                                self.view.note = Some(note);
                                return CmdFlow::ReturnNow;
                            }
                        }
                    };
                    match k {
                        "from" => from = Some(side),
                        "to" => to = Some(side),
                        other => {
                            let note = self.view.lang.pick_owned(
                                format!("port 只认 from=/to=,不认 {other}"),
                                format!("port takes from=/to=, not {other}"),
                            );
                            self.view.note = Some(note);
                            return CmdFlow::ReturnNow;
                        }
                    }
                }
                self.begin_edit("改端口");
                if let Some(edge) = self.doc.edges.get_mut(e) {
                    if let Some(f) = from {
                        edge.from_side = f;
                    }
                    if let Some(t) = to {
                        edge.to_side = t;
                    }
                }
                self.doc.rebuild();
                self.end_edit();
                let note = self.view.lang.pick_owned(
                    format!("边 {e} 端口已改"),
                    format!("edge {e} ports updated"),
                );
                self.view.note = Some(note);
            }
            _ => return CmdFlow::NotMine,
        }
        CmdFlow::Handled
    }
}
