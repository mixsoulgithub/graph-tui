//! cmd_apply 的 box/nodecolor/fill/textcolor/textbg/edgebg/textalign/style/edgecolor/group/dir/corner/head/via/text 分支.

use super::cmd::CmdFlow;
use super::*;

impl App {
    pub(super) fn cmd_style_verbs(&mut self, cmd: &str, arg: Option<&str>, rest: &str) -> CmdFlow {
        match (cmd, arg) {
            // ---- 编辑动词:默认作用于光标框,选了边就作用于那条边 ----
            ("box", Some(a)) => match BoxStyle::parse(a) {
                Some(bs) => {
                    self.require_edit_scale();
                    // 换样式只动尺寸与左上角:尽量保持原来的占位与视觉中心
                    let ids = self.targets().boxes;
                    for &i in &ids {
                        self.doc.set_style(i, bs);
                    }
                    // 通过菜单/命令改过样式的框也算"选中":新建框照抄它
                    if let Some(&i) = ids.first() {
                        self.remember_box_look(i);
                    }
                    let name = bs.name();
                    let n = ids.len();
                    let zh = if n > 1 {
                        format!("({n} 个框)")
                    } else {
                        String::new()
                    };
                    let en = if n > 1 {
                        format!(" ({n} boxes)")
                    } else {
                        String::new()
                    };
                    self.view.note = Some(
                        self.view
                            .lang
                            .pick_owned(format!("框 = {name}{zh}"), format!("box = {name}{en}")),
                    );
                }
                None => {
                    let note = self.view.lang.pick_owned(
                        format!("box 只认 point / single / rounded / double / heavy,不认 {a}"),
                        format!("box takes point / single / rounded / double / heavy, not {a}"),
                    );
                    self.view.note = Some(note);
                }
            },
            ("box", None) => {
                let note = self.view.lang.pick_owned(
                    "box 缺参数:point / single / rounded / double / heavy".to_string(),
                    "box needs a value: point / single / rounded / double / heavy".to_string(),
                );
                self.view.note = Some(note);
            }
            ("nodecolor", Some(a)) => {
                let color = if a.eq_ignore_ascii_case("auto") {
                    Some(None)
                } else {
                    parse_color(a).map(Some)
                };
                match color {
                    Some(c) => {
                        self.require_edit_scale();
                        let ids = self.targets().boxes;
                        for &i in &ids {
                            if i < self.doc.boxes.len() {
                                self.doc.boxes[i].color = c;
                            }
                        }
                        let text = color_name(c);
                        let n = ids.len();
                        let zh = if n > 1 {
                            format!("({n} 个框)")
                        } else {
                            String::new()
                        };
                        let en = if n > 1 {
                            format!(" ({n} boxes)")
                        } else {
                            String::new()
                        };
                        self.view.note = Some(self.view.lang.pick_owned(
                            format!("框颜色 = {text}{zh}"),
                            format!("box colour = {text}{en}"),
                        ));
                    }
                    None => {
                        let note = self.view.lang.pick_owned(
                            format!("nodecolor 要 #rrggbb 或 auto,不认 {a}"),
                            format!("nodecolor takes #rrggbb or auto, not {a}"),
                        );
                        self.view.note = Some(note);
                    }
                }
            }
            ("nodecolor", None) => {
                let note = self.view.lang.pick_owned(
                    "nodecolor 缺参数:#rrggbb 或 auto".to_string(),
                    "nodecolor needs #rrggbb or auto".to_string(),
                );
                self.view.note = Some(note);
            }
            // 框背景填充:none 等于 null;点框也允许设,由渲染忽略
            ("fill", Some(a)) => {
                let fill = if a.eq_ignore_ascii_case("none") {
                    Some(None)
                } else {
                    parse_color(a).map(Some)
                };
                match fill {
                    Some(c) => {
                        self.require_edit_scale();
                        let ids = self.targets().boxes;
                        for &i in &ids {
                            if i < self.doc.boxes.len() {
                                self.doc.boxes[i].fill = c;
                            }
                        }
                        let text = match c {
                            Some(_) => color_name(c),
                            None => "none".to_string(),
                        };
                        let n = ids.len();
                        let zh = if n > 1 {
                            format!("({n} 个框)")
                        } else {
                            String::new()
                        };
                        let en = if n > 1 {
                            format!(" ({n} boxes)")
                        } else {
                            String::new()
                        };
                        self.view.note = Some(self.view.lang.pick_owned(
                            format!("填充 = {text}{zh}"),
                            format!("fill = {text}{en}"),
                        ));
                    }
                    None => {
                        let note = self.view.lang.pick_owned(
                            format!("fill 要 #rrggbb 或 none,不认 {a}"),
                            format!("fill takes #rrggbb or none, not {a}"),
                        );
                        self.view.note = Some(note);
                    }
                }
            }
            ("fill", None) => {
                let note = self.view.lang.pick_owned(
                    "fill 缺参数:#rrggbb 或 none".to_string(),
                    "fill needs a value: #rrggbb or none".to_string(),
                );
                self.view.note = Some(note);
            }
            // 文字颜色:auto 等于 null(跟着框的边框色)
            ("textcolor", Some(a)) => {
                let color = if a.eq_ignore_ascii_case("auto") {
                    Some(None)
                } else {
                    parse_color(a).map(Some)
                };
                match color {
                    Some(c) => {
                        self.require_edit_scale();
                        let ids = self.targets().boxes;
                        for &i in &ids {
                            if let Some(ti) = self.doc.text_of(i) {
                                self.doc.texts[ti].color = c;
                            }
                        }
                        let text = color_name(c);
                        let n = ids.len();
                        let zh = if n > 1 {
                            format!("({n} 个框)")
                        } else {
                            String::new()
                        };
                        let en = if n > 1 {
                            format!(" ({n} boxes)")
                        } else {
                            String::new()
                        };
                        self.view.note = Some(self.view.lang.pick_owned(
                            format!("文字颜色 = {text}{zh}"),
                            format!("text colour = {text}{en}"),
                        ));
                    }
                    None => {
                        let note = self.view.lang.pick_owned(
                            format!("textcolor 要 #rrggbb 或 auto,不认 {a}"),
                            format!("textcolor takes #rrggbb or auto, not {a}"),
                        );
                        self.view.note = Some(note);
                    }
                }
            }
            ("textcolor", None) => {
                let note = self.view.lang.pick_owned(
                    "textcolor 缺参数:#rrggbb 或 auto".to_string(),
                    "textcolor needs #rrggbb or auto".to_string(),
                );
                self.view.note = Some(note);
            }
            // 文字底色:和框的填充、边的底色各自独立
            ("textbg", Some(a)) => {
                let color = if a.eq_ignore_ascii_case("none") || a.eq_ignore_ascii_case("auto") {
                    Some(None)
                } else {
                    parse_color(a).map(Some)
                };
                match color {
                    Some(c) => {
                        self.require_edit_scale();
                        let ids = self.targets().boxes;
                        if ids.is_empty() {
                            self.note_no_edge();
                        } else {
                            self.begin_edit("文字底色");
                            for &i in &ids {
                                if let Some(ti) = self.doc.text_of(i) {
                                    self.doc.texts[ti].bg = c;
                                }
                            }
                            self.end_edit();
                            let text = color_name(c);
                            self.view.note = Some(self.view.lang.pick_owned(
                                format!("文字底色 = {text}"),
                                format!("text background = {text}"),
                            ));
                        }
                    }
                    None => {
                        let note = self.view.lang.pick_owned(
                            format!("textbg 要 #rrggbb 或 none,不认 {a}"),
                            format!("textbg takes #rrggbb or none, not {a}"),
                        );
                        self.view.note = Some(note);
                    }
                }
            }
            ("textbg", None) => {
                let note = self.view.lang.pick_owned(
                    "textbg 缺参数:#rrggbb 或 none".to_string(),
                    "textbg needs #rrggbb or none".to_string(),
                );
                self.view.note = Some(note);
            }
            // 边的底色:铺在这条边经过的格子上
            ("edgebg", Some(a)) => {
                let color = if a.eq_ignore_ascii_case("none") || a.eq_ignore_ascii_case("auto") {
                    Some(None)
                } else {
                    parse_color(a).map(Some)
                };
                match color {
                    Some(c) => {
                        let text = color_name(c);
                        self.edit_edges(
                            move |doc, e| doc.edges[e].bg = c,
                            format!("边底色 = {text}"),
                            format!("edge background = {text}"),
                        );
                    }
                    None => {
                        let note = self.view.lang.pick_owned(
                            format!("edgebg 要 #rrggbb 或 none,不认 {a}"),
                            format!("edgebg takes #rrggbb or none, not {a}"),
                        );
                        self.view.note = Some(note);
                    }
                }
            }
            ("edgebg", None) => {
                let note = self.view.lang.pick_owned(
                    "edgebg 缺参数:#rrggbb 或 none".to_string(),
                    "edgebg needs #rrggbb or none".to_string(),
                );
                self.view.note = Some(note);
            }
            // 框内文字水平对齐
            ("textalign", Some(a)) => match Align::parse(a) {
                Some(al) => {
                    self.require_edit_scale();
                    let ids = self.targets().boxes;
                    for &i in &ids {
                        if let Some(ti) = self.doc.text_of(i) {
                            self.doc.texts[ti].align = al;
                        }
                    }
                    let name = al.name();
                    let n = ids.len();
                    let zh = if n > 1 {
                        format!("({n} 个框)")
                    } else {
                        String::new()
                    };
                    let en = if n > 1 {
                        format!(" ({n} boxes)")
                    } else {
                        String::new()
                    };
                    self.view.note = Some(self.view.lang.pick_owned(
                        format!("文字对齐 = {name}{zh}"),
                        format!("text align = {name}{en}"),
                    ));
                }
                None => {
                    let note = self.view.lang.pick_owned(
                        format!("textalign 只认 left / center / right,不认 {a}"),
                        format!("textalign takes left / center / right, not {a}"),
                    );
                    self.view.note = Some(note);
                }
            },
            ("textalign", None) => {
                let note = self.view.lang.pick_owned(
                    "textalign 缺参数:left / center / right".to_string(),
                    "textalign needs a value: left / center / right".to_string(),
                );
                self.view.note = Some(note);
            }
            ("style", Some(a)) => match EdgeStyle::parse(a) {
                Some(st) => {
                    let name = st.name();
                    self.edit_edges(
                        |doc, e| doc.edges[e].style = st,
                        format!("线型 = {name}"),
                        format!("edge style = {name}"),
                    );
                }
                None => {
                    let note = self.view.lang.pick_owned(
                        format!("style 只认 solid / dashed / dotted,不认 {a}"),
                        format!("style takes solid / dashed / dotted, not {a}"),
                    );
                    self.view.note = Some(note);
                }
            },
            ("style", None) => {
                if self.targets().edges.is_empty() {
                    self.note_no_edge();
                } else {
                    let note = self.view.lang.pick_owned(
                        "style 缺参数:solid / dashed / dotted".to_string(),
                        "style needs a value: solid / dashed / dotted".to_string(),
                    );
                    self.view.note = Some(note);
                }
            }
            ("edgecolor", Some(a)) => {
                let color = if a.eq_ignore_ascii_case("auto") {
                    Some(None)
                } else {
                    parse_color(a).map(Some)
                };
                match color {
                    Some(c) => {
                        let text = color_name(c);
                        self.edit_edges(
                            move |doc, e| doc.edges[e].color = c,
                            format!("边颜色 = {text}"),
                            format!("edge colour = {text}"),
                        );
                    }
                    None => {
                        let note = self.view.lang.pick_owned(
                            format!("edgecolor 要 #rrggbb 或 auto,不认 {a}"),
                            format!("edgecolor takes #rrggbb or auto, not {a}"),
                        );
                        self.view.note = Some(note);
                    }
                }
            }
            // edge 的"分组"菜单项:把选中的边换到另一个分组
            ("group", Some(a)) => {
                let g = a.trim();
                if self.doc.group_idx(g).is_none() {
                    let note = self
                        .view
                        .lang
                        .pick_owned(format!("没有这个分组:{g}"), format!("no such group: {g}"));
                    self.view.note = Some(note);
                } else {
                    self.edit_edges(
                        move |doc, e| doc.edges[e].group = g.to_string(),
                        format!("分组 = {g}"),
                        format!("group = {g}"),
                    );
                }
            }
            ("group", None) => {
                let note = self.view.lang.pick_owned(
                    "group 缺参数:分组 id".to_string(),
                    "group needs a group id".to_string(),
                );
                self.view.note = Some(note);
            }
            ("edgecolor", None) => {
                if self.targets().edges.is_empty() {
                    self.note_no_edge();
                } else {
                    let note = self.view.lang.pick_owned(
                        "edgecolor 缺参数:#rrggbb 或 auto".to_string(),
                        "edgecolor needs #rrggbb or auto".to_string(),
                    );
                    self.view.note = Some(note);
                }
            }
            // 选中边的方向:四态,不再用两条反向边表达双箭头
            ("dir", Some(a)) => match Dir::parse(a) {
                Some(d) => {
                    let name = d.name();
                    self.edit_edges(
                        |doc, e| doc.edges[e].dir = d,
                        format!("边方向 = {name}"),
                        format!("edge dir = {name}"),
                    );
                }
                None => {
                    let note = self.view.lang.pick_owned(
                        format!("dir 只认 none / forward / back / both,不认 {a}"),
                        format!("dir takes none / forward / back / both, not {a}"),
                    );
                    self.view.note = Some(note);
                }
            },
            ("dir", None) => {
                if self.targets().edges.is_empty() {
                    self.note_no_edge();
                } else {
                    let note = self.view.lang.pick_owned(
                        "dir 缺参数:none / forward / back / both".to_string(),
                        "dir needs a value: none / forward / back / both".to_string(),
                    );
                    self.view.note = Some(note);
                }
            }
            // 选中边的拐角:圆角或锐角
            ("corner", Some(a)) => match Corner::parse(a) {
                Some(c) => {
                    let name = c.name();
                    self.edit_edges(
                        |doc, e| doc.edges[e].corner = c,
                        format!("边拐角 = {name}"),
                        format!("edge corner = {name}"),
                    );
                }
                None => {
                    let note = self.view.lang.pick_owned(
                        format!("corner 只认 rounded / sharp,不认 {a}"),
                        format!("corner takes rounded / sharp, not {a}"),
                    );
                    self.view.note = Some(note);
                }
            },
            ("corner", None) => {
                if self.targets().edges.is_empty() {
                    self.note_no_edge();
                } else {
                    let note = self.view.lang.pick_owned(
                        "corner 缺参数:rounded / sharp".to_string(),
                        "corner needs a value: rounded / sharp".to_string(),
                    );
                    self.view.note = Some(note);
                }
            }
            // 选中边的箭头字形:auto 跟全局走,其余三种是具体字形
            ("head", Some(a)) => {
                let parsed = if a.eq_ignore_ascii_case("auto") {
                    Some(None)
                } else {
                    crate::model::ArrowStyle::parse(a).map(Some)
                };
                match parsed {
                    Some(h) => {
                        let name = h.map(|x| x.name()).unwrap_or("auto");
                        self.edit_edges(
                            |doc, e| doc.edges[e].head = h,
                            format!("箭头 = {name}"),
                            format!("edge arrow = {name}"),
                        );
                    }
                    None => {
                        let note = self.view.lang.pick_owned(
                            format!("head 只认 auto / ascii / arrows / triangles,不认 {a}"),
                            format!("head takes auto / ascii / arrows / triangles, not {a}"),
                        );
                        self.view.note = Some(note);
                    }
                }
            }
            ("head", None) => {
                if self.targets().edges.is_empty() {
                    self.note_no_edge();
                } else {
                    let note = self.view.lang.pick_owned(
                        "head 缺参数:auto / ascii / arrows / triangles".to_string(),
                        "head needs a value: auto / ascii / arrows / triangles".to_string(),
                    );
                    self.view.note = Some(note);
                }
            }
            ("via", _) => {
                if self.targets().edges.is_empty() {
                    self.note_no_edge();
                } else {
                    let text = rest.trim();
                    if text.is_empty() {
                        let note = self.view.lang.pick_owned(
                            "via 缺参数:x,y x,y ... 或 off".to_string(),
                            "via needs x,y x,y ... or off".to_string(),
                        );
                        self.view.note = Some(note);
                    } else if text.eq_ignore_ascii_case("off") {
                        self.edit_edges(
                            |doc, e| doc.edges[e].via.clear(),
                            "折点 = off".to_string(),
                            "via = off".to_string(),
                        );
                    } else {
                        match parse_via(text) {
                            Some(v) => {
                                let n = v.len();
                                self.edit_edges(
                                    move |doc, e| doc.edges[e].via = v.clone(),
                                    format!("折点 = {n} 个"),
                                    format!("via = {n} point(s)"),
                                );
                            }
                            None => {
                                let note = self.view.lang.pick_owned(
                                    format!("via 要整数 x,y x,y ... 或 off,不认 {text}"),
                                    format!("via takes integer x,y x,y ... or off, not {text}"),
                                );
                                self.view.note = Some(note);
                            }
                        }
                    }
                }
            }
            ("text", _) => {
                // 选中边时改的是这条边的注释框(空串 = 连框一起删);否则改框里的文字
                let picked_edges = self.targets().edges;
                if !picked_edges.is_empty() {
                    let label = rest.trim().to_string();
                    self.require_edit_scale();
                    self.begin_edit("改注释");
                    let n = picked_edges.len();
                    for &e in &picked_edges {
                        if e < self.doc.edges.len() {
                            self.doc.set_note(e, &label);
                        }
                    }
                    self.end_edit();
                    self.refresh_pick();
                    self.view.note = Some(self.view.lang.pick_owned(
                        if label.is_empty() {
                            format!("注释已删({n} 条边)")
                        } else {
                            format!("注释已改({n} 条边)")
                        },
                        if label.is_empty() {
                            format!("note removed ({n} edges)")
                        } else {
                            format!("note updated ({n} edges)")
                        },
                    ));
                    return CmdFlow::ReturnNow;
                }
                if rest.trim().is_empty() {
                    let note = self.view.lang.pick_owned(
                        "text 后面要有内容(整行都会变成标签)".to_string(),
                        "text needs a label (the rest of the line becomes it)".to_string(),
                    );
                    self.view.note = Some(note);
                } else {
                    self.require_edit_scale();
                    let ids = self.targets().boxes;
                    let label = rest.to_string();
                    for &i in &ids {
                        self.doc.set_text(i, label.clone());
                    }
                    let n = ids.len();
                    let zh = if n > 1 {
                        format!("({n} 个框)")
                    } else {
                        String::new()
                    };
                    let en = if n > 1 {
                        format!(" ({n} boxes)")
                    } else {
                        String::new()
                    };
                    self.view.note = Some(
                        self.view
                            .lang
                            .pick_owned(format!("标签已改{zh}"), format!("label updated{en}")),
                    );
                }
            }
            _ => return CmdFlow::NotMine,
        }
        CmdFlow::Handled
    }
}
