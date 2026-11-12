//! 交互状态与事件.这一层不碰画法(那是 render),只维护"现在在看哪儿,光标在哪,搜了什么".
//!
//! 数据在 `Document` 里(框 / 文字 / 边分开,坐标是整数单元格);
//! 这里只维护视图状态(中心,缩放,搜索,选区)与修改文档的命令.
//! 修改几何的动作一律要求 1:1 缩放:别的缩放倍数下只能查看.

use std::path::Path;
use std::time::Instant;

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::document::{Document, Hits, Layer, ViewState};
use crate::lang::Lang;
use crate::layout::{self, Layout};
use crate::model::{Align, BoxStyle, Corner, Dir, EdgeStyle, Side};
use crate::paths;
use crate::render;

mod cmd;
mod cmd_clip;
mod cmd_doc;
mod cmd_io;
mod cmd_ports;
mod cmd_style;
mod cmd_view;
mod edit;
mod history;
mod input;
mod io;
mod link;
mod menu;
mod mouse;
mod nav;
mod state;
mod tabs;

/// 光标呼吸一个来回的时长(秒):约 75 下/分,和心跳差不多
const BEAT: f64 = 0.8;

/// 呼吸动画量化成几档:60fps 的全屏重画在真终端上是白写的字节
const PULSE_STEPS: f64 = 16.0;

/// 平移时长的下限与上限(秒):远的只多给一点,不能越远越拖
const PAN_MIN: f64 = 0.2;
const PAN_MAX: f64 = 0.42;

/// 缩放的范围:键盘与滚轮共用
const ZOOM_MIN: f64 = 0.05;
/// 上限只到 16:字形不会跟着变大,再放大只是把框拉开、把画面清空
const ZOOM_MAX: f64 = 16.0;
/// 键盘一档
const ZOOM_STEP: f64 = 1.25;
/// 滚轮一档(触控板一格就是一次事件,步进要更细)
const WHEEL_STEP: f64 = 1.1;
/// 离 1:1 这么近就吸过去,免得停在 "x1.02" 那种要看清不清的倍率上
const SNAP: f64 = 0.06;
/// 右键拖过这么多格才算"拖不动画面",否则算单击弹菜单(切比雪夫距离)
const RIGHT_PAN_THRESHOLD: i32 = 2;

/// 一段平移动画:按**时长**缓动,不是"每帧走固定格数"
#[derive(Clone, Copy)]
pub struct Pan {
    pub from: (f64, f64),
    pub to: (f64, f64),
    pub start: Instant,
    /// 走多少秒(远的多给一点,但有封顶)
    pub dur: f64,
}

/// 左键按住之后在拖什么:按在框上是拖框,按在空白处是平移视图
enum Drag {
    /// 拖视图:起点格,按下时的视图中心,当时的投影
    Pan {
        from: (u16, u16),
        center: (f64, f64),
        proj: render::Proj,
    },
    /// 拖出一块选区:按下的那一格当锚点(真的拖起来才落成选区,单击不算)
    Select { anchor: render::Cell },
    /// 拖框:起点格、按下时这一批框的左上角单元格、以及"真的动过了吗"
    /// (只点一下不挪框,也不压撤销)
    Boxes {
        from: (u16, u16),
        at: Vec<(usize, (i32, i32))>,
        /// 按下时的投影:缩放多少都按它把屏幕格换成画布格
        proj: render::Proj,
        /// 按下时的光标与选区(一起按同一个位移移动)
        cursor0: render::Cell,
        sel0: Option<(render::Cell, render::Cell, bool)>,
        began: bool,
    },
    /// 从框的边框拖出去连边;`moved` = 真的拖起来了(没拖就是一次普通点击)
    Link { source: usize, moved: bool },
    /// 右键按在框上:拖起来是连边,没拖就是单击弹菜单
    RightLink {
        source: usize,
        /// 按下的那一格:菜单要开在这里
        press: (u16, u16),
        moved: bool,
    },
    /// 右键按在空白/边上:可能是拖动平移,也可能只是想单击弹菜单
    RightPan {
        /// 按下的那一格:菜单要开在这里,不是松开的地方
        press: (u16, u16),
        center: (f64, f64),
        proj: render::Proj,
        /// 已经拖过阈值:之后即使回到原点也不再弹菜单
        moved: bool,
    },
    /// 拖动已有边的端点改接:边下标、哪一端(true = to)
    Reconnect {
        edge: usize,
        to_end: bool,
        moved: bool,
    },
}

/// 打开文件的选择浮层:上方一行过滤词,下面是过滤后的候选
#[derive(Clone)]
pub struct FilePick {
    /// 候选(完整路径)
    pub items: Vec<String>,
    /// 当前选中第几个(打的是"过滤之后"的下标)
    pub at: usize,
    /// 过滤词
    pub filter: String,
}

impl FilePick {
    /// 过滤之后还能看到的候选(是 items 的下标)
    pub fn shown(&self) -> Vec<usize> {
        let f = self.filter.trim().to_lowercase();
        if f.is_empty() {
            return (0..self.items.len()).collect();
        }
        (0..self.items.len())
            .filter(|i| self.items[*i].to_lowercase().contains(&f))
            .collect()
    }

    /// 当前选中的路径
    pub fn current(&self) -> Option<&str> {
        let shown = self.shown();
        shown
            .get(self.at.min(shown.len().saturating_sub(1)))
            .map(|i| self.items[*i].as_str())
    }
}

/// 视图状态:render 只读它,app 改它
#[derive(Clone)]
pub struct View {
    /// 现在用哪套布局(只用于显示与 r 键循环,打开文件时不据此重算坐标)
    pub layout: Layout,
    /// 布局方向:auto 或固定角度(只影响 r / :layout 的重排)
    pub orientation: layout::Orientation,
    /// 就地改框文字(按 i 进):草稿 + 光标(按字符计)
    pub edit: Option<BoxEdit>,
    /// 箭头字形风格(`--arrow ascii|arrows|triangles`)
    pub arrow_style: crate::model::ArrowStyle,
    /// 当前实际角度(0/90/180/270),`:rotate` 按它算增量
    pub angle: i32,
    /// 正在输入的搜索词(None = 不在搜索)
    pub query: Option<String>,
    /// 搜索命中/高亮的框下标(会一直留着,方便按 n 一个个跳)
    pub hits: Vec<usize>,
    /// 聚焦的分组(只留这几组的颜色,别的压暗)
    pub focus: Vec<usize>,
    /// 当前跳到第几个
    pub hit_at: usize,
    /// 放大倍数(1 = 一份画布坐标占一列)
    pub zoom: f64,
    /// 视图中心(单元格坐标),None = 用整幅图的中心
    pub center: Option<(f64, f64)>,
    /// 平移目标(见 app::tick)
    pub target: Option<(f64, f64)>,
    /// 正在进行的平移
    pub pan: Option<Pan>,
    /// 光标在画布上的格子(真实光标指示它)
    pub cursor: render::Cell,
    /// 光标格下面是什么(每次移动后由 refresh_pick 派生)
    pub pick: render::Pick,
    /// 同一格上有好几条边时,E 在该格候选里循环到这个下标
    pub cand: usize,
    /// 是否画出所有节点名
    pub labels: bool,
    /// 光标画在 buffer 里(真光标模式关掉:光标由终端画,字形保持原样)
    pub drawn: bool,
    /// 边注释(绑在边上的文字框)画不画;默认开,`--no-notes` 关
    pub notes: bool,
    /// 状态行左边的来源路径(`~` 缩写,绝对路径);None = 空白缓冲
    pub file_display: Option<String>,
    /// 状态行右边的当前工作目录(`~` 缩写)
    pub cwd: String,
    /// 还没走完的按键序列(`12` / `100,100` / `g`),右下角显示
    pub showcmd: String,
    /// 命令行光标(按字符数,0 = 行首)
    pub cmd_pos: usize,
    /// 底行输入框的光标(按字符数)
    pub prompt_pos: usize,
    /// 搜索词的光标(按字符数)
    pub query_pos: usize,
    /// 边的默认墨色(配置 `edge_color`):没写颜色的边用它,None = 跟分组色
    pub edge_ink: Option<(u8, u8, u8)>,
    /// 文字的默认墨色(配置 `text_color`):没写颜色的文字用它,None = 跟框/边/默认亮色
    pub text_ink: Option<(u8, u8, u8)>,
    /// 本会话随机挑的一条提示(按当前语言解析好);`--dump` / 导出 / headless 恒为 None
    pub tip: Option<String>,
    /// E 隔离:只显示这个节点、邻居和相连的边,其余不画(再按 E 或 Esc 退出)
    pub ego: Option<usize>,
    /// 中心节点的 id:文档改过之后下标会漂移,靠它把中心认回来(认不回就退出隔离)
    pub ego_id: Option<String>,
    /// 光标节点"呼吸"的相位:0 = 本站原色,1 = 纯红
    pub pulse: f64,
    /// 跳过的格子(搜到的,鼠标点的,半屏跳的),Ctrl-o / Ctrl-i 前后翻

    /// 是否显示键位速查
    pub help: bool,
    /// 绕行容忍强度:同组那一档允许比全局最优远几倍
    pub cycle: Option<f64>,
    /// 侧偏权重:打分里"横向偏斜"乘几倍
    pub side: f64,
    /// ":" 命令行的输入缓冲
    pub cmd: Option<String>,
    /// 底行单行输入(新建框 / 改文字 / 连边)
    pub prompt: Option<(Prompt, String)>,
    /// 右键动作菜单
    pub menu: Option<Menu>,
    /// `:` 命令行的补全弹窗:候选 + 已经用到第几个
    pub cmd_menu: Option<(Vec<String>, usize)>,
    /// 命令行右侧的路径提示(会写到哪 / 从哪里读),每敲一个字重算一次
    pub cmd_hint: Option<String>,
    /// 源文件在磁盘上被改过(还没重新载入 / 覆盖),状态行右侧一直标着
    pub stale: bool,
    /// 多文档:第几个 / 共几个(只用于状态栏,不落盘)
    pub tab: Option<(usize, usize)>,
    /// 当前 tab 的名字(文件名字或 scratch)
    pub tab_name: String,
    /// 当前 tab 有没保存的改动
    pub tab_dirty: bool,
    /// 正在拖框:暂时不画选区矩形(拖动时它只会干扰)
    pub drag_hide_sel: bool,
    /// 低倍率时要不要降细节(LOD);dump / 导出关掉它,永远画全套文字
    pub lod: bool,
    /// 上一帧有多少个点节点名字没放下(状态栏空闲时提示)
    pub labels_hidden: usize,
    /// 渲染字形:Unicode 制表符(默认)或纯 ASCII(`--ascii`)
    pub glyphs: crate::glyphs::Glyphs,
    /// 底部提示(命令回显)
    pub note: Option<String>,
    /// 界面语言:只影响应用自己的提示文字
    pub lang: Lang,
    /// 呼吸相位从哪个时刻开始算(tick 用)
    pub started: Instant,

    /// 区块选择(锚点格 + 另一端 + 是否已确认)
    pub sel: Option<RectSel>,
    /// e 的两段式连边(源已定,等目标)
    pub link: Option<LinkState>,
    /// 只看/只动哪一层(None = 全部)
    pub layer: Option<String>,
    /// 打开文件的选择浮层(`:e` 不带参数时弹出)
    pub file_pick: Option<FilePick>,
}

impl Default for View {
    fn default() -> Self {
        View {
            layout: Layout::Geo,
            orientation: layout::Orientation::Auto,
            arrow_style: Default::default(),
            edit: None,
            angle: 0,
            query: None,
            hits: Vec::new(),
            focus: Vec::new(),
            hit_at: 0,
            zoom: 1.0,
            center: None,
            target: None,
            pan: None,
            cursor: (0, 0),
            pick: render::Pick::None,
            cand: 0,
            labels: false,
            drawn: true,
            notes: true,
            file_display: None,
            cwd: String::new(),
            showcmd: String::new(),
            cmd_pos: 0,
            prompt_pos: 0,
            query_pos: 0,
            edge_ink: None,
            text_ink: None,
            ego: None,
            ego_id: None,
            tip: None,
            pulse: 0.0,

            help: false,
            cycle: None,
            side: 2.0,
            cmd: None,
            prompt: None,
            menu: None,
            cmd_menu: None,
            cmd_hint: None,
            stale: false,
            tab: None,
            tab_name: String::new(),
            tab_dirty: false,
            drag_hide_sel: false,
            lod: true,
            labels_hidden: 0,
            glyphs: crate::glyphs::Glyphs::default(),
            note: None,
            lang: Lang::default(),
            started: Instant::now(),
            sel: None,
            link: None,
            layer: None,
            file_pick: None,
        }
    }
}

/// 注释开关的三种来源:auto = 听文档的 view.notes,on/off = 命令行硬压
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NotesMode {
    #[default]
    Auto,
    On,
    Off,
}

impl NotesMode {
    pub fn parse(v: &str) -> Option<NotesMode> {
        match v.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(NotesMode::Auto),
            "on" | "true" | "1" => Some(NotesMode::On),
            "off" | "false" | "0" => Some(NotesMode::Off),
            _ => None,
        }
    }

    pub fn apply(self, notes: bool) -> bool {
        match self {
            NotesMode::Auto => notes,
            NotesMode::On => true,
            NotesMode::Off => false,
        }
    }
}

pub struct Options {
    pub layout: Option<Layout>,
    pub labels: bool,
    /// 注释开关:auto = 听文档的
    pub notes: NotesMode,
    /// false = 用终端真光标,不做 buffer 里的绘制光标
    pub drawn: bool,
    /// 开局把光标停在这个框上(名字/别名,可模糊)
    pub at: Option<String>,
    /// 开局就搜一遍(等于按 / 输入再回车)
    pub find: Option<String>,
    pub lang: Lang,
    /// 撤销步数上限(默认 200)
    pub history: Option<usize>,
    /// 键位(来自 config.ini,默认表在 keys.rs)
    pub keys: crate::keys::KeyMap,
    /// 鼠标开关(默认开)
    pub mouse: bool,
    /// 右键拖框连边(默认开;关掉后右键拖只平移)
    pub mouse_edges: bool,
    /// 箭头字形风格
    pub arrow: Option<crate::model::ArrowStyle>,
    /// 布局间距(同层列距 / 层间行距 / 外边距)
    pub spacing: layout::Spacing,
    /// 渲染字形(配置里的 render = unicode|ascii)
    pub glyphs: crate::glyphs::Glyphs,
    /// 边的默认墨色(None = 跟分组色)
    pub edge_ink: Option<(u8, u8, u8)>,
    /// 文字的默认墨色(None = 跟框/边/默认亮色)
    pub text_ink: Option<(u8, u8, u8)>,
    /// 打开链接的命令(None/空 = auto)
    pub opener: Option<String>,
    /// `[links]`:自定义 scheme -> 命令(小写 scheme)
    pub links: Vec<(String, String)>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            layout: None,
            labels: false,
            notes: NotesMode::Auto,
            drawn: false,
            at: None,
            find: None,
            lang: Lang::default(),
            history: None,
            keys: crate::keys::KeyMap::defaults(),
            mouse: true,
            mouse_edges: true,
            arrow: None,
            spacing: layout::Spacing::default(),
            glyphs: crate::glyphs::Glyphs::default(),
            edge_ink: None,
            text_ink: None,
            opener: None,
            links: Vec::new(),
        }
    }
}

/// 就地改框文字:改的是草稿,Esc 才写回文档(一步撤销)
#[derive(Clone, Debug, PartialEq)]
pub struct BoxEdit {
    /// 正在改哪个框(下标)
    pub box_idx: usize,
    /// 这个框是这次按 i 建的:取消时整步撤掉,不留空框
    pub created: bool,
    /// 草稿(原始文字,markdown 标记在这里按字面显示,光标才能对得上)
    pub text: String,
    /// 光标位置,按字符计
    pub caret: usize,
}

impl BoxEdit {
    /// 草稿按框型算出来的屏幕尺寸(格):文字节点就是文字自己,带框的加两格边框
    pub fn text_size(&self, doc: &crate::document::Document, i: usize) -> (i32, i32) {
        let mut widest = 0i32;
        for line in self.text.split('\n') {
            widest = widest.max(unicode_width::UnicodeWidthStr::width(line) as i32);
        }
        let lines = self.text.split('\n').count().max(1) as i32;
        let bare = !doc
            .boxes
            .get(i)
            .map(|b| b.style.is_boxed())
            .unwrap_or(false);
        // 多留一格:行尾的光标才有地方站,不会压到边框上
        if bare {
            (widest + 1, lines.max(1))
        } else {
            ((widest + 3).max(4), (lines + 2).max(3))
        }
    }
}

/// Ctrl+G 交出去的编辑对象
#[derive(Clone, Debug, PartialEq)]
pub enum EditTarget {
    /// 底行输入框:编辑器回来直接按 Enter 的方式提交
    Prompt(Prompt),
    /// 框里的字:编辑器回来直接写进那几个框(一步撤销)
    Boxes(Vec<usize>),
}

/// 底行单行输入(和 `:` 同一套)
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Prompt {
    NewBox,
    /// 改一批框的文字(选区里有几个就改几个)
    Text(Vec<usize>),
    Link(usize),
}

/// 右键菜单的一项:直接执行一条动词,或者进下一层
#[derive(Clone, Debug)]
pub enum MenuAction {
    Cmd(String),
    Sub(Vec<MenuItem>),
    /// 连边模式:true = 确认建边,false = 取消
    Link(bool),
}

#[derive(Clone, Debug)]
pub struct MenuItem {
    pub title: String,
    pub action: MenuAction,
    /// 色块:用两个空格的底色画,不用宽字符
    pub swatch: Option<(u8, u8, u8)>,
    /// 当前值(列表里打 `*`)
    pub checked: bool,
}

impl MenuItem {
    fn new(title: impl Into<String>, action: MenuAction) -> MenuItem {
        MenuItem {
            title: title.into(),
            action,
            swatch: None,
            checked: false,
        }
    }

    fn swatch(mut self, c: (u8, u8, u8)) -> MenuItem {
        self.swatch = Some(c);
        self
    }

    fn checked(mut self, on: bool) -> MenuItem {
        self.checked = on;
        self
    }
}

/// 右键动作菜单:就地弹在点击处,一次只显示一层
#[derive(Clone)]
pub struct Menu {
    /// 屏幕左上角
    pub at: (u16, u16),
    /// 每一层(0 是顶层)
    pub levels: Vec<Vec<MenuItem>>,
    /// 当前层里的第几项
    pub sel: usize,
    /// 弹菜单那一刻的文档:预览 = 回到它,再套用当前项
    pub snapshot: Document,
    /// 弹菜单那一刻的光标与选中项(预览可能把它们改掉,回来时要一起复原)
    pub snapshot_cursor: (render::Cell, render::Pick),
    /// 菜单顶上的属性名(颜色菜单只一层,状态栏靠它显示"边框色 #rrggbb")
    pub title: Option<String>,
    /// 进来的路径(每进一层子菜单压一个属性名,退出去弹一个)
    pub parents: Vec<String>,
    /// 每一层进来时停在哪一项:退回去(h)要回到原来那一项,不能回到第一项
    pub parents_sel: Vec<usize>,
    /// 这是哪个菜单(box / edge / colour-… / canvas):记"上次选了什么"要用它分组
    pub kind: String,
}

impl Menu {
    pub fn level(&self) -> &[MenuItem] {
        self.levels.last().map(Vec::as_slice).unwrap_or(&[])
    }
}

/// 区块选择:锚点、当前另一端,以及是否已按 Enter 确认
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RectSel {
    pub anchor: render::Cell,
    pub end: render::Cell,
    pub confirmed: bool,
}

/// e 的两段式连边状态:先选源,再选目标.
/// 只活在这一次会话里,不写 `.gtui`
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LinkState {
    /// 源框下标(已去重排序)
    pub sources: Vec<usize>,
    /// 显式勾选的目标(Shift+左键);空就按光标 / 选区现算
    pub targets: Vec<usize>,
    /// 数量大时等第二次 Enter
    pub confirming: bool,
}

/// 一次命令要作用到的元素集合
#[derive(Default, Clone, Debug)]
pub struct TargetSet {
    pub boxes: Vec<usize>,
    pub edges: Vec<usize>,
}

impl TargetSet {
    pub fn is_empty(&self) -> bool {
        self.boxes.is_empty() && self.edges.is_empty()
    }
}

/// 删除剪贴板:单槽,只活在这个会话里
#[derive(Default, Clone)]
pub struct Clipboard {
    pub origin: render::Cell,
    pub boxes: Vec<crate::document::Box>,
    pub texts: Vec<crate::document::Text>,
    pub edges: Vec<crate::document::DocEdge>,
    pub groups: Vec<crate::document::Group>,
    /// 这些框/边用到的图层(粘贴时照原顺序重建)
    pub layers: Vec<crate::document::Layer>,
}

/// `:` 命令行的动词(补全用;顺序就是提示里的顺序)
const VERBS: &[&str] = &[
    "box",
    "corner",
    "cycle",
    "del",
    "dir",
    "head",
    "e",
    "edgecolor",
    "export",
    "export-canvas",
    "edgebg",
    "fill",
    "group",
    "history",
    "lang",
    "layer",
    "layout",
    "link",
    "new",
    "nodecolor",
    "notes",
    "q",
    "redo",
    "relabel",
    "rotate",
    "save",
    "side",
    "style",
    "text",
    "textalign",
    "textbg",
    "textcolor",
    "tolayer",
    "undo",
    "via",
    "w",
    "wq",
];

/// 撤销/重做:每步存一份改之前的文档快照
#[derive(Default)]
/// 源文件在磁盘上的样子:长度 + 修改时间.用来发现"别的程序改了这个文件"
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FileStamp {
    len: u64,
    mtime: Option<std::time::SystemTime>,
}

impl FileStamp {
    /// 文件的当前标记;读不到(不存在 / 没权限)就是 None
    pub fn of(path: &str) -> Option<FileStamp> {
        let meta = std::fs::metadata(path).ok()?;
        Some(FileStamp {
            len: meta.len(),
            mtime: meta.modified().ok(),
        })
    }
}

pub struct History {
    past: Vec<(String, Document)>,
    future: Vec<(String, Document)>,
    cap: usize,
}

impl History {
    pub fn new(cap: usize) -> History {
        History {
            past: Vec::new(),
            future: Vec::new(),
            cap: cap.max(1),
        }
    }

    /// 改文档之前压一份快照;参数是这一步的名字(回显用)
    pub fn begin(&mut self, label: &str, doc: &Document) {
        self.future.clear();
        self.past.push((label.to_string(), doc.clone()));
        if self.past.len() > self.cap {
            self.past.remove(0);
        }
    }

    /// 丢掉最后一步(事务回滚用)
    pub fn drop_last(&mut self) {
        self.past.pop();
    }

    /// 这一步其实没改文档(同值设置之类):把刚压的快照丢掉
    /// 这一步没改动就把快照丢掉;返回是否真的留了快照(即文档变了)
    pub fn prune(&mut self, doc: &Document) -> bool {
        if let Some((_, before)) = self.past.last() {
            if before == doc {
                self.past.pop();
                return false;
            }
            return true;
        }
        false
    }

    /// 撤销:返回这一步的名字
    pub fn undo(&mut self, doc: &mut Document) -> Option<String> {
        let (label, before) = self.past.pop()?;
        self.future.push((label.clone(), doc.clone()));
        *doc = before;
        Some(label)
    }

    /// 重做
    pub fn redo(&mut self, doc: &mut Document) -> Option<String> {
        let (label, after) = self.future.pop()?;
        self.past.push((label.clone(), doc.clone()));
        *doc = after;
        Some(label)
    }

    pub fn clear(&mut self) {
        self.past.clear();
        self.future.clear();
    }

    pub fn counts(&self) -> (usize, usize) {
        (self.past.len(), self.future.len())
    }
}

/// 主循环该做什么
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flow {
    Continue,
    Quit,
}

/// 新建框时照抄的样式(来自最近"选中"的那个框)
#[derive(Clone, Copy)]
pub struct BoxLook {
    pub style: crate::model::BoxStyle,
    pub color: Option<(u8, u8, u8)>,
    pub fill: Option<(u8, u8, u8)>,
    pub text_color: Option<(u8, u8, u8)>,
    pub text_bg: Option<(u8, u8, u8)>,
    pub align: crate::model::Align,
}

/// 新建边时照抄的样式(来自最近"选中"的那条边)
#[derive(Clone)]
pub struct EdgeLook {
    pub group: String,
    pub style: crate::model::EdgeStyle,
    pub corner: crate::model::Corner,
    pub color: Option<(u8, u8, u8)>,
    pub bg: Option<(u8, u8, u8)>,
    /// 新边跟着用这个箭头字形(None = 跟全局设置)
    pub head: Option<crate::model::ArrowStyle>,
}

pub struct App {
    pub doc: Document,
    pub view: View,
    /// 数据来源(状态栏显示),从 stdin 来就是 None
    pub source: Option<String>,
    /// 上一帧画面区域(鼠标坐标要按它换算;draw 时刷新)
    pub area: Rect,
    /// 还没画过第一帧(首帧要把光标拉进画面)
    first_frame: bool,
    /// 上一帧的边命中表(鼠标点边用;draw 时刷新)
    pub hit: render::FrameHit,
    /// `:q` 请求退出(主循环看到就把 Flow::Quit 交出去)
    pub quit: bool,
    /// 键位表(normal 模式;config.ini 可以改)
    pub keys: crate::keys::KeyMap,
    /// 当前提示的下标(会话内推进;`view.tip` 是它按语言解析好的样子)
    pub tip_at: usize,
    /// 鼠标开关(关掉后鼠标事件一律忽略)
    pub mouse: bool,
    /// 右键拖框连边(关掉后右键拖只平移;`none` 文字节点上误连太多就关它)
    pub mouse_edges: bool,
    /// 布局间距(r 循环 / :layout 用)
    pub spacing: layout::Spacing,
    /// 渲染字形(默认 Unicode;`--ascii` 换纯 ASCII)
    pub glyphs: crate::glyphs::Glyphs,
    /// 当前工作目录(`~` 缩写),状态行用;开机算一次
    pub cwd: String,
    /// 新建框时照抄的样式:最近点到的框,或者刚用菜单/命令改过样式的那个框
    pub box_look: Option<BoxLook>,
    /// 新建边时照抄的样式:最近选中的那条边
    pub edge_look: Option<EdgeLook>,
    /// 撤销/重做栈
    pub undo: History,
    /// 正在预览菜单项:不压撤销、不写回显
    pub previewing: bool,
    /// 删除剪贴板(单槽,只在会话里)
    pub clipboard: Option<Clipboard>,
    /// 数字前缀(5l = 连挪 5 个框)
    pending: Option<u32>,
    /// 还没走完的按键序列(`12`, `100,100`, `g`):右下角像 nvim 的 showcmd 那样显示
    seq: String,
    /// 要打开的链接(原始目标, 种类):App 只登记,终端与进程由 main 处理
    /// 等着 main 去执行的开计划(Editor / Opener / Spawn / Note);Jump 在这里就做完了
    open_request: Option<crate::target::OpenPlan>,
    /// 打开链接的命令(`[prefs] opener`;空 = auto)
    opener: Option<String>,
    /// `[links]`:自定义 scheme -> 命令(小写 scheme),来自配置文件
    links: Vec<(String, String)>,
    /// 跳转历史:跨 tab,每条记着"哪个 tab 的哪一格"
    jumps: Vec<Jump>,
    /// 当前站在历史里的哪一条
    jump_at: usize,
    /// 每个菜单上次选的那条路径(菜单名 -> 每一层的标题与下标):
    /// 下次打开按这个栈还原,连"退回上一层停在哪儿"都一致
    menu_memory: std::collections::HashMap<String, Vec<(String, usize)>>,
    /// 上一次左键按下的时间与格子(双击判定)
    last_click: Option<(std::time::Instant, (u16, u16))>,
    /// `i` 在边上刚建的注释框(还是空的时候 Esc 要删掉)
    note_draft: Vec<String>,
    /// Ctrl+G:等 main 去跑外部编辑器(带着编辑对象与初始内容)
    edit_request: Option<(EditTarget, String)>,
    /// 左键按住之后在拖什么(见 Drag)
    drag: Option<Drag>,
    /// 进搜索态之前光标在哪(Esc 取消时退回去)
    search_from: render::Cell,
    /// 搜索开始时的聚焦(数字分组搜索会改它;Esc 取消时还原)
    search_focus: Vec<usize>,
    /// 源文件在磁盘上的标记(载入 / 存盘时记下),用来发现外部修改
    src_stamp: Option<FileStamp>,
    /// 本地改了几次;和 `src_rev` 相等就是"没动过",可以自动重新载入
    pub rev: u64,
    /// 载入 / 存盘时的 rev 基线
    src_rev: u64,

    /// 按过一次 R(等第二次确认丢弃本地改动)
    reload_armed: Option<std::time::Instant>,
    /// 多文档:每个槽位要么是停靠的 tab,要么是 None(= 正在显示的那个)
    pub tabs: Vec<Option<Parked>>,
    /// 正在显示第几个 tab
    pub tab_at: usize,
}

/// 一个"停靠在后面"的 tab:活动 tab 的状态仍然放在 `App` 的字段里,槽位是 None
pub struct Parked {
    doc: Document,
    view: View,
    source: Option<String>,
    undo: History,
    rev: u64,
    src_rev: u64,
    src_stamp: Option<FileStamp>,
    reload_armed: Option<std::time::Instant>,
}

impl App {
    /// 拆分标记:文字里写它就切出一个新框
    const SPLIT_MARK: &'static str = "<|new box|>";
}

/// 这一格在矩形选择里吗(锚点与另一端谁大谁小都算)
fn rect_has(anchor: render::Cell, end: render::Cell, cell: render::Cell) -> bool {
    let (x0, x1) = (anchor.0.min(end.0), anchor.0.max(end.0));
    let (y0, y1) = (anchor.1.min(end.1), anchor.1.max(end.1));
    cell.0 >= x0 && cell.0 <= x1 && cell.1 >= y0 && cell.1 <= y1
}

/// 1:1 指一份画布坐标占一列(允许浮点误差)
fn at_one_scale(view: &View) -> bool {
    (view.zoom - 1.0).abs() < 1e-9
}

/// 接近 1:1 就吸过去:差一点点的倍率看起来最糊
fn snap_zoom(z: f64) -> f64 {
    if (z - 1.0).abs() <= SNAP {
        1.0
    } else {
        z
    }
}

fn clamp_zoom(z: f64) -> f64 {
    if z.is_finite() {
        z.clamp(ZOOM_MIN, ZOOM_MAX)
    } else {
        1.0
    }
}

/// 整幅图的中心(单元格坐标)
fn bbox_center(doc: &Document) -> (f64, f64) {
    let (x0, y0, x1, y1) = doc.bbox();
    ((x0 as f64 + x1 as f64) / 2.0, (y0 as f64 + y1 as f64) / 2.0)
}

/// 一个框显示的文字(没有就是空串)
/// 状态行里的名字:多行文字只取第一行
fn label_of(doc: &Document, i: usize) -> String {
    doc.text(i)
        .and_then(|t| t.text.split('\n').next())
        .map(str::to_string)
        .unwrap_or_default()
}

/// 颜色显示成 #rrggbb / auto
fn color_name(c: Option<(u8, u8, u8)>) -> String {
    match c {
        Some((r, g, b)) => format!("#{r:02x}{g:02x}{b:02x}"),
        None => "auto".to_string(),
    }
}

/// 名字/别名找一个框(给 --at 用)
fn find_box(doc: &Document, name: &str) -> Option<usize> {
    match doc.search(name) {
        Hits::Boxes(h) => h.first().copied(),
        _ => None,
    }
}

/// 闪的强度曲线:一个正弦,phase 是 0..1 的循环位置.
/// 0 -> 1 -> 0,两头平滑停住(正弦的斜率在端点本来就是 0),所以闪起来不生硬
fn pulse_at(phase: f64) -> f64 {
    0.5 - 0.5 * (phase * std::f64::consts::TAU).cos()
}

/// 数字前缀:5l = 连挪 5 个框.返回 true 表示这个键被当成计数吃了
fn count_digit(pending: &mut Option<u32>, c: char) -> bool {
    let Some(d) = c.to_digit(10) else {
        return false;
    };
    if d == 0 && pending.is_none() {
        return false; // 没前缀时 0 还是"复位"
    }
    *pending = Some((pending.unwrap_or(0) * 10 + d).min(999));
    true
}

/// 命令名之后的原文:只剥掉分开的那一个空格/制表符,后面的空格都算内容
fn cmd_rest<'a>(line: &'a str, cmd: &str) -> &'a str {
    let s = line.trim_start();
    let r = s.get(cmd.len()..).unwrap_or("");
    r.strip_prefix(' ')
        .or_else(|| r.strip_prefix('\t'))
        .unwrap_or(r)
}

/// "#rrggbb" -> 真彩色;'#' 可省,写得不完整就当没写
/// 第 idx 个字符的字节位置(越界就取串尾)
fn char_byte(text: &str, idx: usize) -> usize {
    text.char_indices()
        .nth(idx)
        .map(|(b, _)| b)
        .unwrap_or(text.len())
}

/// 行内插入:光标跟着走
fn edit_insert(text: &mut String, pos: &mut usize, s: &str) {
    if s.is_empty() {
        return;
    }
    let b = char_byte(text, *pos);
    text.insert_str(b, s);
    *pos += s.chars().count();
}

/// 退格:删光标前一个字符
fn edit_backspace(text: &mut String, pos: &mut usize) {
    if *pos == 0 {
        return;
    }
    let cur = char_byte(text, *pos);
    let prev = char_byte(text, *pos - 1);
    text.replace_range(prev..cur, "");
    *pos -= 1;
}

/// Delete:删光标处那个字符
fn edit_delete(text: &mut String, pos: usize) {
    if pos >= text.chars().count() {
        return;
    }
    let a = char_byte(text, pos);
    let b = char_byte(text, pos + 1);
    text.replace_range(a..b, "");
}

/// 词的字符:除空白以外的都算(命令行里的路径 `out/x.gtui` 是一个词)
fn is_word_char(c: char) -> bool {
    !c.is_whitespace()
}

/// 往前一个词首(emacs 的 M-b)
fn word_left(text: &str, mut pos: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    while pos > 0 && !is_word_char(chars[pos - 1]) {
        pos -= 1;
    }
    while pos > 0 && is_word_char(chars[pos - 1]) {
        pos -= 1;
    }
    pos
}

/// 往后一个词尾(emacs 的 M-f)
fn word_right(text: &str, mut pos: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    while pos < chars.len() && !is_word_char(chars[pos]) {
        pos += 1;
    }
    while pos < chars.len() && is_word_char(chars[pos]) {
        pos += 1;
    }
    pos
}

fn parse_color(s: &str) -> Option<(u8, u8, u8)> {
    let hex = s.trim().trim_start_matches('#').as_bytes();
    if hex.len() != 6 || !hex.iter().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| -> Option<u8> {
        let text = std::str::from_utf8(&hex[i..i + 2]).ok()?;
        u8::from_str_radix(text, 16).ok()
    };
    Some((byte(0)?, byte(2)?, byte(4)?))
}

/// "x,y x,y ..." -> 整数折点;任何一段写坏都算整串写坏
fn parse_via(s: &str) -> Option<Vec<(i32, i32)>> {
    if s.trim().is_empty() {
        return None;
    }
    s.split_whitespace()
        .map(|part| {
            let (x, y) = part.split_once(',')?;
            Some((x.trim().parse::<i32>().ok()?, y.trim().parse::<i32>().ok()?))
        })
        .collect()
}

/// PATH 里找得到这个命令吗(不调用 shell,只按 PATH 拼路径)
fn which(cmd: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(cmd))
        .find(|p| p.is_file())
}

/// 解析跳转序列:`a,b` = 列,行(和状态行的 `x,y` 一致);单独一个 `n` 按 vim 的
/// "第 n 行"算,列不动
fn parse_goto(seq: &str, cur: (i32, i32)) -> Option<(i32, i32)> {
    let seq = seq.trim_end_matches('G');
    match seq.split_once(',') {
        Some((a, b)) => {
            if b.is_empty() {
                return None;
            }
            Some((a.parse().ok()?, b.parse().ok()?))
        }
        None => Some((cur.0, seq.parse().ok()?)),
    }
}

/// 跳转历史的一条:哪个 tab 的哪一格(跨 buffer 才回得去)
#[derive(Clone, Copy, PartialEq, Eq)]
struct Jump {
    tab: usize,
    cell: render::Cell,
}

/// 搜索态下重新算一遍命中:数字=聚焦这几条分组,文字=框名/别名
fn refresh(doc: &Document, view: &mut View) {
    let text = view.query.clone().unwrap_or_default();
    view.hit_at = 0;
    match doc.search(&text) {
        // 数字:聚焦这几条分组(它们经过的框都算命中,光标不动)
        Hits::Groups(groups) => {
            view.hits = (0..doc.boxes.len())
                .filter(|i| doc.box_groups(*i).iter().any(|g| groups.contains(g)))
                .collect();
            view.focus = groups;
        }
        // 文字:只挑命中的框(它们会被染成搜索黄),光标跳到第一个.
        // 不去动 focus —— 同一条链上的框本来就同组,聚焦会把它们一起点亮,
        // 看着就像整条链都命中了
        Hits::Boxes(hits) => {
            if let Some(&first) = hits.first() {
                view.cursor = doc
                    .text(first)
                    .map(|t| t.at)
                    .unwrap_or_else(|| (doc.boxes[first].at.0 + 1, doc.boxes[first].at.1 + 1));
            }
            view.hits = hits;
        }
        Hits::Empty => view.hits.clear(),
    }
}

/// 方向键选框的完整过程:返回 (选中, 按分数排好的候选 [(框, 分数, 是否同组)]).
///
/// 打分 `距离 + side_w x 横向偏斜`.候选分两档:
///   0 档 = "同一条分组"的框;1 档 = 其它.
/// 0 档里最好的那个只要不超过全局最优的 cycle 倍就走它.
/// 按方向挑最近的节点(光标改成格子之后不再用它,`cycle`/`side` 这两个调参也只影响它;
/// 留着是因为它在方向选择上仍然是对的工具,删掉会连带丢掉一组真实的行为测试)
#[allow(dead_code)]
fn step_cfg(
    doc: &Document,
    proj: &render::Proj,
    cursor: usize,
    dir: (i32, i32),
    focus: &[usize],
    cycle: f64,
    side_w: f64,
) -> (usize, Vec<(usize, f64, bool)>) {
    if cursor >= doc.boxes.len() {
        return (cursor, Vec::new());
    }
    let cur = proj.cell(doc.center(cursor));
    let (dc, dr) = (dir.0 as f64, dir.1 as f64);
    let mut cands: Vec<(u8, f64, usize, bool)> = Vec::new();
    for i in 0..doc.boxes.len() {
        if i == cursor {
            continue;
        }
        let c = proj.cell(doc.center(i));
        let (dx, dy) = ((c.0 - cur.0) as f64, (c.1 - cur.1) as f64);
        let along = dx * dc + dy * dr;
        if along <= 0.0 {
            continue; // 不在这个方向上
        }
        let side = (dx * dr - dy * dc).abs();
        if side > along {
            continue; // 偏出去超过 45 度,不算"这个方向"
        }
        // "同一条分组"这一档:聚焦了几条就按它算;没聚焦就按光标框自己的分组算
        let mine: &[usize] = if focus.is_empty() {
            doc.box_groups(cursor)
        } else {
            focus
        };
        let focused = doc.box_groups(i).iter().any(|g| mine.contains(g));
        let tier = if focused { 0u8 } else { 1u8 };
        let score = (dx * dx + dy * dy).sqrt() + side * side_w;
        cands.push((tier, score, i, focused));
    }
    cands.sort_by(|a, b| {
        (a.0, a.1)
            .partial_cmp(&(b.0, b.1))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let ranked: Vec<(usize, f64, bool)> = cands.iter().map(|&(_, s, i, f)| (i, s, f)).collect();
    let all_min = cands
        .iter()
        .map(|&(_, s, _, _)| s)
        .fold(f64::INFINITY, f64::min);
    // 0 档最好的那个只要不超过全局最优的 cycle 倍,就走它
    if let Some(&(0, score, i, _)) = cands.first() {
        if score <= cycle * all_min {
            return (i, ranked);
        }
    }
    let best = cands
        .iter()
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|&(_, _, i, _)| i)
        .unwrap_or(cursor);
    (best, ranked)
}
fn animate(view: &mut View, doc: &Document, cell: f64) -> bool {
    let Some(to) = view.target else {
        view.pan = None;
        return false;
    };
    let cur = view.center.unwrap_or_else(|| bbox_center(doc));
    let cw = cell.max(1e-6);
    let pan = match view.pan {
        Some(p) if p.to == to => p, // 还是同一段,接着走
        _ => {
            // 新的一段:按时长决定走多久
            let cells = ((to.0 - cur.0) / cw).abs().max(((to.1 - cur.1) / cw).abs());
            let pan = Pan {
                from: cur,
                to,
                start: Instant::now(),
                dur: (0.16 + cells * 0.002).clamp(PAN_MIN, PAN_MAX),
            };
            view.pan = Some(pan);
            pan
        }
    };
    let t = (pan.start.elapsed().as_secs_f64() / pan.dur).clamp(0.0, 1.0);
    let e = t * t;
    view.center = Some((
        pan.from.0 + (pan.to.0 - pan.from.0) * e,
        pan.from.1 + (pan.to.1 - pan.from.1) * e,
    ));
    if t >= 1.0 {
        view.center = Some(pan.to);
        view.pan = None;
        view.target = None;
        return false;
    }
    true
}

/// 让视图平滑地挪到某一点(单元格坐标):已经在动的就从当前显示的位置接着走
fn pan_to(view: &mut View, doc: &Document, p: (f64, f64)) {
    if view.center.is_none() {
        view.center = Some(bbox_center(doc));
    }
    view.target = Some(p);
}

/// 把这几个点的包围盒铺满地图区:返回 (缩放, 中心).
fn fit_to(map: Rect, pts: impl Iterator<Item = (f64, f64)>) -> Option<(f64, (f64, f64))> {
    let mut b: Option<(f64, f64, f64, f64)> = None;
    for p in pts {
        b = Some(match b {
            None => (p.0, p.1, p.0, p.1),
            Some((x0, y0, x1, y1)) => (x0.min(p.0), y0.min(p.1), x1.max(p.0), y1.max(p.1)),
        });
    }
    let (x0, y0, x1, y1) = b?;
    let (bw, bh) = ((x1 - x0).max(1.0), (y1 - y0).max(1.0));
    let cols = (map.width as f64 - 4.0).max(8.0);
    let rows = (map.height as f64 - 2.0).max(4.0);
    let want = (bw / cols).max(bh / rows);
    Some((
        (1.0 / want).clamp(ZOOM_MIN, ZOOM_MAX),
        ((x0 + x1) / 2.0, (y0 + y1) / 2.0),
    ))
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // 测试里先造默认值再改几项,比堆一长串结构体字面量清楚
mod tests;
