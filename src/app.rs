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
    /// E 模式:只亮这个节点、邻居和相连的边,其余压暗(再按 E 或 Esc 退出)
    pub ego: Option<usize>,
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

    /// 光标在草稿里的第几行第几列(按字符计)
    pub fn caret_row_col(&self) -> (usize, usize) {
        Self::caret_row_col_at(&self.text, self.caret)
    }

    pub fn caret_row_col_at(text: &str, caret: usize) -> (usize, usize) {
        let (mut row, mut col) = (0usize, 0usize);
        for (i, c) in text.chars().enumerate() {
            if i == caret {
                break;
            }
            if c == '\n' {
                row += 1;
                col = 0;
            } else {
                col += 1;
            }
        }
        (row, col)
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
    /// 鼠标开关(关掉后鼠标事件一律忽略)
    pub mouse: bool,
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
    pub fn new(doc: Document, opts: Options) -> Result<App, String> {
        let mut app = App {
            doc,
            view: View::default(),
            source: None,
            area: Rect::default(),
            hit: render::FrameHit::default(),
            first_frame: true,
            quit: false,
            undo: History::new(200),
            previewing: false,
            clipboard: None,
            pending: None,
            jumps: Vec::new(),
            jump_at: 0,
            seq: String::new(),
            open_request: None,
            opener: opts.opener.clone(),
            links: opts.links.clone(),
            menu_memory: std::collections::HashMap::new(),
            last_click: None,
            note_draft: Vec::new(),
            edit_request: None,
            drag: None,
            search_from: (0, 0),
            search_focus: Vec::new(),
            src_stamp: None,
            rev: 0,
            src_rev: 0,
            reload_armed: None,
            tabs: vec![None],
            tab_at: 0,
            keys: opts.keys.clone(),
            mouse: opts.mouse,
            spacing: opts.spacing,
            glyphs: opts.glyphs,
            cwd: std::env::current_dir()
                .map(|p| crate::paths::tildify(&p.to_string_lossy()))
                .unwrap_or_default(),
            box_look: None,
            edge_look: None,
        };
        app.view_from_doc();
        app.view.glyphs = app.glyphs;
        // 配置里的默认墨色:没写颜色的边/文字用它
        app.view.edge_ink = opts.edge_ink;
        app.view.text_ink = opts.text_ink;
        // 打开就 1:1 并且整幅居中(文件里存的视角只管导出,交互打开不恢复)
        app.reset_view();
        // 命令行显式给的视角优先于文件里的视图状态.
        // layout 只改显示与 r 的循环顺序:打开文件不重算坐标.
        if let Some(l) = opts.layout {
            app.view.layout = l;
        }
        if opts.labels {
            app.view.labels = true;
        }
        app.view.notes = opts.notes.apply(app.view.notes);
        app.view.drawn = opts.drawn;
        if let Some(n) = opts.history {
            app.undo = History::new(n);
        }
        if let Some(a) = opts.arrow {
            app.view.arrow_style = a;
        }
        // Options.lang 没有"没给"的表示:只有显式给了非默认语言才盖过文件
        if opts.lang != Lang::default() {
            app.view.lang = opts.lang;
        }
        // --at:按名字/别名找,找不着就把话说在提示行上,不拦着开局
        if let Some(name) = &opts.at {
            match find_box(&app.doc, name) {
                Some(i) => app.jump_to(i),
                None => {
                    let note = app.view.lang.pick_owned(
                        format!("没有叫 {name} 的框"),
                        format!("no box named {name}"),
                    );
                    app.view.note = Some(note);
                }
            }
        }
        // --find:等于按 / 输入再回车,命中留着可以按 n 跳
        if let Some(text) = &opts.find {
            if !text.trim().is_empty() {
                app.view.query = Some(text.clone());
                refresh(&app.doc, &mut app.view);
                app.view.query = None;
                if app.view.hits.is_empty() {
                    let note = app
                        .view
                        .lang
                        .pick_owned(format!("没搜到 {text}"), format!("nothing matched {text}"));
                    app.view.note = Some(note);
                }
            }
        }
        app.search_from = app.view.cursor;
        if !app.doc.boxes.is_empty() {
            app.note_here();
        }
        Ok(app)
    }

    // ---- 多文档 tab ----

    /// 把当前 tab 打包停靠到槽位(槽位里原来那个应当已经被取走)
    fn park_current(&mut self) {
        let parked = Parked {
            doc: std::mem::replace(&mut self.doc, Document::scratch()),
            view: self.view.clone(),
            source: self.source.clone(),
            undo: std::mem::replace(&mut self.undo, History::new(200)),
            rev: self.rev,
            src_rev: self.src_rev,
            src_stamp: self.src_stamp.clone(),
            reload_armed: self.reload_armed,
        };
        if self.tab_at < self.tabs.len() {
            self.tabs[self.tab_at] = Some(parked);
        }
    }

    /// 把槽位里的 tab 取出来当作活动 tab
    fn unpark(&mut self, parked: Parked) {
        self.doc = parked.doc;
        self.view = parked.view;
        self.view.glyphs = self.glyphs;
        self.source = parked.source;
        self.undo = parked.undo;
        self.rev = parked.rev;
        self.src_rev = parked.src_rev;
        self.src_stamp = parked.src_stamp;
        self.reload_armed = parked.reload_armed;
        // 帧状态不属于任何 tab:重画一帧就重建
        self.first_frame = true;
        self.hit = render::FrameHit::default();
        self.drag = None;
        self.view.drag_hide_sel = false;
        self.pending = None;
        self.clear_seq();
        self.note_draft.clear();
        self.last_click = None;
        // 临时 UI 一律收起
        self.view.cmd = None;
        self.view.cmd_menu = None;
        self.view.cmd_hint = None;
        self.view.prompt = None;
        self.view.menu = None;
        self.view.file_pick = None;
        self.view.query = None;
        self.view.hits.clear();
        self.refresh_pick();
    }

    /// 切到第 `i` 个 tab
    pub fn switch_tab(&mut self, i: usize) {
        if i >= self.tabs.len() || i == self.tab_at {
            return;
        }
        self.park_current();
        let Some(parked) = self.tabs[i].take() else {
            self.tab_at = i;
            return;
        };
        self.tab_at = i;
        self.unpark(parked);
        // 后台期间文件可能被改过:只标 stale,不自动覆盖
        self.check_disk();
        let note = self.view.lang.pick_owned(
            format!(
                "[{}/{}] {}",
                self.tab_at + 1,
                self.tabs.len(),
                self.tab_name()
            ),
            format!(
                "[{}/{}] {}",
                self.tab_at + 1,
                self.tabs.len(),
                self.tab_name()
            ),
        );
        self.view.note = Some(note);
    }

    /// 当前 tab 显示用的名字(来源文件名,没有来源就是 scratch)
    pub fn tab_name(&self) -> String {
        match &self.source {
            Some(p) => std::path::Path::new(p)
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.clone()),
            None => self.view.lang.pick("草稿", "scratch").to_string(),
        }
    }

    /// 这个 tab 有没有没保存的改动
    fn dirty(&self) -> bool {
        self.rev != self.src_rev
    }

    /// 新开一个 tab(带路径就打开它),返回新槽位
    pub fn new_tab(&mut self, path: Option<&str>) -> usize {
        // 同一个路径已经开着就切过去,不重复建
        if let Some(p) = path {
            if let Some(i) = self.tab_of_path(p) {
                self.switch_tab(i);
                return i;
            }
        }
        self.park_current();
        let at = self.tabs.len();
        self.tabs.push(None);
        self.tab_at = at;
        // 新 tab 的初始状态:空白文档或打开的文件
        let mut fresh =
            App::new(Document::scratch(), Options::default()).expect("空白文档不可能非法");
        fresh.keys = self.keys.clone();
        fresh.mouse = self.mouse;
        fresh.spacing = self.spacing;
        fresh.glyphs = self.glyphs;
        fresh.view.glyphs = self.glyphs;
        fresh.view.lang = self.view.lang;
        fresh.tabs = vec![None];
        fresh.tab_at = 0;
        let mut parked = Parked {
            doc: fresh.doc,
            view: fresh.view,
            source: None,
            undo: fresh.undo,
            rev: 0,
            src_rev: 0,
            src_stamp: None,
            reload_armed: None,
        };
        if let Some(p) = path {
            let expanded = crate::paths::expand(p);
            match Self::read_doc(&expanded) {
                Ok((doc, _)) => {
                    parked.doc = doc;
                    parked.source = Some(expanded.clone());
                    parked.src_stamp = FileStamp::of(&expanded);
                    parked.view = View {
                        glyphs: self.glyphs,
                        lang: self.view.lang,
                        ..View::default()
                    };
                }
                Err(e) => {
                    self.view.note = Some(e);
                    // 打开失败:撤掉这个空槽位,回到原来的 tab
                    self.tabs.pop();
                    self.tab_at = at.saturating_sub(1);
                    return self.tab_at;
                }
            }
        }
        self.tabs[at] = None;
        self.unpark(parked);
        let note = self.view.lang.pick_owned(
            format!("[{}/{}] {}", at + 1, self.tabs.len(), self.tab_name()),
            format!("[{}/{}] {}", at + 1, self.tabs.len(), self.tab_name()),
        );
        self.view.note = Some(note);
        at
    }

    /// 某个路径对应的 tab 下标(规范化比较)
    fn tab_of_path(&self, path: &str) -> Option<usize> {
        let want = std::fs::canonicalize(crate::paths::expand(path)).ok();
        for (i, slot) in self.tabs.iter().enumerate() {
            let src = if i == self.tab_at {
                self.source.clone()
            } else {
                slot.as_ref().and_then(|p| p.source.clone())
            };
            let Some(src) = src else { continue };
            if want.is_some() && std::fs::canonicalize(&src).ok() == want {
                return Some(i);
            }
            if src == crate::paths::expand(path) {
                return Some(i);
            }
        }
        None
    }

    /// 关掉当前 tab;脏且没强制就拒绝
    fn close_tab(&mut self, force: bool) -> bool {
        if self.dirty() && !force {
            let note = self.view.lang.pick_owned(
                "这个 tab 有没保存的改动::tabclose! 丢弃".to_string(),
                "this tab has unsaved changes: :tabclose! to discard".to_string(),
            );
            self.view.note = Some(note);
            return false;
        }
        if self.tabs.len() <= 1 {
            // 只剩一个:直接退出(和 :q 一样)
            self.quit = true;
            return true;
        }
        // 跳转历史里指向这个 tab 的条目作废,后面的下标往前挪一格
        let gone = self.tab_at;
        self.jumps.retain(|j| j.tab != gone);
        for j in &mut self.jumps {
            if j.tab > gone {
                j.tab -= 1;
            }
        }
        self.jump_at = self.jump_at.min(self.jumps.len().saturating_sub(1));
        self.tabs.remove(self.tab_at);
        // 停靠状态作废:取下一个槽位当活动 tab
        let next = self.tab_at.min(self.tabs.len() - 1);
        // 去掉了一个槽位后,当前显示的内容本来就在 App 里,重新载入 next 的内容
        let Some(parked) = self.tabs[next].take() else {
            // next 就是原来的活动槽位(被移走了):用空白文档兜底
            self.doc = Document::scratch();
            self.view = View::default();
            self.view.glyphs = self.glyphs;
            self.source = None;
            self.undo = History::new(200);
            self.rev = 0;
            self.src_rev = 0;
            self.src_stamp = None;
            self.tab_at = next;
            self.first_frame = true;
            self.hit = render::FrameHit::default();
            return true;
        };
        self.tab_at = next;
        self.unpark(parked);
        let note = format!("[{}/{}] {}", next + 1, self.tabs.len(), self.tab_name());
        self.view.note = Some(note);
        true
    }

    /// 打开文件后把视角归位:和按 `0` 完全一样(1:1、居中到整幅).
    /// 文档里存的 zoom/center 仍然会被 `--dump` / 导出用到,但交互打开时不恢复
    pub fn reset_view(&mut self) {
        self.view.zoom = 1.0;
        self.view.center = None;
        self.view.target = None;
        self.view.pan = None;
    }

    /// 把文档里的 view 状态映射进 View,并清掉不属于文档的临时状态
    fn view_from_doc(&mut self) {
        let z = self.doc.view.zoom;
        self.view.zoom = if z.is_finite() && z > 0.0 {
            clamp_zoom(z)
        } else {
            1.0
        };
        let c = self.doc.view.center;
        self.view.center = if c.0.is_finite() && c.1.is_finite() {
            Some(c)
        } else {
            None
        };
        let layout =
            Layout::parse(&self.doc.view.layout).unwrap_or_else(|| crate::layout::pick(&self.doc));
        self.view.layout = layout;
        self.view.labels = self.doc.view.labels;
        self.view.notes = self.doc.view.notes;
        if let Some(l) = Lang::parse(&self.doc.view.lang) {
            self.view.lang = l;
        }
        self.view.layer = self.doc.view.layer.clone();
        self.view.side = if self.doc.view.side.is_finite() {
            self.doc.view.side
        } else {
            2.0
        };
        self.view.cycle = self.doc.view.cycle.filter(|c| c.is_finite());
        self.view.orientation = layout::Orientation::parse(&self.doc.view.orientation)
            .unwrap_or(layout::Orientation::Auto);
        self.view.angle = self.view.orientation.degrees();
        let focus: Vec<usize> = self
            .doc
            .view
            .focus
            .iter()
            .filter_map(|id| self.doc.group_idx(id))
            .collect();
        self.view.focus = focus;
        // 文档里存的是光标格(v1 存的是框 id,这里都换成格子)
        self.view.cursor = self.doc.view.cursor_cell(&self.doc);
        self.refresh_pick();
        // 打开文档不再自动"铺满":缩放是文档状态(新文档是 1:1),字符格放大缩小都不会改变字形,
        // 铺满只会把图压成看不清的方块.想看全图按 F.
        // 这些不属于文档:换文档时一律清空
        self.view.query = None;
        self.view.hits.clear();
        self.view.hit_at = 0;
        self.view.sel = None;
        self.view.cmd = None;
        self.view.help = false;
        self.view.target = None;
        self.view.pan = None;
        self.view.pulse = 0.0;
        self.view.started = Instant::now();
        self.pending = None;
        self.clear_seq();
        self.drag = None;
        self.search_from = self.view.cursor;
        self.search_focus = self.view.focus.clone();
    }

    /// 菜单作用于哪些元素:选区里和光标 Pick 同类型的那批(没选区就是光标下那个)
    fn menu_scope(&self) -> (Vec<usize>, Vec<usize>) {
        let all = self.targets();
        match self.view.pick {
            render::Pick::Box(_) => (all.boxes, Vec::new()),
            render::Pick::Edge(_) => (Vec::new(), all.edges),
            render::Pick::None => (Vec::new(), Vec::new()),
        }
    }

    /// 一组框/边在这个属性上是不是同一个值;不是就返回 None(菜单里不打 *)
    fn common<T: PartialEq + Clone>(values: impl Iterator<Item = T>) -> Option<T> {
        let mut it = values;
        let first = it.next()?;
        if it.all(|v| v == first) {
            Some(first)
        } else {
            None
        }
    }

    /// 菜单里那一串颜色(带色块、当前值打 *)
    fn color_items(
        &self,
        prefix: &str,
        none_label: &str,
        none_line: &str,
        current: Option<(u8, u8, u8)>,
        none_when_auto: bool,
    ) -> Vec<MenuItem> {
        let mut out = vec![
            MenuItem::new(none_label, MenuAction::Cmd(none_line.to_string()))
                .checked(none_when_auto),
        ];
        for (i, c) in crate::model::PALETTE.iter().enumerate() {
            let hex = format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2);
            out.push(
                MenuItem::new(
                    format!("{i:02} {hex}"),
                    MenuAction::Cmd(format!("{prefix} {hex}")),
                )
                .swatch(*c)
                .checked(current == Some(*c)),
            );
        }
        out
    }

    /// 菜单里的图层项(选中元素挪到那一层)
    fn layer_items(&self, on: &str) -> Vec<MenuItem> {
        let cur = if on == "box" {
            Self::common(
                self.menu_scope()
                    .0
                    .iter()
                    .map(|i| self.doc.boxes[*i].layer.clone()),
            )
        } else {
            Self::common(
                self.menu_scope()
                    .1
                    .iter()
                    .map(|i| self.doc.edges[*i].layer.clone()),
            )
        };
        self.doc
            .layers
            .iter()
            .map(|l| {
                let name = if l.name.is_empty() {
                    self.view.lang.pick("(默认)", "(default)").to_string()
                } else {
                    l.name.clone()
                };
                MenuItem::new(
                    format!("{name} → {}", l.name),
                    MenuAction::Cmd(format!("tolayer {}", l.name)),
                )
                .checked(cur.as_deref() == Some(l.name.as_str()))
            })
            .collect()
    }

    /// 这是哪一类菜单:记忆按它分组,框和边各记各的
    fn menu_kind(&self, color_only: bool) -> String {
        let edge = self.view.pick.edge().is_some();
        let boxed = self.view.pick.box_at().is_some();
        match (color_only, edge, boxed) {
            (true, true, _) => "colour-edge",
            (true, _, _) => "colour-box",
            (false, true, _) => "edge",
            (false, _, true) => "box",
            _ => "canvas",
        }
        .to_string()
    }

    /// 按上次的选择还原菜单:记住的子菜单直接进去,记住的项停在那个位置
    fn menu_from_memory(
        &self,
        items: Vec<MenuItem>,
        kind: &str,
    ) -> (Vec<Vec<MenuItem>>, Vec<String>, Vec<usize>, usize) {
        let mut levels = vec![items];
        let mut parents: Vec<String> = Vec::new();
        let mut parents_sel: Vec<usize> = Vec::new();
        let mut sel: Option<usize> = None;
        if let Some(path) = self.menu_memory.get(kind) {
            for (want, want_sel) in path {
                let Some(level) = levels.last() else { break };
                let Some(found) = level.iter().position(|it| &it.title == want) else {
                    break;
                };
                // 标题对上就用记住的下标(菜单项变了也能对上标题)
                let i = if level.get(*want_sel).map(|it| &it.title) == Some(want) {
                    *want_sel
                } else {
                    found
                };
                match &level[i].action {
                    MenuAction::Sub(sub) => {
                        let sub = sub.clone();
                        parents.push(want.clone());
                        parents_sel.push(i);
                        levels.push(sub);
                    }
                    _ => {
                        sel = Some(i);
                        break;
                    }
                }
            }
        }
        let sel = sel.unwrap_or_else(|| {
            levels
                .last()
                .and_then(|l| l.iter().position(|i| i.checked))
                .unwrap_or(0)
        });
        (levels, parents, parents_sel, sel)
    }

    /// 菜单内容:按光标下的类型给一套(右键和 s 共用)
    fn menu_for(&self) -> Vec<MenuItem> {
        let cmd = |title: &str, line: &str| MenuItem::new(title, MenuAction::Cmd(line.to_string()));
        // 菜单标题跟界面语言走(值本身是命令里的关键字,保持英文)
        let t = |zh: &'static str, en: &'static str| self.view.lang.pick(zh, en);
        if self.view.link.is_some() {
            return vec![
                MenuItem::new(
                    self.view.lang.pick("确认连边", "create edges"),
                    MenuAction::Link(true),
                ),
                MenuItem::new(
                    self.view.lang.pick("取消连边", "cancel linking"),
                    MenuAction::Link(false),
                ),
            ];
        }
        let pick = self.view.pick;
        if pick.edge().is_some() {
            let (_, edges) = self.menu_scope();
            let style = Self::common(edges.iter().map(|e| self.doc.edges[*e].style));
            let dir = Self::common(edges.iter().map(|e| self.doc.edges[*e].dir));
            let corner = Self::common(edges.iter().map(|e| self.doc.edges[*e].corner));
            let head = Self::common(edges.iter().map(|e| self.doc.edges[*e].head));
            let color = Self::common(edges.iter().map(|e| self.doc.edges[*e].color));
            let auto = color == Some(None);
            let color = color.flatten();
            let bg_raw = Self::common(edges.iter().map(|e| self.doc.edges[*e].bg));
            let bg_auto = bg_raw == Some(None);
            let bg = bg_raw.flatten();
            let group = Self::common(edges.iter().map(|e| self.doc.edges[*e].group.clone()));
            let from_side = Self::common(edges.iter().map(|e| self.doc.edges[*e].from_side));
            let to_side = Self::common(edges.iter().map(|e| self.doc.edges[*e].to_side));
            let named = |name: &str, value: &str, line: &str, on: bool| {
                MenuItem::new(name, MenuAction::Cmd(format!("{line} {value}"))).checked(on)
            };
            // 端口:auto 交回自动,其余四侧;选中的几条边值不同就都不打勾
            let sides = |verb: &str, cur: Option<Option<Side>>| {
                let mut out = vec![named("auto", "auto", verb, cur == Some(None))];
                for s in [Side::Left, Side::Right, Side::Up, Side::Down] {
                    out.push(named(s.name(), s.name(), verb, cur == Some(Some(s))));
                }
                out
            };
            let mut items = vec![
                MenuItem::new(
                    t("线型", "line style"),
                    MenuAction::Sub(vec![
                        named("solid", "solid", "style", style == Some(EdgeStyle::Solid)),
                        named(
                            "dashed",
                            "dashed",
                            "style",
                            style == Some(EdgeStyle::Dashed),
                        ),
                        named(
                            "dotted",
                            "dotted",
                            "style",
                            style == Some(EdgeStyle::Dotted),
                        ),
                    ]),
                ),
                MenuItem::new(
                    t("方向", "direction"),
                    MenuAction::Sub(vec![
                        named("none", "none", "dir", dir == Some(Dir::None)),
                        named("forward", "forward", "dir", dir == Some(Dir::Forward)),
                        named("back", "back", "dir", dir == Some(Dir::Back)),
                        named("both", "both", "dir", dir == Some(Dir::Both)),
                    ]),
                ),
                MenuItem::new(
                    t("箭头", "arrowhead"),
                    MenuAction::Sub(vec![
                        named("auto", "auto", "head", head == Some(None)),
                        named(
                            "ascii",
                            "ascii",
                            "head",
                            head == Some(Some(crate::model::ArrowStyle::Ascii)),
                        ),
                        named(
                            "arrows",
                            "arrows",
                            "head",
                            head == Some(Some(crate::model::ArrowStyle::Arrows)),
                        ),
                        named(
                            "triangles",
                            "triangles",
                            "head",
                            head == Some(Some(crate::model::ArrowStyle::Triangles)),
                        ),
                    ]),
                ),
                MenuItem::new(
                    t("端口", "ports"),
                    MenuAction::Sub(vec![
                        MenuItem::new(
                            t("起点", "from"),
                            MenuAction::Sub(sides("fromside", from_side)),
                        ),
                        MenuItem::new(t("终点", "to"), MenuAction::Sub(sides("toside", to_side))),
                    ]),
                ),
                MenuItem::new(
                    t("拐角", "corner"),
                    MenuAction::Sub(vec![
                        named(
                            "rounded",
                            "rounded",
                            "corner",
                            corner == Some(crate::model::Corner::Rounded),
                        ),
                        named(
                            "sharp",
                            "sharp",
                            "corner",
                            corner == Some(crate::model::Corner::Sharp),
                        ),
                    ]),
                ),
                MenuItem::new(
                    t("颜色", "colour"),
                    MenuAction::Sub(self.color_items(
                        "edgecolor",
                        "auto",
                        "edgecolor auto",
                        color,
                        auto,
                    )),
                ),
            ];
            items.push(MenuItem::new(
                t("边底色", "edge background"),
                MenuAction::Sub(self.color_items("edgebg", "none", "edgebg none", bg, bg_auto)),
            ));
            if !self.doc.groups.is_empty() {
                items.push(MenuItem::new(
                    t("分组", "group"),
                    MenuAction::Sub(
                        self.doc
                            .groups
                            .iter()
                            .map(|g| {
                                MenuItem::new(
                                    format!("{} {}", g.id, g.label),
                                    MenuAction::Cmd(format!("group {}", g.id)),
                                )
                                .swatch(g.color)
                                .checked(group.as_deref() == Some(g.id.as_str()))
                            })
                            .collect(),
                    ),
                ));
            }
            if self.doc.layers.len() > 1 {
                items.push(MenuItem::new(
                    t("图层", "layer"),
                    MenuAction::Sub(self.layer_items("edge")),
                ));
            }
            items.push(cmd(t("清掉折点", "clear waypoints"), "via off"));
            items.push(cmd(t("删除这条边", "delete this edge"), "del"));
            return items;
        }
        if pick.box_at().is_some() {
            let (boxes, _) = self.menu_scope();
            let style = Self::common(boxes.iter().map(|i| self.doc.boxes[*i].style));
            let fill_raw = Self::common(boxes.iter().map(|i| self.doc.boxes[*i].fill));
            let border_raw = Self::common(boxes.iter().map(|i| self.doc.boxes[*i].color));
            let text_raw = Self::common(
                boxes
                    .iter()
                    .flat_map(|i| self.doc.text(*i).map(|t| t.color)),
            );
            let (fill, no_fill) = (fill_raw.flatten(), fill_raw == Some(None));
            let (border, auto_border) = (border_raw.flatten(), border_raw == Some(None));
            let (text, auto_text) = (text_raw.flatten(), text_raw == Some(None));
            let align = Self::common(
                boxes
                    .iter()
                    .flat_map(|i| self.doc.text(*i).map(|t| t.align)),
            );
            let text_bg_raw =
                Self::common(boxes.iter().flat_map(|i| self.doc.text(*i).map(|t| t.bg)));
            let (text_bg, text_bg_auto) = (text_bg_raw.flatten(), text_bg_raw == Some(None));
            let mut items = vec![
                MenuItem::new(
                    t("框型", "box style"),
                    MenuAction::Sub(vec![
                        MenuItem::new("point", MenuAction::Cmd("box point".into()))
                            .checked(style == Some(BoxStyle::Point)),
                        MenuItem::new("none", MenuAction::Cmd("box none".into()))
                            .checked(style == Some(BoxStyle::None)),
                        MenuItem::new("single", MenuAction::Cmd("box single".into()))
                            .checked(style == Some(BoxStyle::Single)),
                        MenuItem::new("rounded", MenuAction::Cmd("box rounded".into()))
                            .checked(style == Some(BoxStyle::Rounded)),
                        MenuItem::new("double", MenuAction::Cmd("box double".into()))
                            .checked(style == Some(BoxStyle::Double)),
                        MenuItem::new("heavy", MenuAction::Cmd("box heavy".into()))
                            .checked(style == Some(BoxStyle::Heavy)),
                    ]),
                ),
                MenuItem::new(
                    t("边框色", "border colour"),
                    MenuAction::Sub(self.color_items(
                        "nodecolor",
                        "auto",
                        "nodecolor auto",
                        border,
                        auto_border,
                    )),
                ),
                MenuItem::new(
                    t("填充", "fill"),
                    MenuAction::Sub(self.color_items("fill", "none", "fill none", fill, no_fill)),
                ),
                MenuItem::new(
                    t("文字色", "text colour"),
                    MenuAction::Sub(self.color_items(
                        "textcolor",
                        "auto",
                        "textcolor auto",
                        text,
                        auto_text,
                    )),
                ),
                MenuItem::new(
                    t("文字底色", "text background"),
                    MenuAction::Sub(self.color_items(
                        "textbg",
                        "none",
                        "textbg none",
                        text_bg,
                        text_bg_auto,
                    )),
                ),
                MenuItem::new(
                    t("对齐", "align"),
                    MenuAction::Sub(vec![
                        MenuItem::new("left", MenuAction::Cmd("textalign left".into()))
                            .checked(align == Some(Align::Left)),
                        MenuItem::new("center", MenuAction::Cmd("textalign center".into()))
                            .checked(align == Some(Align::Center)),
                        MenuItem::new("right", MenuAction::Cmd("textalign right".into()))
                            .checked(align == Some(Align::Right)),
                    ]),
                ),
            ];
            if self.doc.layers.len() > 1 {
                items.push(MenuItem::new(
                    t("图层", "layer"),
                    MenuAction::Sub(self.layer_items("box")),
                ));
            }
            items.push(cmd(t("删除", "delete"), "del"));
            return items;
        }
        vec![
            MenuItem::new(t("新建框…", "new box…"), MenuAction::Cmd(String::new())),
            MenuItem::new(t("居中", "centre"), MenuAction::Cmd(String::new())),
        ]
    }

    /// 颜色菜单(只一层):框给边框色,边给边颜色
    fn color_menu_for(&self) -> Vec<MenuItem> {
        let pick = self.view.pick;
        if pick.edge().is_some() {
            let (_, edges) = self.menu_scope();
            let cur = Self::common(edges.iter().map(|e| self.doc.edges[*e].color));
            let auto = cur == Some(None);
            return self.color_items("edgecolor", "auto", "edgecolor auto", cur.flatten(), auto);
        }
        if pick.box_at().is_some() {
            let (boxes, _) = self.menu_scope();
            let cur = Self::common(boxes.iter().map(|i| self.doc.boxes[*i].color));
            let auto = cur == Some(None);
            return self.color_items("nodecolor", "auto", "nodecolor auto", cur.flatten(), auto);
        }
        Vec::new()
    }

    /// s:在光标处打开完整样式菜单(不动光标)
    fn open_style_menu(&mut self) {
        self.open_menu_here(false);
    }

    /// c:直接打开颜色子菜单
    fn open_color_menu(&mut self) {
        self.open_menu_here(true);
    }

    /// 键盘打开菜单:位置贴着光标格右下,快照与右键那条路完全一样
    fn open_menu_here(&mut self, color_only: bool) {
        // 光标下什么都没有:菜单没有可作用的元素
        if self.view.link.is_none() && self.view.pick == render::Pick::None {
            let note = self.view.lang.pick_owned(
                "光标下没有可改样式的元素".to_string(),
                "nothing under the cursor to restyle".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        let items = if color_only {
            self.color_menu_for()
        } else {
            self.menu_for()
        };
        if items.is_empty() {
            let note = self.view.lang.pick_owned(
                "光标下没有可改样式的元素".to_string(),
                "nothing under the cursor to restyle".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        let p = render::panels(&self.doc, self.area, &self.view);
        let (cx, cy) = p
            .proj
            .cell((self.view.cursor.0 as f64, self.view.cursor.1 as f64));
        let at = ((cx.max(0) as u16).saturating_add(2), cy.max(0) as u16);
        let kind = self.menu_kind(color_only);
        let (levels, parents, parents_sel, sel) = self.menu_from_memory(items, &kind);
        self.view.menu = Some(Menu {
            at,
            levels,
            sel,
            parents_sel,
            kind,
            snapshot: self.doc.clone(),
            snapshot_cursor: (self.view.cursor, self.view.pick),
            parents,
            title: if color_only {
                Some(if self.view.pick.edge().is_some() {
                    self.view.lang.pick("颜色", "colour").to_string()
                } else {
                    self.view.lang.pick("边框色", "border colour").to_string()
                })
            } else {
                None
            },
        });
        self.menu_preview();
    }

    /// 右键:把光标挪到点击格,弹菜单
    fn open_menu(&mut self, col: u16, row: u16) {
        let p = render::panels(&self.doc, self.area, &self.view);
        let inside = col >= p.map.x
            && col < p.map.x + p.map.width
            && row >= p.map.y
            && row < p.map.y + p.map.height;
        // 连边模式弹菜单不动光标:目标就是当前那个,别被右键位置带跑
        if inside && self.view.link.is_none() {
            self.view.cursor = p.proj.canvas(col as i32, row as i32);
            self.view.cand = 0;
            self.refresh_pick();
        }
        let kind = self.menu_kind(false);
        let (levels, parents, parents_sel, sel) = self.menu_from_memory(self.menu_for(), &kind);
        self.view.menu = Some(Menu {
            at: (col, row),
            levels,
            sel,
            parents_sel,
            kind,
            snapshot: self.doc.clone(),
            snapshot_cursor: (self.view.cursor, self.view.pick),
            title: None,
            parents,
        });
        // 一进来就把第一项套上:不用点就能看到效果
        self.menu_preview();
    }

    /// 把当前选中项"预演"到文档上:先回到弹菜单时的样子,再套用这一项
    fn menu_preview(&mut self) {
        let Some(menu) = self.view.menu.clone() else {
            return;
        };
        let Some(item) = menu.level().get(menu.sel).cloned() else {
            return;
        };
        self.doc = menu.snapshot.clone();
        self.view.cursor = menu.snapshot_cursor.0;
        self.view.pick = menu.snapshot_cursor.1;
        if let MenuAction::Cmd(line) = &item.action {
            if !line.is_empty() {
                self.previewing = true;
                self.cmd_apply(line);
                self.previewing = false;
            }
        }
        // Link 那两个按钮不动文档,没有预览
        let path = {
            let mut p = menu.parents.clone();
            if let Some(t) = &menu.title {
                p.insert(0, t.clone());
            }
            p.push(item.title.clone());
            p.join(" › ")
        };
        let note = if matches!(item.action, MenuAction::Sub(_)) {
            self.view.lang.pick_owned(
                format!("{path}   l 进入 / h 返回 / Enter 应用 / esc 取消"),
                format!("{path}   l open / h back / Enter apply / esc cancel"),
            )
        } else {
            self.view.lang.pick_owned(
                format!("{path}   (预览;Enter 应用 / h 返回 / esc 取消)"),
                format!("{path}   (preview; Enter applies / h back / esc cancels)"),
            )
        };
        self.view.note = Some(note);
    }

    /// 关掉菜单:文档回到弹菜单之前
    fn menu_cancel(&mut self) {
        if let Some(menu) = self.view.menu.take() {
            self.doc = menu.snapshot;
            self.view.cursor = menu.snapshot_cursor.0;
            self.view.pick = menu.snapshot_cursor.1;
            self.refresh_pick();
            self.view.note = None;
        }
    }

    /// 菜单上执行一项:子菜单就进一层,动词就执行
    fn menu_choose(&mut self) {
        let Some(menu) = self.view.menu.clone() else {
            return;
        };
        let Some(item) = menu.level().get(menu.sel).cloned() else {
            self.view.menu = None;
            return;
        };
        let snapshot = menu.snapshot.clone();
        let snapshot_cursor = menu.snapshot_cursor;
        match item.action {
            MenuAction::Sub(items) => {
                if let Some(m) = self.view.menu.as_mut() {
                    m.levels.push(items);
                    m.parents.push(item.title.clone());
                    m.parents_sel.push(m.sel);
                    m.sel = 0;
                }
                self.menu_preview();
            }
            MenuAction::Cmd(line) if line.is_empty() => {
                // 空命令是"要输入"的那几项
                self.view.menu = None;
                self.doc = snapshot;
                self.view.cursor = snapshot_cursor.0;
                self.view.pick = snapshot_cursor.1;
                self.refresh_pick();
                match item.title.as_str() {
                    t if t.starts_with("新建框") => {
                        self.open_prompt(Prompt::NewBox, String::new());
                    }
                    t if t.starts_with("改文字") => {
                        if let Some(i) = self.cursor_box() {
                            let text = self.doc.text(i).map(|t| t.text.clone()).unwrap_or_default();
                            self.open_prompt(Prompt::Text(vec![i]), text);
                        }
                    }
                    _ => self.center_cursor(),
                }
            }
            MenuAction::Link(ok) => {
                self.view.menu = None;
                self.doc = snapshot;
                self.view.cursor = snapshot_cursor.0;
                self.view.pick = snapshot_cursor.1;
                self.refresh_pick();
                self.view.pick = snapshot_cursor.1;
                if ok {
                    self.link_commit();
                } else {
                    self.cancel_link();
                }
            }
            MenuAction::Cmd(line) => {
                // 记住这次选的路径:下次开同一个菜单,直接进到这一层、停在这一项
                let mut path: Vec<(String, usize)> = menu
                    .parents
                    .iter()
                    .cloned()
                    .zip(menu.parents_sel.iter().copied())
                    .collect();
                path.push((item.title.clone(), menu.sel));
                self.menu_memory.insert(menu.kind.clone(), path);
                // 回到弹菜单之前,再正常执行一次:预览不算数,这一步才进撤销
                self.view.menu = None;
                self.doc = snapshot;
                self.view.cursor = snapshot_cursor.0;
                self.view.pick = snapshot_cursor.1;
                self.refresh_pick();
                // 菜单是在这个元素上开的:框和边同格时 refresh 会挑框,这里把类型保住
                self.view.pick = snapshot_cursor.1;
                self.cmd_apply(&line);
            }
        }
    }

    /// 菜单打开时的按键
    fn handle_menu(&mut self, code: KeyCode) {
        let Some(menu) = self.view.menu.as_mut() else {
            return;
        };
        let n = menu.level().len();
        match code {
            // h / Esc / ←:回上一层菜单,顶层就关掉
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => {
                if menu.levels.len() > 1 {
                    menu.levels.pop();
                    menu.parents.pop();
                    menu.sel = menu.parents_sel.pop().unwrap_or(0);
                    // 回到上一层:把预览收回去,并按这一层的当前项重新预演
                    self.menu_preview();
                } else {
                    self.menu_cancel();
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if n > 0 {
                    menu.sel = (menu.sel + n - 1) % n;
                }
                self.menu_preview();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if n > 0 {
                    menu.sel = (menu.sel + 1) % n;
                }
                self.menu_preview();
            }
            // Enter:应用当前项(子菜单就进去,叶子就执行)
            KeyCode::Enter => self.menu_choose(),
            // l / →:只往下钻一层;叶子项不执行(要执行按 Enter)
            KeyCode::Right | KeyCode::Char('l') => self.menu_enter_level(),
            KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
                let i = c as usize - '1' as usize;
                if i < n {
                    menu.sel = i;
                    self.menu_choose();
                }
            }
            _ => {}
        }
    }

    /// 菜单里按 l / →:当前项是子菜单才进去,叶子项什么都不做
    fn menu_enter_level(&mut self) {
        let Some(menu) = self.view.menu.clone() else {
            return;
        };
        let Some(item) = menu.level().get(menu.sel) else {
            return;
        };
        if matches!(item.action, MenuAction::Sub(_)) {
            self.menu_choose();
        }
    }

    /// 改文档之前压一份撤销快照
    pub fn begin_edit(&mut self, label: &str) {
        if self.previewing {
            return; // 预览不压撤销
        }
        // 动手的地方也算一个跳转点(和搜索、鼠标点一样):插字、建框、改样式之后
        // 走开了还能用 Ctrl-o 回来
        self.note_here();
        self.undo.begin(label, &self.doc);
    }

    /// 这一步没改动就丢掉快照(cmd_apply / 顶部按键路径结束时调)
    pub fn end_edit(&mut self) {
        if self.previewing {
            return;
        }
        if self.undo.prune(&self.doc) {
            self.rev += 1;
        }
    }

    /// u:撤销
    fn undo_step(&mut self) {
        let link = self.link_ids();
        match self.undo.undo(&mut self.doc) {
            Some(label) => {
                self.rev += 1;
                self.after_history(&format!("撤销:{label}"));
                self.rebind_link(link);
            }
            None => {
                let note = self
                    .view
                    .lang
                    .pick_owned("没有可撤销的".to_string(), "nothing to undo".to_string());
                self.view.note = Some(note);
            }
        }
    }

    /// Ctrl-r:重做
    fn redo_step(&mut self) {
        let link = self.link_ids();
        match self.undo.redo(&mut self.doc) {
            Some(label) => {
                self.rev += 1;
                self.after_history(&format!("重做:{label}"));
                self.rebind_link(link);
            }
            None => {
                let note = self
                    .view
                    .lang
                    .pick_owned("没有可重做的".to_string(), "nothing to redo".to_string());
                self.view.note = Some(note);
            }
        }
    }

    /// 撤销/重做之后:光标夹回文档范围,选区与选边都清掉
    fn after_history(&mut self, note: &str) {
        self.view.sel = None;
        self.view.cand = 0;
        if self.doc.boxes.is_empty() {
            self.view.cursor = (0, 0);
        }
        self.refresh_pick();
        self.view.note = Some(note.to_string());
    }

    /// ":" 动词的名字(撤销回显用)
    fn verb_label(cmd: &str) -> &'static str {
        match cmd {
            "box" => "框型",
            "nodecolor" => "边框色",
            "fill" => "填色",
            "textcolor" => "文字色",
            "textbg" => "文字底色",
            "edgebg" => "边底色",
            "textalign" => "对齐",
            "text" => "文字",
            "style" => "线型",
            "edgecolor" => "边色",
            "dir" => "方向",
            "corner" => "拐角",
            "head" => "箭头",
            "via" => "折点",
            "new" => "建框",
            "link" => "连边",
            "del" | "delete" => "删除",
            "tolayer" => "换层",
            _ => "修改",
        }
    }

    /// 重新算"光标格下面是什么";候选边按下标挑
    pub fn refresh_pick(&mut self) {
        self.sync_hit();
        // 邻居高亮跟着光标走:点到/挪到别的节点就换过去亮它.
        // 光标压在注释框上时,亮的是它绑的那条边的"两头"(按边看)
        if self.view.ego.is_some() {
            if let Some(n) = self.doc.box_at_cell(self.view.cursor) {
                let n = match self.doc.edge_of_box(n) {
                    Some(e) => self.doc.edge_ends(e).map(|(a, _)| a).unwrap_or(n),
                    None => n,
                };
                self.view.ego = Some(n);
            }
        }
        let p = render::panels(&self.doc, self.area, &self.view);
        let screen = p
            .proj
            .cell((self.view.cursor.0 as f64, self.view.cursor.1 as f64));
        self.view.pick =
            render::pick_at(&self.doc, &self.hit, self.view.cursor, screen, &self.view);
        if let render::Pick::Edge(_) = self.view.pick {
            // 候选表按屏幕格记;这一格压着好几条边时用 cand 轮
            let cands = self.hit.candidates_at(screen);
            if cands.len() > 1 {
                let e = cands[self.view.cand % cands.len()];
                self.view.pick = render::Pick::Edge(e);
            }
        }
        self.remember_look();
    }

    /// 记住"最近选中/点到的框"与"最近选中的边":新建元素时照抄它们的样式.
    /// 区块选区(拖框/`v`)选中的是一片,不是一个元素,这时不更新模板
    fn remember_look(&mut self) {
        if self.view.sel.is_some() {
            return;
        }
        if let render::Pick::Box(i) = self.view.pick {
            self.box_look = self.look_of_box(i);
        }
        if let render::Pick::Edge(e) = self.view.pick {
            if e < self.doc.edges.len() {
                let ed = &self.doc.edges[e];
                self.edge_look = Some(EdgeLook {
                    group: ed.group.clone(),
                    style: ed.style,
                    corner: ed.corner,
                    color: ed.color,
                    bg: ed.bg,
                    head: ed.head,
                });
            }
        }
    }

    /// 某个框现在的样式快照
    fn look_of_box(&self, i: usize) -> Option<BoxLook> {
        if i >= self.doc.boxes.len() {
            return None;
        }
        let b = &self.doc.boxes[i];
        let t = self.doc.text(i);
        Some(BoxLook {
            style: b.style,
            color: b.color,
            fill: b.fill,
            text_color: t.and_then(|t| t.color),
            text_bg: t.and_then(|t| t.bg),
            align: t.map(|t| t.align).unwrap_or_default(),
        })
    }

    /// 刚用菜单/命令改过样式的那个框:也算"选中",新建框时照抄它
    fn remember_box_look(&mut self, i: usize) {
        if let Some(look) = self.look_of_box(i) {
            self.box_look = Some(look);
        }
    }

    /// 新建框该照抄的样式:光标下这个框(实时取),否则用最近改过/选中的那个框
    fn box_look_now(&self) -> Option<BoxLook> {
        if self.view.sel.is_some() {
            return None;
        }
        self.view
            .pick
            .box_at()
            .and_then(|i| self.look_of_box(i))
            .or(self.box_look)
    }

    /// 新建边时该照抄的样式:最近选中的那条边;区块选区时为空(那不算选中一条边)
    fn edge_look_now(&self) -> Option<EdgeLook> {
        if self.view.sel.is_some() {
            return None;
        }
        self.view
            .pick
            .edge()
            .filter(|&e| e < self.doc.edges.len())
            .map(|e| {
                let ed = &self.doc.edges[e];
                EdgeLook {
                    group: ed.group.clone(),
                    style: ed.style,
                    corner: ed.corner,
                    color: ed.color,
                    bg: ed.bg,
                    head: ed.head,
                }
            })
            .or_else(|| self.edge_look.clone())
    }

    /// 把照抄来的样式落到新建的边上(分组在 add_edge 时已经定了)
    fn apply_edge_look(&mut self, e: usize, look: &EdgeLook) {
        if e >= self.doc.edges.len() {
            return;
        }
        let ed = &mut self.doc.edges[e];
        ed.style = look.style;
        ed.corner = look.corner;
        ed.color = look.color;
        ed.bg = look.bg;
        ed.head = look.head;
    }

    /// 命中表只对生成它的那一版文档有效(里面存的是数组下标);
    /// 文档改过就整张作废,别拿旧下标去选边
    fn sync_hit(&mut self) {
        if self.hit.rev != self.doc.rev() {
            self.hit = render::FrameHit::default();
            self.view.cand = 0;
        }
    }

    /// 当前这一版可用的命中表(对不上就当没有)
    fn usable_hit(&self) -> &render::FrameHit {
        static EMPTY: std::sync::OnceLock<render::FrameHit> = std::sync::OnceLock::new();
        if self.hit.rev == self.doc.rev() {
            &self.hit
        } else {
            EMPTY.get_or_init(render::FrameHit::default)
        }
    }

    /// 有没有动画在跑(没有就可以少画几帧)
    pub fn busy(&self) -> bool {
        // 有选中元素就要一直推进呼吸动画;平移缓动同理
        self.view.target.is_some()
            || self.view.pan.is_some()
            || self.view.pick != render::Pick::None
            || self.view.sel.is_some()
    }

    /// 按时间推进动画(平移缓动,光标呼吸).返回"这一帧和上一帧可能不一样吗":
    /// 呼吸量化成有限档,缓动也要真的动了才需要重画 —— 否则每秒几十次全屏 diff
    /// 全是在往终端写同样的字节.
    pub fn tick(&mut self) -> bool {
        let before = self.view.pulse;
        let phase = self.view.started.elapsed().as_secs_f64() / BEAT;
        let q = (pulse_at(phase) * PULSE_STEPS).round() / PULSE_STEPS;
        self.view.pulse = q;
        let (center, zoom) = (self.view.center, self.view.zoom);
        let cell = render::panels(&self.doc, self.area, &self.view).proj.scale;
        animate(&mut self.view, &self.doc, cell);
        (q - before).abs() > f64::EPSILON || self.view.center != center || self.view.zoom != zoom
    }

    /// 画一帧(顺手把画面区域记下来,鼠标换算要用)
    pub fn draw(&mut self, buf: &mut Buffer, area: Rect) {
        self.area = area;
        // 状态栏要显示 [n/m] 与脏标记
        self.view.tab = (self.tabs.len() > 1).then_some((self.tab_at + 1, self.tabs.len()));
        self.view.tab_name = self.tab_name();
        self.view.tab_dirty = self.dirty();
        // 上行的文件路径与工作目录:`~` 缩写
        self.view.file_display = self
            .source
            .as_deref()
            .map(|p| crate::paths::tildify(&crate::paths::absolute_display(p)));
        self.view.cwd = self.cwd.clone();
        // 节点被删掉/被撤销掉之后,高亮范围不能还指着它
        if self.view.ego.is_some_and(|n| n >= self.doc.boxes.len()) {
            self.view.ego = None;
        }
        // 选区里已经什么都没有了(比如把选中的框删了):自动取消,
        // 效果和"点选区外面"一样,不给编辑留一块空选区
        if self.view.sel.is_some_and(|s| s.confirmed) {
            let p = render::panels(&self.doc, area, &self.view);
            let (boxes, edges) = render::selected_all(&self.doc, &p, &self.view, &self.hit);
            if boxes.is_empty() && edges.is_empty() {
                self.view.sel = None;
            }
        }
        // 打开文件时光标可能不在视口里:首帧把它居中一次
        if self.first_frame {
            self.first_frame = false;
            if self.cursor_cell().is_none() && !self.doc.boxes.is_empty() {
                self.view.center = Some((self.view.cursor.0 as f64, self.view.cursor.1 as f64));
                self.view.target = None;
            }
        }
        // 上一帧的命中表借出去给这一帧用;挪走再放回,省掉每帧一次深拷贝
        let prev = std::mem::take(&mut self.hit);
        let (_, hit) = render::render_hit(buf, area, &self.doc, &self.view, &prev);
        // 状态栏要提示"有名字没画下":用上一帧的统计,一帧延迟看不出来
        self.view.labels_hidden = hit.labels_skipped.len();
        self.hit = hit;
        if self.view.help {
            render::render_help(buf, area, render::help_keys(self.view.lang), self.view.lang);
        }
        if self.view.file_pick.is_some() {
            render::render_pick(buf, area, &self.view);
        }
    }

    /// 一个事件
    pub fn handle(&mut self, ev: Event) -> Flow {
        if self.quit {
            return Flow::Quit;
        }
        if self.view.edit.is_some() {
            match &ev {
                // 编辑时点一下鼠标:先提交,再按平常处理这次点击
                Event::Mouse(_) => self.commit_box_edit(false),
                Event::Key(KeyEvent {
                    code, modifiers, ..
                }) => {
                    self.handle_edit_key(*code, *modifiers);
                    return Flow::Continue;
                }
                _ => return Flow::Continue,
            }
        }
        if let Event::Mouse(me) = &ev {
            if !self.mouse {
                return Flow::Continue;
            }
            self.sync_hit();
            self.handle_mouse(*me);
            return Flow::Continue;
        }
        self.sync_hit();
        let Event::Key(KeyEvent {
            code, modifiers, ..
        }) = ev
        else {
            return Flow::Continue;
        };
        if modifiers.contains(KeyModifiers::CONTROL) && matches!(code, KeyCode::Char('c')) {
            self.note_quit_hint();
            return Flow::Continue;
        }
        // 打开文件的选择浮层:它优先吃掉所有按键
        if self.view.file_pick.is_some() {
            self.handle_pick(code);
            return Flow::Continue;
        }
        // 速查浮层:任意键关掉
        if self.view.help {
            self.view.help = false;
            return Flow::Continue;
        }
        // 选"沿线走一站"走哪条分组:数字直选,j k 挪,回车走,esc 取消
        // 右键菜单:上下选、回车进/执行、esc 退一层
        if self.view.menu.is_some() {
            self.handle_menu(code);
            return Flow::Continue;
        }
        // 底行单行输入:所有按键都归它
        if self.view.prompt.is_some() {
            self.handle_prompt(code, modifiers);
            return Flow::Continue;
        }
        // ":" 命令行:所有按键都归输入行
        if self.view.cmd.is_some() {
            self.handle_cmd(code, modifiers);
            // :q 在这一下里敲完的,立刻交出去,不用等下一个事件
            return if self.quit {
                Flow::Quit
            } else {
                Flow::Continue
            };
        }
        // 搜索态:所有按键都归输入行
        if self.view.query.is_some() {
            self.handle_search(code, modifiers);
            return Flow::Continue;
        }
        // 连边模式:Esc 取消、Enter 建边、再按 e 只提示
        if self.view.link.is_some()
            && !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            match code {
                KeyCode::Esc => {
                    self.cancel_link();
                    return Flow::Continue;
                }
                KeyCode::Enter => {
                    self.link_commit();
                    return Flow::Continue;
                }
                KeyCode::Char('e') => {
                    let note = self.view.lang.pick_owned(
                        "连边中:Enter 创建,Esc 取消".to_string(),
                        "linking: Enter to create, Esc to cancel".to_string(),
                    );
                    self.view.note = Some(note);
                    return Flow::Continue;
                }
                _ => {}
            }
        }
        // 非字符键(方向、Esc 等)把没走完的序列作废
        if !matches!(code, KeyCode::Char(_)) {
            self.clear_seq();
        }
        // 数字前缀(5l)、坐标跳转(100,100G)、gg:先吃掉,序列挂在右下角
        if let KeyCode::Char(c) = code {
            if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) && self.seq_key(c) {
                return Flow::Continue;
            }
        }
        // 剩下的都归键位表:没绑的键(含没认领的 Ctrl/Alt 组合)什么都不做,只清计数
        let Some(action) = self.keys.get(code, modifiers) else {
            self.pending = None;
            return Flow::Continue;
        };
        let dir = |a: crate::keys::Action| -> (i32, i32) {
            use crate::keys::Action as K;
            match a {
                K::MoveLeft | K::NudgeLeft => (-1, 0),
                K::MoveRight | K::NudgeRight => (1, 0),
                K::MoveUp | K::NudgeUp => (0, -1),
                _ => (0, 1),
            }
        };
        use crate::keys::Action;
        match action {
            // esc:先退邻居高亮,再退选区
            Action::Escape => {
                if self.view.ego.is_some() {
                    self.view.ego = None;
                    let note = self
                        .view
                        .lang
                        .pick_owned("退出邻居高亮".to_string(), "neighbour view off".to_string());
                    self.view.note = Some(note);
                } else if self.view.sel.is_some() {
                    self.view.sel = None;
                    let note = self
                        .view
                        .lang
                        .pick_owned("取消选择".to_string(), "selection off".to_string());
                    self.view.note = Some(note);
                }
            }
            Action::Help => self.view.help = true,
            // z / 空格:把光标那个框挪到画面正中
            Action::Center => self.center_cursor(),
            // f:把聚焦的分组(没聚焦就是光标所在的分组)铺满画面
            Action::Fit => {
                self.fit_focus();
                self.note_zoom();
            }
            // F:把整幅图铺满画面(不设中心 = 回到整幅图的中心)
            Action::FitAll => {
                let zoom = render::zoom_for_fit(
                    &self.doc,
                    self.area.width,
                    self.area.height,
                    self.view.notes,
                );
                self.view.zoom = clamp_zoom(zoom);
                self.view.center = None;
                self.view.target = None;
                self.view.pan = None;
                self.note_zoom();
            }
            Action::Link => self.begin_link(),
            Action::BlockSelect => self.start_rect_select(),
            Action::CycleLayer => self.cycle_layer(),
            Action::EgoToggle => self.toggle_ego(),
            Action::ToggleNotes => self.cmd_notes(None),
            Action::CycleEdge => self.cycle_edge(),
            // 半屏跳:远距离旅行用
            Action::LeapDown => {
                let next = self.leap((0, 1));
                self.jump_to(next);
            }
            Action::LeapUp => {
                let next = self.leap((0, -1));
                self.jump_to(next);
            }
            Action::JumpBack => {
                self.jump_history(true);
            }
            Action::JumpForward => {
                self.jump_history(false);
            }
            // 移动光标:带计数就是走 N 格
            Action::MoveLeft | Action::MoveRight | Action::MoveUp | Action::MoveDown => {
                let steps = self.pending.take().unwrap_or(1);
                let d = dir(action);
                self.move_cursor(d, steps);
            }
            // 挪框:先把选中(或光标)的框动起来
            Action::NudgeLeft | Action::NudgeRight | Action::NudgeUp | Action::NudgeDown => {
                let steps = self.pending.take().unwrap_or(1).max(1) as i32;
                let d = dir(action);
                self.move_boxes(d, steps);
            }
            Action::ZoomIn => {
                let at = self.cursor_screen_cell();
                self.zoom_at(self.view.zoom * ZOOM_STEP, at);
            }
            Action::ZoomOut => {
                let at = self.cursor_screen_cell();
                self.zoom_at(self.view.zoom / ZOOM_STEP, at);
            }
            Action::ZoomReset => {
                // 只把缩放拨回 1:1,不动画面:要回原点用 gg / {x},{y}G
                self.view.zoom = 1.0;
                self.view.target = None;
                self.view.pan = None;
                self.note_zoom();
            }
            Action::ToggleLabels => {
                self.view.labels = !self.view.labels;
                let on = self.view.labels;
                let note = if on {
                    self.view
                        .lang
                        .pick_owned("标节点名:开".to_string(), "labels: on".to_string())
                } else {
                    self.view
                        .lang
                        .pick_owned("标节点名:关".to_string(), "labels: off".to_string())
                };
                self.view.note = Some(note);
            }
            Action::Search => {
                self.search_from = self.view.cursor;
                self.search_focus = self.view.focus.clone();
                self.view.query = Some(String::new());
                self.view.query_pos = 0;
                self.view.hits.clear();
            }
            // n / N:下一个 / 上一个命中的框
            Action::NextHit => {
                if !self.view.hits.is_empty() {
                    self.view.hit_at = (self.view.hit_at + 1) % self.view.hits.len();
                    let next = self.view.hits[self.view.hit_at];
                    self.jump_to(next);
                }
            }
            Action::PrevHit => {
                if !self.view.hits.is_empty() {
                    self.view.hit_at =
                        (self.view.hit_at + self.view.hits.len() - 1) % self.view.hits.len();
                    let next = self.view.hits[self.view.hit_at];
                    self.jump_to(next);
                }
            }
            // r:四套布局轮着换
            Action::CycleLayout => self.cycle_layout(),
            Action::Undo => self.undo_step(),
            Action::Redo => self.redo_step(),
            // d:带边剪切;y:只复制不删;p:粘贴
            Action::Cut => self.cmd_del(true),
            Action::Yank => self.cmd_yank(),
            Action::SplitBox => self.cmd_split_box(),
            Action::MergeBoxes => self.cmd_merge_boxes(),
            Action::EditOrNew => self.cmd_edit_inline(),
            // w / b:朝前 / 朝后射一条线,光标落到第一个撞到的框或边
            Action::JumpNext => self.jump_ray(false),
            Action::JumpPrev => self.jump_ray(true),
            Action::Paste => self.cmd_paste(),
            // Enter:光标下有链接就打开;否则确认选区(没有选区时什么都不做)
            Action::OpenLink => self.open_link(),
            Action::ConfirmSelection => self.confirm_selection(),
            // Ctrl+G:直接拿外部编辑器改光标框的文字,退出即生效(不经过底行)
            Action::EditTextExternal => self.edit_text_external(),
            // c:颜色菜单(框是边框色,边是边颜色);连边走 e 或 :link
            Action::ColorMenu => self.open_color_menu(),
            // s:完整样式菜单
            Action::StyleMenu => self.open_style_menu(),
            Action::Command => {
                self.view.cmd = Some(String::new());
                self.view.cmd_pos = 0;
                self.view.note = None;
            }
            // 退出只认 :q(和 vim 一样):q 提示一句
            Action::QuitHint => self.note_quit_hint(),
            Action::Reload => self.reload_step(),
            Action::TabNew => {
                self.new_tab(None);
            }
            Action::TabNext => {
                let n = self.tabs.len();
                self.switch_tab((self.tab_at + 1) % n);
            }
            Action::TabPrev => {
                let n = self.tabs.len();
                self.switch_tab((self.tab_at + n - 1) % n);
            }
            Action::TabClose => {
                self.close_tab(false);
            }
        }
        Flow::Continue
    }

    /// 修改图之前调用:不在 1:1 就先切到 1:1 并把视图居中到光标框.
    /// 返回 true 表示本来就已经 1:1(可以继续做这次修改);false 表示刚切过来,
    /// 本次修改不执行 -- 放大/缩小的状态下只允许查看.
    fn require_edit_scale(&mut self) {
        if at_one_scale(&self.view) {
            return;
        }
        // 以光标为锚回到 1:1:光标在屏幕上的位置尽量不变(不再跳回"居中到框").
        // 光标本来就在视口外才退回居中,否则编辑目标又看不见了
        let p = render::panels(&self.doc, self.area, &self.view);
        let at = self.cursor_screen_cell();
        let inside = at.0 >= p.map.x as i32
            && at.0 < (p.map.x + p.map.width) as i32
            && at.1 >= p.map.y as i32
            && at.1 < (p.map.y + p.map.height) as i32;
        if inside {
            self.zoom_at(1.0, at);
            // zoom_at 会写回显,这里换成编辑相关的说明
            let note = self.view.lang.pick_owned(
                "编辑要在 1:1:已切回".to_string(),
                "editing needs 1:1: switched back".to_string(),
            );
            self.view.note = Some(note);
        } else {
            self.view.zoom = 1.0;
            self.view.target = None;
            self.view.pan = None;
            if let Some(i) = self.cursor_box() {
                self.view.center = Some(self.doc.center(i));
            }
            let note = self.view.lang.pick_owned(
                "编辑要在 1:1:已切回并居中".to_string(),
                "editing needs 1:1: switched back and centered".to_string(),
            );
            self.view.note = Some(note);
        }
    }

    /// ":" 命令行:layout <名>,side <倍数>,cycle <倍数|auto>,lang <zh|en>,
    /// layer / tolayer / export / export-canvas / w / save / e,
    /// 以及编辑动词 box / nodecolor / fill / textcolor / textalign / style / edgecolor / dir / corner / via / text
    pub fn cmd_apply(&mut self, line: &str) {
        let mut it = line.split_whitespace();
        let (cmd, arg) = (it.next().unwrap_or(""), it.next());
        // 改文档的动词先压一份撤销快照
        // 边动词(style/edgecolor/dir/corner/via)由 edit_edges 自己压一步,
        // 这里再包一层就会变成两步撤销
        let edits = matches!(
            cmd,
            "box" | "nodecolor" | "fill" | "textcolor" | "textalign" | "text" | "tolayer"
        );
        if edits && self.blocked_by_selection() {
            return;
        }
        if edits {
            self.begin_edit(Self::verb_label(cmd));
        }
        // 用菜单/命令改过样式的框也算"选中":新建框时照抄它的样式
        let styled_box = matches!(
            cmd,
            "box" | "nodecolor" | "fill" | "textcolor" | "textalign" | "textbg"
        );
        // 改元素的动词先看光标/选区有没有东西
        // (边的动词自己有"没选中边"的提示,这里只管框)
        if matches!(
            cmd,
            "box" | "nodecolor" | "fill" | "textcolor" | "textalign" | "text" | "tolayer"
        ) && self.targets().is_empty()
        {
            let note = self.view.lang.pick_owned(
                "光标下没有可编辑项".to_string(),
                "nothing under the cursor".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        // 命令名之后的原文: text 与 via 要保留整行(空格也是内容)
        let rest = cmd_rest(line, cmd);
        match (cmd, arg) {
            ("q", _) | ("quit", _) => self.quit = true,
            ("undo", _) => self.undo_step(),
            ("redo", _) => self.redo_step(),
            ("history", _) => {
                let (p, f) = self.undo.counts();
                self.view.note = Some(self.view.lang.pick_owned(
                    format!("可撤销 {p} 步,可重做 {f} 步"),
                    format!("{p} to undo, {f} to redo"),
                ));
            }
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
            // ---- 图层:看/动哪一层,以及把选中的东西归到某层 ----
            // ---- 从零建图:新建框 / 连边 / 删除 ----
            ("new", _) => self.cmd_new(rest.trim()),
            ("link", Some(a)) => self.cmd_link(a),
            ("link", None) => {
                let note = self.view.lang.pick_owned(
                    "link 缺参数:要连到哪个框(id 或文字)".to_string(),
                    "link needs a target box (id or label)".to_string(),
                );
                self.view.note = Some(note);
            }
            ("del", _) | ("delete", _) => self.cmd_del(true),
            ("paste", _) | ("put", _) => self.cmd_paste(),
            ("rotate", Some(a)) => self.cmd_rotate(Some(a)),
            ("rotate", None) => self.cmd_rotate(None),
            // 开关是视图状态;title / add / rm 改文档(进撤销栈)
            ("notes", arg) => self.cmd_notes(arg),
            ("relabel", _) => self.cmd_relabel(rest),
            ("layer", Some(a)) => {
                self.require_edit_scale();
                if a.eq_ignore_ascii_case("all") {
                    self.view.layer = None;
                } else {
                    self.view.layer = Some(a.to_string());
                }
                self.note_layer();
            }
            ("layer", None) => self.note_layer(),
            ("tolayer", Some(a)) => self.move_to_layer(a),
            ("tolayer", None) => {
                let note = self.view.lang.pick_owned(
                    "tolayer 缺参数:要移到哪一层".to_string(),
                    "tolayer needs a layer name".to_string(),
                );
                self.view.note = Some(note);
            }
            ("export", _) => {
                let path = rest.trim();
                if path.is_empty() {
                    let note = self.view.lang.pick_owned(
                        "export 缺参数:要写到哪个路径(.svg/.png/.dot/.canvas)".to_string(),
                        "export needs a path (.svg/.png/.dot/.canvas)".to_string(),
                    );
                    self.view.note = Some(note);
                } else if path.to_ascii_lowercase().ends_with(".dot") {
                    self.export_dot(path);
                } else if path.to_ascii_lowercase().ends_with(".canvas") {
                    self.export_canvas(path);
                } else {
                    self.export_svg(path);
                }
            }
            ("export-canvas", _) => {
                let path = rest.trim();
                if path.is_empty() {
                    let note = self.view.lang.pick_owned(
                        "export-canvas 缺参数:要写到哪个 .canvas".to_string(),
                        "export-canvas needs a path (.canvas)".to_string(),
                    );
                    self.view.note = Some(note);
                } else {
                    self.export_canvas(path);
                }
            }
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
                    return;
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
            // :w 与 :save 同义:vim 的写法与旧习惯都留着
            ("save", _) | ("w", _) => {
                let text = rest.trim();
                if text.is_empty() {
                    self.save(None);
                } else {
                    self.save(Some(text));
                }
            }
            // :wq / :wq <路径>:存成功才退(存失败就留在原地,别把改动丢了)
            ("wq", _) => {
                let text = rest.trim();
                let ok = if text.is_empty() {
                    self.save(None)
                } else {
                    self.save(Some(text))
                };
                if ok {
                    self.quit = true;
                }
            }
            // 多文档 tab
            ("tabnew", _) => {
                let path = rest.trim();
                if path.is_empty() {
                    self.new_tab(None);
                } else {
                    self.new_tab(Some(path));
                }
            }
            ("tabnext", _) => {
                let n = self.tabs.len();
                self.switch_tab((self.tab_at + 1) % n);
            }
            ("tabprev", _) => {
                let n = self.tabs.len();
                self.switch_tab((self.tab_at + n - 1) % n);
            }
            ("tab", Some(a)) => match a.parse::<usize>() {
                Ok(n) if n >= 1 && n <= self.tabs.len() => self.switch_tab(n - 1),
                _ => {
                    let note = self.view.lang.pick_owned(
                        format!("tab: 1..={} 的数字,不是 {a}", self.tabs.len()),
                        format!("tab: a number 1..={}, not {a}", self.tabs.len()),
                    );
                    self.view.note = Some(note);
                }
            },
            ("tabclose", _) | ("tabclose!", _) => {
                self.close_tab(cmd == "tabclose!");
            }
            ("tabs", _) => {
                let mut parts: Vec<String> = Vec::new();
                for i in 0..self.tabs.len() {
                    let (name, dirty) = if i == self.tab_at {
                        (self.tab_name(), self.dirty())
                    } else {
                        match self.tabs[i].as_ref() {
                            Some(p) => (
                                match &p.source {
                                    Some(path) => std::path::Path::new(path)
                                        .file_name()
                                        .map(|f| f.to_string_lossy().into_owned())
                                        .unwrap_or_else(|| path.clone()),
                                    None => self.view.lang.pick("草稿", "scratch").to_string(),
                                },
                                p.rev != p.src_rev,
                            ),
                            None => (String::new(), false),
                        }
                    };
                    parts.push(format!(
                        "{}{} {}{}",
                        if i == self.tab_at { ">" } else { " " },
                        i + 1,
                        name,
                        if dirty { "*" } else { "" }
                    ));
                }
                let list = parts.join("  ");
                self.view.note = Some(list);
            }
            // 端口:强制某条边从哪一侧进出(auto = 交回自动)
            ("fromside", _) | ("toside", _) => {
                let Some(e) = self.view.pick.edge() else {
                    let note = self.view.lang.pick_owned(
                        "先选中一条边".to_string(),
                        "select an edge first".to_string(),
                    );
                    self.view.note = Some(note);
                    return;
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
                            return;
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
                    return;
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
                        return;
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
                                return;
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
                            return;
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
            // :region ... —— 区域的增删改(collapse 只影响显示,不动数据)
            ("region", _) => self.cmd_region(arg, rest),
            ("e", _) => {
                let path = rest.trim();
                // 当前 tab 有没保存的改动:先停下来问一句(`:e!` 才丢)
                if self.dirty() {
                    let note = self.view.lang.pick_owned(
                        "当前文档有没保存的改动:用 :e! 丢弃,或用 :tabnew 另开一个".to_string(),
                        "unsaved changes: use :e! to discard, or :tabnew to open elsewhere"
                            .to_string(),
                    );
                    self.view.note = Some(note);
                    return;
                }
                if path.is_empty() {
                    self.open_picker();
                } else {
                    self.open_doc(path);
                }
            }
            // :e! 强制从磁盘重读(丢掉本地改动);带路径就是强制打开那个文件
            ("e!", _) => {
                let path = rest.trim();
                if path.is_empty() || self.source.as_deref() == Some(path) {
                    self.reload_now();
                } else {
                    self.open_doc(path);
                }
            }
            ("", None) => self.view.note = None,
            _ => {
                let note = self.view.lang.pick_owned(
                    format!(
                        "未知命令:{line}(有 layout/side/cycle/lang/layer/tolayer/export/export-canvas,box/nodecolor/fill/textcolor/textalign/style/edgecolor/dir/corner/via/text,w/save/wq/e)"
                    ),
                    format!(
                        "unknown command: {line} (try layout/side/cycle/lang/layer/tolayer/export/export-canvas, box/nodecolor/fill/textcolor/textalign/style/edgecolor/dir/corner/via/text, w/save/e)"
                    ),
                );
                self.view.note = Some(note);
            }
        }
        if styled_box {
            if let Some(&i) = self.targets().boxes.first() {
                self.remember_box_look(i);
            }
        }
        if edits {
            self.end_edit();
        }
    }

    /// 区域(cluster)动词.参数是子命令;`rest` 保留整行(标题可以有空格)
    fn cmd_region(&mut self, sub: Option<&str>, rest: &str) {
        let sub = sub.unwrap_or("");
        // 子命令之后的原文:`region label 一段文字` 里标题是"一段文字"
        let tail = {
            let r = rest.trim_start();
            r.strip_prefix(sub).unwrap_or("").trim()
        };
        // 目标:光标框所属的区域,或者显式给的 id
        let by_id = |app: &App, id: &str| app.doc.region_idx(id);
        let pick_region = |app: &App, arg: &str| -> Option<usize> {
            if !arg.is_empty() {
                return by_id(app, arg);
            }
            app.cursor_box().and_then(|b| app.doc.box_region(b))
        };
        // 参与这次操作的框:选区里的,否则光标下的
        let members = |app: &App| -> Vec<String> {
            let boxes = app.targets().boxes;
            boxes
                .iter()
                .filter_map(|&b| app.doc.boxes.get(b).map(|x| x.id.clone()))
                .collect()
        };
        match sub {
            "" => {
                let n = self.doc.regions.len();
                let note = if n == 0 {
                    self.view.lang.pick_owned(
                        "还没有区域:选中几个框再 :region new 标签".to_string(),
                        "no regions yet: select boxes, then :region new <label>".to_string(),
                    )
                } else {
                    let names: Vec<String> = self
                        .doc
                        .regions
                        .iter()
                        .map(|r| format!("{}({})", r.id, r.members.len()))
                        .collect();
                    self.view.lang.pick_owned(
                        format!("{n} 个区域: {}", names.join(" ")),
                        format!("{n} region(s): {}", names.join(" ")),
                    )
                };
                self.view.note = Some(note);
            }
            "new" => {
                let ids = members(self);
                if ids.is_empty() {
                    let note = self.view.lang.pick_owned(
                        "区域要有成员:先选中几个框(或把光标放在框上)".to_string(),
                        "a region needs members: select boxes or put the cursor on one".to_string(),
                    );
                    self.view.note = Some(note);
                    return;
                }
                self.begin_edit("建区域");
                let label = tail;
                let n = self.doc.regions.len() + 1;
                let id = format!("r{n}");
                self.doc.regions.push(crate::document::Region {
                    id: id.clone(),
                    label: label.to_string(),
                    color: None,
                    style: crate::model::BoxStyle::Rounded,
                    layer: String::new(),
                    padding: 1,
                    members: ids.clone(),
                    collapsed: false,
                });
                self.doc.rebuild();
                self.end_edit();
                let note = self.view.lang.pick_owned(
                    format!("区域 {id}:{} 个框", ids.len()),
                    format!("region {id}: {} box(es)", ids.len()),
                );
                self.view.note = Some(note);
            }
            "add" | "rm" => {
                let Some(ri) = pick_region(self, tail) else {
                    let note = self.view.lang.pick_owned(
                        "光标下的框不属于任何区域(:region add <区域 id> 直接指定)".to_string(),
                        "the cursor box is in no region (:region add <region id> to name one)"
                            .to_string(),
                    );
                    self.view.note = Some(note);
                    return;
                };
                let ids = members(self);
                if ids.is_empty() {
                    return;
                }
                self.begin_edit(if sub == "add" {
                    "区域加成员"
                } else {
                    "区域移成员"
                });
                if let Some(r) = self.doc.regions.get_mut(ri) {
                    if sub == "add" {
                        for id in ids {
                            if !r.members.contains(&id) {
                                r.members.push(id);
                            }
                        }
                    } else {
                        r.members.retain(|m| !ids.contains(m));
                    }
                }
                self.doc.rebuild();
                self.end_edit();
                let note = self.view.lang.pick_owned(
                    format!(
                        "区域 {} 现在 {} 个成员",
                        self.doc.regions[ri].id,
                        self.doc.regions[ri].members.len()
                    ),
                    format!(
                        "region {} now has {} member(s)",
                        self.doc.regions[ri].id,
                        self.doc.regions[ri].members.len()
                    ),
                );
                self.view.note = Some(note);
            }
            "del" => {
                let Some(ri) = pick_region(self, tail) else {
                    return;
                };
                self.begin_edit("删区域");
                let id = self.doc.regions[ri].id.clone();
                self.doc.regions.remove(ri);
                self.doc.rebuild();
                self.end_edit();
                let note = self.view.lang.pick_owned(
                    format!("删掉区域 {id}(框都留着)"),
                    format!("removed region {id} (boxes stay)"),
                );
                self.view.note = Some(note);
            }
            "collapse" | "expand" => {
                let Some(ri) = pick_region(self, tail) else {
                    return;
                };
                self.begin_edit(if sub == "collapse" {
                    "折叠区域"
                } else {
                    "展开区域"
                });
                if let Some(r) = self.doc.regions.get_mut(ri) {
                    r.collapsed = sub == "collapse";
                }
                self.doc.rebuild();
                self.end_edit();
                let note = self.view.lang.pick_owned(
                    format!(
                        "区域 {} {}",
                        self.doc.regions[ri].id,
                        if sub == "collapse" {
                            "已折叠"
                        } else {
                            "已展开"
                        }
                    ),
                    format!(
                        "region {} {}",
                        self.doc.regions[ri].id,
                        if sub == "collapse" {
                            "collapsed"
                        } else {
                            "expanded"
                        }
                    ),
                );
                self.view.note = Some(note);
            }
            "label" => {
                let Some(ri) = pick_region(self, "") else {
                    self.view.note = Some(self.view.lang.pick_owned(
                        "光标下的框不属于任何区域".to_string(),
                        "the cursor box is in no region".to_string(),
                    ));
                    return;
                };
                self.begin_edit("区域标题");
                let text = tail.to_string();
                if let Some(r) = self.doc.regions.get_mut(ri) {
                    r.label = text;
                }
                self.doc.rebuild();
                self.end_edit();
            }
            "color" => {
                let Some(ri) = pick_region(self, "") else {
                    return;
                };
                let v = tail;
                let color = if v.is_empty() || v == "auto" || v == "none" {
                    None
                } else {
                    parse_color(v)
                };
                if v != "auto" && v != "none" && color.is_none() {
                    let note = self.view.lang.pick_owned(
                        format!("认不出颜色 {v}(用 auto 清掉)"),
                        format!("unknown colour {v} (use auto to clear)"),
                    );
                    self.view.note = Some(note);
                    return;
                }
                self.begin_edit("区域颜色");
                if let Some(r) = self.doc.regions.get_mut(ri) {
                    r.color = color;
                }
                self.doc.rebuild();
                self.end_edit();
            }
            "style" => {
                let Some(ri) = pick_region(self, "") else {
                    return;
                };
                let Some(st) = self
                    .doc
                    .regions
                    .get(ri)
                    .map(|r| crate::model::BoxStyle::parse(tail).unwrap_or(r.style))
                else {
                    return;
                };
                if !st.is_boxed() {
                    let note = self.view.lang.pick_owned(
                        "区域只能用带边框的样式(single/rounded/double/heavy)".to_string(),
                        "a region needs a boxed style (single/rounded/double/heavy)".to_string(),
                    );
                    self.view.note = Some(note);
                    return;
                }
                self.begin_edit("区域样式");
                if let Some(r) = self.doc.regions.get_mut(ri) {
                    r.style = st;
                }
                self.doc.rebuild();
                self.end_edit();
            }
            other => {
                let note = self.view.lang.pick_owned(
                    format!("region 只认 new/add/rm/del/collapse/expand/label/color/style,不认 {other}"),
                    format!("region takes new/add/rm/del/collapse/expand/label/color/style, not {other}"),
                );
                self.view.note = Some(note);
            }
        }
    }

    /// 拖拽时靠近视口边缘就自动平移:离边 2 格以内,越靠边挪得越多(最多 3 格).
    /// 只在拖拽过程中调用,普通鼠标移动不挪视图
    fn autopan(&mut self, col: u16, row: u16) {
        let p = render::panels(&self.doc, self.area, &self.view);
        if p.map.width == 0 || p.map.height == 0 {
            return;
        }
        let (x0, y0) = (p.map.x as i32, p.map.y as i32);
        let (x1, y1) = (x0 + p.map.width as i32 - 1, y0 + p.map.height as i32 - 1);
        let (c, r) = (col as i32, row as i32);
        let edge_dist = |v: i32, lo: i32, hi: i32| -> i32 {
            if v < lo + 2 {
                (lo + 2 - v).min(3)
            } else if v > hi - 2 {
                (v - (hi - 2)).min(3)
            } else {
                0
            }
        };
        // 正数 = 视图往左/上推(内容往右/下走)
        let dx = edge_dist(c, x0, x1);
        let dy = edge_dist(r, y0, y1);
        if dx == 0 && dy == 0 {
            return;
        }
        let center = self.view.center.unwrap_or_else(|| bbox_center(&self.doc));
        let proj = p.proj;
        self.view.center = Some(render::pan_center(&proj, center, dx, dy));
        self.view.target = None;
        self.view.pan = None;
    }

    /// 缩放变化后用回显位说一句(状态行只剩节点名了)
    fn note_zoom(&mut self) {
        let pct = (self.view.zoom * 100.0).round() as i64;
        let note = self
            .view
            .lang
            .pick_owned(format!("缩放 {pct}%"), format!("zoom {pct}%"));
        self.view.note = Some(note);
    }

    /// 光标格当前的屏幕位置(缩放锚点)
    fn cursor_screen_cell(&self) -> render::Cell {
        let p = render::panels(&self.doc, self.area, &self.view);
        p.proj
            .cell((self.view.cursor.0 as f64, self.view.cursor.1 as f64))
    }

    /// 以屏幕上的 `anchor` 格为锚点换缩放:缩放前后那一格下面的画布坐标不变.
    /// 指针/光标就不会在这次缩放里"跑掉"
    fn zoom_at(&mut self, zoom: f64, anchor: render::Cell) {
        let zoom = snap_zoom(clamp_zoom(zoom));
        if (zoom - self.view.zoom).abs() < 1e-9 {
            return;
        }
        let p = render::panels(&self.doc, self.area, &self.view);
        let point = p.proj.canvas_f(anchor.0, anchor.1);
        let (cx, cy) = p.proj.center_for(point, anchor.0, anchor.1, zoom);
        self.view.zoom = zoom;
        self.view.center = Some((cx, cy));
        self.view.target = None;
        self.view.pan = None;
        self.note_zoom();
    }

    /// 光标格下面是不是一个框
    fn cursor_box(&self) -> Option<usize> {
        self.view.pick.box_at()
    }

    /// 现在选中的边:没选,或者下标已经越界都算没选
    /// 真实终端光标该放哪:命令/搜索输入时在输入行末尾,否则在光标框的文字起点.
    /// 不在视口内就返回 None(让光标藏起来,别被终端夹到边上)
    pub fn cursor_cell(&self) -> Option<(u16, u16)> {
        let area = self.area;
        if area.width == 0 || area.height == 0 {
            return None;
        }
        if self.view.help || self.view.file_pick.is_some() {
            return None;
        }
        let status_y = area.y + area.height - 1;
        if let Some((what, typed)) = &self.view.prompt {
            let prefix = match what {
                Prompt::NewBox => "框: ",
                Prompt::Text(_) => "文字: ",
                Prompt::Link(_) => "连到: ",
            };
            let shown = render::prompt_display(typed, self.view.lang);
            // 光标在 `prompt_pos` 处:多行内容被折成"第一行 [+N 行]",光标只按第一行算
            let first_line: String = typed
                .split('\n')
                .next()
                .unwrap_or("")
                .chars()
                .take(self.view.prompt_pos)
                .collect();
            let _ = &shown;
            let before = render::raw_width(&first_line);
            let x = area.x + render::raw_width(prefix) as u16 + before as u16;
            return (x < area.x + area.width).then_some((x, status_y));
        }
        if let Some(c) = &self.view.cmd {
            // 状态行画的是 `:w hell` 这串本身,光标落在这串的右边一格.
            // 这里必须按原样宽度算:路径里的 `/`、文字里的 `*` 都不是标记
            let before: String = c.chars().take(self.view.cmd_pos).collect();
            let x = area.x + 1 + render::raw_width(&before) as u16;
            return (x < area.x + area.width).then_some((x, status_y));
        }
        if let Some(q) = &self.view.query {
            let before: String = q.chars().take(self.view.query_pos).collect();
            let x = area.x + 1 + render::raw_width(&before) as u16;
            return (x < area.x + area.width).then_some((x, status_y));
        }
        if self.doc.boxes.is_empty() {
            return None;
        }
        let p = render::panels(&self.doc, area, &self.view);
        let (cx, cy) = p
            .proj
            .cell((self.view.cursor.0 as f64, self.view.cursor.1 as f64));
        let (x, y) = (cx as u16, cy as u16);
        let inside =
            x >= p.map.x && x < p.map.x + p.map.width && y >= p.map.y && y < p.map.y + p.map.height;
        inside.then_some((x, y))
    }

    /// 这次重排该不该转 90 度(以及转几份).
    /// auto 只在 force / layered 上生效:geo 与 relax 的输入形状本身有方向含义
    fn layout_quarters(&self, l: Layout) -> i32 {
        if let Some(k) = self.view.orientation.quarters() {
            return k;
        }
        if !matches!(l, Layout::Force | Layout::Layered) {
            return 0;
        }
        let area = render::panels(&self.doc, self.area, &self.view).map;
        let (x0, y0, x1, y1) = self.doc.bbox();
        let (gw, gh) = ((x1 - x0 + 1) as f64, (y1 - y0 + 1) as f64);
        if gw <= 0.0 || gh <= 0.0 || area.width == 0 || area.height == 0 {
            return 0;
        }
        let cols = area.width as f64;
        let rows = area.height as f64;
        let normal = (gw / cols).max(gh / rows);
        let rotated = (gh / cols).max(gw / rows);
        // 差距不到 5% 就别转,免得临界点上左右横跳
        if rotated < normal * 0.95 {
            1
        } else {
            0
        }
    }

    /// `:rotate` —— 不给参数转 90 度,给角度就是绝对值,给 auto 交回自动选择
    fn cmd_rotate(&mut self, arg: Option<&str>) {
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

    /// 把坐标转到绝对角度 deg(要求 1:1)
    fn rotate_to(&mut self, o: layout::Orientation, deg: i32) {
        self.require_edit_scale();
        self.begin_edit("转方向");
        let k = (deg - self.view.angle).rem_euclid(360) / 90;
        if k != 0 {
            self.doc.rotate_quarters(k);
        }
        self.view.orientation = o;
        self.view.angle = deg.rem_euclid(360);
        let note = self.view.lang.pick_owned(
            format!("方向 = {}(坐标已转)", self.view.angle),
            format!("orientation = {} (coordinates rotated)", self.view.angle),
        );
        self.view.note = Some(note);
        self.end_edit();
    }

    /// 想退出时给一句:退出只认 :q
    fn note_quit_hint(&mut self) {
        self.view.note = Some(
            self.view
                .lang
                .pick_owned("退出用 :q".to_string(), "quit with :q".to_string()),
        );
    }

    /// `:notes [on|off]`:注释总开关(和 a 键同一个)
    fn cmd_notes(&mut self, arg: Option<&str>) {
        match arg.unwrap_or("").trim().to_ascii_lowercase().as_str() {
            "" => self.view.notes = !self.view.notes,
            "on" | "true" | "1" => self.view.notes = true,
            "off" | "false" | "0" => self.view.notes = false,
            other => {
                let note = self.view.lang.pick_owned(
                    format!("notes 只认 on / off,不认 {other}"),
                    format!("notes takes on / off, not {other}"),
                );
                self.view.note = Some(note);
                return;
            }
        }
        let on = self.view.notes;
        self.view.note = Some(self.view.lang.pick_owned(
            if on {
                "注释:开".to_string()
            } else {
                "注释:关".to_string()
            },
            if on {
                "notes: on".to_string()
            } else {
                "notes: off".to_string()
            },
        ));
    }

    /// `:relabel [all]`:把注释框重新贴回它那条边旁边(只跑一次,不跟手)。
    /// 不给参数就只动选中的注释框
    fn cmd_relabel(&mut self, rest: &str) {
        let all = rest.trim().eq_ignore_ascii_case("all");
        let picked: Vec<usize> = if all {
            (0..self.doc.boxes.len())
                .filter(|&i| self.doc.is_note(i))
                .collect()
        } else {
            self.targets()
                .boxes
                .into_iter()
                .filter(|&i| self.doc.is_note(i))
                .collect()
        };
        if picked.is_empty() {
            let note = self.view.lang.pick_owned(
                "没有注释框可贴(选中注释框,或 :relabel all)".to_string(),
                "no note box to move (select one, or :relabel all)".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        self.begin_edit("重贴注释");
        let n = picked.len();
        for i in picked {
            let Some(e) = self.doc.edge_of_box(i) else {
                continue;
            };
            let at = self.doc.note_spot(e);
            self.doc.place_box(i, at);
        }
        self.doc.rebuild();
        self.end_edit();
        self.refresh_pick();
        self.view.note = Some(self.view.lang.pick_owned(
            format!("注释已重贴({n} 个)"),
            format!("moved {n} note box(es)"),
        ));
    }

    /// [--glyph solid|dashed|dotted|arrow] <文字>` / `rm <n>` 改文档
    /// `:new <文字>` / i:在光标那一格新建一个框,光标跟过去
    fn cmd_new(&mut self, label: &str) {
        self.require_edit_scale();
        self.begin_edit("建框");
        let (x, y) = self.free_spot(label);
        // 默认样式就是 none(只有文字的节点);最近选中过框就照抄它的样式/颜色/对齐.
        // 区块选区时 view.sel 有值,那不算"选中一个元素",照默认来
        let look = self.box_look_now();
        let style = look.map(|l| l.style).unwrap_or_default();
        match self.doc.add_box(label, (x, y), style) {
            Ok(i) => {
                if label.is_empty() {
                    // 没给文字就拿框 id 当文字,不然框里是空的,看不见
                    let id = self.doc.boxes[i].id.clone();
                    self.doc.set_text(i, id);
                }
                if let Some(l) = look {
                    self.doc.boxes[i].color = l.color;
                    self.doc.boxes[i].fill = l.fill;
                    if let Some(ti) = self.doc.text_of(i) {
                        self.doc.texts[ti].color = l.text_color;
                        self.doc.texts[ti].bg = l.text_bg;
                        self.doc.texts[ti].align = l.align;
                    }
                    self.doc.rebuild();
                }
                self.view.cursor = self.cell_of(i);
                self.view.sel = None;
                self.view.cand = 0;
                self.refresh_pick();
                let name = label_of(&self.doc, i);
                let note = self.view.lang.pick_owned(
                    format!("新建 {name} @({x},{y})"),
                    format!("added {name} at ({x},{y})"),
                );
                self.view.note = Some(note);
            }
            Err(e) => self.view.note = Some(e),
        }
        self.end_edit();
    }

    /// `:link <目标>`:从光标节点拉一条边过去(目标可写 id 或唯一的文字)
    fn cmd_link(&mut self, target: &str) {
        self.require_edit_scale();
        self.begin_edit("连边");
        let Some(i) = self.cursor_box() else {
            let note = self.view.lang.pick_owned(
                "没有光标节点:先用 :new 建一个".to_string(),
                "no cursor box: create one with :new".to_string(),
            );
            self.view.note = Some(note);
            return;
        };
        let Some(j) = self.resolve_box(target) else {
            let note = self.view.lang.pick_owned(
                format!("找不到唯一的框:{target}"),
                format!("no unique box matches {target}"),
            );
            self.view.note = Some(note);
            return;
        };
        let look = self.edge_look_now();
        let group = look
            .as_ref()
            .map(|l| l.group.clone())
            .unwrap_or_else(|| self.link_group(i).to_string());
        let from = self.doc.boxes[i].id.clone();
        let to = self.doc.boxes[j].id.clone();
        match self.doc.add_edge(&from, &to, &group, Dir::Forward) {
            Ok(e) => {
                if let Some(l) = &look {
                    self.apply_edge_look(e, l);
                }
                let a = label_of(&self.doc, i);
                let b = label_of(&self.doc, j);
                let note = self
                    .view
                    .lang
                    .pick_owned(format!("连上 {a} -> {b}"), format!("linked {a} -> {b}"));
                self.view.note = Some(note);
            }
            Err(e) => {
                let note = self
                    .view
                    .lang
                    .pick_owned(format!("连不上:{e}"), format!("cannot link: {e}"));
                self.view.note = Some(note);
            }
        }
        self.end_edit();
    }

    /// `:del`:有选中边就删边,否则删光标框(连它的文字与边)
    fn cmd_del(&mut self, include_edges: bool) {
        if self.blocked_by_selection() {
            return;
        }
        let set = self.targets();
        if set.is_empty() {
            let note = self.view.lang.pick_owned(
                "光标下没有可删的东西".to_string(),
                "nothing to delete here".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        self.require_edit_scale();
        // 至少留一个框:整批删完就一个不剩时,整次拒绝
        let left = self.doc.boxes.len() - set.boxes.len();
        if !set.boxes.is_empty() && left < 1 {
            let note = self.view.lang.pick_owned(
                "最后一个框不能删(空文档没法编辑)".to_string(),
                "cannot remove every box (nothing left to edit)".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        let link = self.link_ids();
        self.begin_edit("删除");
        let (clip, ne) = self.take_for_clipboard(&set, include_edges);
        let nb = set.boxes.len();
        // 先删边,再删框(框删了也会带走它的边).
        // id 要先全抄下来:删一条后面的下标就全移了
        let edge_ids: Vec<String> = set
            .edges
            .iter()
            .filter_map(|e| self.doc.edges.get(*e).map(|e| e.id.clone()))
            .collect();
        for id in edge_ids {
            let _ = self.doc.remove_edge(&id);
        }
        for &i in set.boxes.iter().rev() {
            let id = self.doc.boxes[i].id.clone();
            let _ = self.doc.remove_box(&id);
        }
        self.clipboard = Some(clip);
        self.view.sel = None;
        self.view.cand = 0;
        // 光标留在原地:删掉的东西挪走了,光标格不该跟着跑
        self.refresh_pick();
        self.end_edit();
        let note = if include_edges {
            format!("剪切 {nb} 框 {ne} 边(含边)")
        } else {
            format!("剪切 {nb} 框 {ne} 边(仅框)")
        };
        let note_en = if include_edges {
            format!("cut {nb} box(es), {ne} edge(s) (with edges)")
        } else {
            format!("cut {nb} box(es), {ne} edge(s) (boxes only)")
        };
        let note = self.view.lang.pick_owned(note, note_en);
        self.view.note = Some(note);
        // 连边还开着的话,按 id 把源和目标找回来(源没了它会自己退出)
        self.rebind_link(link);
    }

    /// 复制图片段:剪贴板只收"选中的框 + 它们的内部边 + 显式选中的边".
    /// 返回 (片段, 会因为删这些框而消失的边数).
    fn take_for_clipboard(&self, set: &TargetSet, include_edges: bool) -> (Clipboard, usize) {
        let mut boxes = Vec::new();
        let mut texts = Vec::new();
        let mut edges = Vec::new();
        let mut groups: Vec<crate::document::Group> = Vec::new();
        let mut origin: Option<(i32, i32)> = None;
        let mut note = |c: (i32, i32)| {
            origin = Some(match origin {
                Some(o) => (o.0.min(c.0), o.1.min(c.1)),
                None => c,
            });
        };
        let removed: std::collections::HashSet<&str> = set
            .boxes
            .iter()
            .map(|&i| self.doc.boxes[i].id.as_str())
            .collect();
        for &i in &set.boxes {
            let b = self.doc.boxes[i].clone();
            note(b.at);
            boxes.push(b);
            if let Some(t) = self.doc.text(i) {
                note(t.at);
                texts.push(t.clone());
            }
        }
        // 边界边不算片段的一部分(另一头留在文档里),但删的时候照样跟着框一起消失
        let mut removed_edges = 0usize;
        let mut seen = std::collections::HashSet::new();
        for (k, e) in self.doc.edges.iter().enumerate() {
            let touches = removed.contains(e.from.as_str()) || removed.contains(e.to.as_str());
            if touches {
                removed_edges += 1;
            }
            let inside = removed.contains(e.from.as_str()) && removed.contains(e.to.as_str());
            let take = set.edges.contains(&k) || inside;
            if !take || !seen.insert(k) {
                continue;
            }
            edges.push(e.clone());
            // 边的两端与折点不参与"原点":另一头可能还留在文档里(位置很远),
            // 拿它当基准会让粘贴落点跟着边跑,和没有边时完全不一样
            if let Some(g) = self.doc.groups.iter().find(|g| g.id == e.group) {
                if !groups.iter().any(|x| x.id == g.id) {
                    groups.push(g.clone());
                }
            }
        }
        let edges = if include_edges { edges } else { Vec::new() };
        let groups = if include_edges { groups } else { Vec::new() };
        // 片段用到的图层:按文档里的顺序抄一份,粘贴时照它建新层
        let mut layers: Vec<crate::document::Layer> = Vec::new();
        for name in boxes
            .iter()
            .map(|b| b.layer.clone())
            .chain(edges.iter().map(|e| e.layer.clone()))
        {
            if let Some(l) = self.doc.layers.iter().find(|l| l.name == name) {
                if !layers.iter().any(|x| x.name == l.name) {
                    layers.push(l.clone());
                }
            }
        }
        (
            Clipboard {
                origin: origin.unwrap_or(self.view.cursor),
                boxes,
                texts,
                edges,
                groups,
                layers,
            },
            removed_edges,
        )
    }

    /// y:把图片段(框 + 文字 + 内部边/选中的边)复制到剪贴板,不动文档
    fn cmd_yank(&mut self) {
        let set = self.targets();
        if set.is_empty() {
            let note = self.view.lang.pick_owned(
                "没有可复制的东西".to_string(),
                "nothing to yank".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        let (clip, _) = self.take_for_clipboard(&set, true);
        let (nb, ne) = (clip.boxes.len(), clip.edges.len());
        self.clipboard = Some(clip);
        let note = format!("yanked {nb} boxes + {ne} edges");
        self.view.note = Some(note);
    }

    /// S:在光标所在行与下一行之间把框切开(第一段留原框,其余进新框)
    fn cmd_split_box(&mut self) {
        let Some(i) = self.doc.box_at_cell(self.view.cursor) else {
            let note = self.view.lang.pick_owned(
                "光标不在框上,没法拆".to_string(),
                "the cursor is not on a box".to_string(),
            );
            self.view.note = Some(note);
            return;
        };
        if self.doc.boxes[i].style.is_point() || self.doc.is_note(i) {
            let note = self.view.lang.pick_owned(
                "点节点和边注释不能拆".to_string(),
                "a point node or an edge note cannot be split".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        let Some(t) = self.doc.text(i) else {
            return;
        };
        let (text, t_at) = (t.text.clone(), t.at);
        let b = self.doc.boxes[i].clone();
        let origin = if b.style.is_boxed() {
            (b.at.0 + 1, b.at.1 + 1)
        } else {
            t_at
        };
        let row = self.view.cursor.1 - origin.1;
        let lines: Vec<&str> = text.split('\n').collect();
        if row < 0 || row as usize >= lines.len() {
            let note = self.view.lang.pick_owned(
                "光标不在框里的文字上,没法拆".to_string(),
                "the cursor is not on the box text".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        let row = row as usize;
        if row + 1 >= lines.len() {
            let note = self.view.lang.pick_owned(
                "光标在最后一行,后面没有内容".to_string(),
                "nothing after this line".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        self.require_edit_scale();
        let head: String = lines[..=row].join("\n");
        let tail: String = lines[row + 1..].join("\n");
        self.begin_edit("拆框");
        self.doc.set_text(i, head);
        // 新框放右边,顶部对齐、隔一格;那儿有东西就往下/往右找空位
        let mut at = (b.at.0 + b.size.0 as i32 + 1, b.at.1);
        for _ in 0..32 {
            if self.doc.boxes.iter().all(|o| {
                let (ox, oy, ow, oh) = (o.at.0, o.at.1, o.size.0 as i32, o.size.1 as i32);
                at.0 + 1 > ox + ow
                    || at.0 < ox - 1
                    || at.1 + 1 > oy + oh
                    || at.1 < oy - 1
                    || (o.at == b.at)
            }) {
                break;
            }
            at = (at.0, at.1 + b.size.1 as i32 + 1);
        }
        let style = b.style;
        let Ok(n) = self.doc.add_box("", at, style) else {
            self.end_edit();
            return;
        };
        self.doc.set_text(n, tail);
        {
            let src = self.doc.boxes[i].clone();
            let dst = &mut self.doc.boxes[n];
            dst.color = src.color;
            dst.fill = src.fill;
            dst.layer = src.layer.clone();
        }
        if let (Some(st), Some(dt)) = (self.doc.text(i).cloned(), self.doc.text_of(n)) {
            let t = &mut self.doc.texts[dt];
            t.color = st.color;
            t.bg = st.bg;
            t.align = st.align;
            t.layer = st.layer;
        }
        self.doc.rebuild();
        self.view.cursor = origin;
        self.refresh_pick();
        self.end_edit();
        let note = self.view.lang.pick_owned(
            "已在下一行处拆开".to_string(),
            "split below the cursor line".to_string(),
        );
        self.view.note = Some(note);
    }

    /// M:把选区里的多个框合成一个(文字按阅读顺序接起来)
    fn cmd_merge_boxes(&mut self) {
        if self.blocked_by_selection() {
            return;
        }
        let set = self.targets();
        if set.boxes.len() < 2 {
            let note = self.view.lang.pick_owned(
                "至少要选中两个框".to_string(),
                "select at least two boxes".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        // 目标框:光标所在的那个,否则阅读顺序第一个(上到下、左到右)
        let mut order = set.boxes.clone();
        order.sort_by_key(|&i| (self.doc.boxes[i].at.1, self.doc.boxes[i].at.0));
        let target = self
            .doc
            .box_at_cell(self.view.cursor)
            .filter(|t| order.contains(t))
            .unwrap_or(order[0]);
        // 文字:按阅读顺序拼,空的跳过
        let mut parts: Vec<String> = Vec::new();
        for &i in &order {
            if let Some(t) = self.doc.text(i) {
                if !t.text.trim().is_empty() {
                    parts.push(t.text.clone());
                }
            }
        }
        let merged = parts.join("\n");
        let kept: std::collections::HashSet<String> = set
            .boxes
            .iter()
            .map(|&i| self.doc.boxes[i].id.clone())
            .collect();
        let target_id = self.doc.boxes[target].id.clone();
        self.require_edit_scale();
        self.begin_edit("合并");
        self.doc.set_text(target, merged);
        // 边:内部边删掉,边界边改接到目标框
        let mut doomed: Vec<String> = Vec::new();
        let mut rewired: Vec<(String, crate::document::DocEdge)> = Vec::new();
        for e in self.doc.edges.clone() {
            let from_in = kept.contains(&e.from);
            let to_in = kept.contains(&e.to);
            match (from_in, to_in) {
                (true, true) => doomed.push(e.id.clone()),
                (true, false) => {
                    let mut ne = e.clone();
                    ne.from = target_id.clone();
                    rewired.push((e.id.clone(), ne));
                }
                (false, true) => {
                    let mut ne = e.clone();
                    ne.to = target_id.clone();
                    rewired.push((e.id.clone(), ne));
                }
                (false, false) => {}
            }
        }
        for id in &doomed {
            let _ = self.doc.remove_edge(id);
        }
        // 改接:先删后加(端点相同、方向/分组/线型一样的合成一条)
        for (old_id, ne) in rewired {
            if ne.from == ne.to && ne.id == old_id {
                // 自己连自己:保持原样
                continue;
            }
            let _ = self.doc.remove_edge(&old_id);
            let dup = self.doc.edges.iter().any(|x| {
                x.from == ne.from
                    && x.to == ne.to
                    && x.dir == ne.dir
                    && x.group == ne.group
                    && x.style == ne.style
            });
            if dup || ne.from == ne.to {
                continue;
            }
            if let Ok(k) = self.doc.add_edge(&ne.from, &ne.to, &ne.group, ne.dir) {
                self.doc.edges[k].style = ne.style;
                self.doc.edges[k].color = ne.color;
                self.doc.edges[k].bg = ne.bg;
                self.doc.edges[k].corner = ne.corner;
                self.doc.edges[k].from_side = ne.from_side;
                self.doc.edges[k].to_side = ne.to_side;
                self.doc.edges[k].layer = ne.layer;
                self.doc.edges[k].head = ne.head;
            }
        }
        // 删掉其它框(连它们的文字)
        for &i in order.iter().filter(|&&i| i != target) {
            let id = self.doc.boxes[i].id.clone();
            let _ = self.doc.remove_box(&id);
        }
        self.doc.rebuild();
        self.view.sel = None;
        self.view.cursor = self.cell_of(self.doc.box_idx(&target_id).unwrap_or(0));
        self.refresh_pick();
        self.end_edit();
        let note = self.view.lang.pick_owned(
            format!("合并了 {} 个框", set.boxes.len()),
            format!("merged {} boxes", set.boxes.len()),
        );
        self.view.note = Some(note);
    }

    /// p:把剪贴板里的东西按当前光标偏移粘一份新的
    fn cmd_paste(&mut self) {
        if self.blocked_by_selection() {
            return;
        }
        let Some(clip) = self.clipboard.clone() else {
            let note = self
                .view
                .lang
                .pick_owned("剪贴板为空".to_string(), "clipboard is empty".to_string());
            self.view.note = Some(note);
            return;
        };
        self.require_edit_scale();
        self.begin_edit("粘贴");
        let (dx, dy, crowded) = self.paste_offset(&clip);

        // 新 id:框和边分开编号,避开已有 id
        let mut box_ids: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut n = 1;
        for b in &clip.boxes {
            loop {
                let id = format!("b{n}");
                n += 1;
                if self.doc.box_idx(&id).is_none() && box_ids.values().all(|v| *v != id) {
                    box_ids.insert(b.id.clone(), id);
                    break;
                }
            }
        }
        let mut text_ids: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut t = 1;
        for tx in &clip.texts {
            loop {
                let id = format!("t{t}");
                t += 1;
                if !self.doc.texts.iter().any(|x| x.id == id) && text_ids.values().all(|v| *v != id)
                {
                    text_ids.insert(tx.id.clone(), id);
                    break;
                }
            }
        }
        // 分组:id 或 label 对上就复用,否则新建
        let mut group_of: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for g in &clip.groups {
            let reuse = self
                .doc
                .groups
                .iter()
                .find(|x| x.id == g.id || (!g.label.is_empty() && x.label == g.label))
                .map(|x| x.id.clone());
            if let Some(id) = reuse {
                group_of.insert(g.id.clone(), id);
            } else {
                let mut k = self.doc.groups.len();
                let id = loop {
                    let id = format!("g{k}");
                    if self.doc.group_idx(&id).is_none() {
                        break id;
                    }
                    k += 1;
                };
                self.doc.groups.push(crate::document::Group {
                    id: id.clone(),
                    label: g.label.clone(),
                    color: g.color,
                });
                group_of.insert(g.id.clone(), id);
            }
        }
        // 图层:有就复用,没有就建
        let mut layer_of: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for name in clip
            .boxes
            .iter()
            .map(|b| b.layer.clone())
            .chain(clip.edges.iter().map(|e| e.layer.clone()))
        {
            if name.is_empty() || self.doc.layers.iter().any(|l| l.name == name) {
                layer_of.insert(name.clone(), name);
                continue;
            }
            let order = self.doc.layers.iter().map(|l| l.order).max().unwrap_or(0) + 1;
            self.doc.layers.push(crate::document::Layer {
                name: name.clone(),
                order,
            });
            layer_of.insert(name.clone(), name);
        }

        // 框 + 文字
        let mut new_boxes: Vec<usize> = Vec::new();
        for b in &clip.boxes {
            let mut nb = b.clone();
            nb.id = box_ids[&b.id].clone();
            nb.at = (b.at.0 + dx, b.at.1 + dy);
            nb.layer = layer_of.get(&b.layer).cloned().unwrap_or_default();
            let at = nb.at;
            new_boxes.push(self.doc.boxes.len());
            self.doc.boxes.push(nb);
            if let Some(t) = clip.texts.iter().find(|t| t.box_id == b.id) {
                let mut nt = t.clone();
                nt.id = text_ids[&t.id].clone();
                nt.box_id = box_ids[&b.id].clone();
                nt.at = if b.style.is_boxed() {
                    (at.0 + 1, at.1 + 1)
                } else {
                    (t.at.0 + dx, t.at.1 + dy)
                };
                nt.layer = layer_of.get(&t.layer).cloned().unwrap_or_default();
                self.doc.texts.push(nt);
            }
        }
        // 边:两端都在剪贴板里就换新 id,另一端还在就保留,否则跳过
        let mut nb_edges = 0usize;
        let mut skipped = 0usize;
        for e in &clip.edges {
            let from = box_ids.get(&e.from).cloned();
            let to = box_ids.get(&e.to).cloned();
            let endpoint = |id: &String, mapped: Option<String>| -> Option<String> {
                if let Some(new) = mapped {
                    return Some(new);
                }
                // 剪贴板里的框被粘出来之前就已经没了:看原文档里还在不在
                self.doc.box_idx(id).map(|_| id.clone())
            };
            let (Some(f), Some(t)) = (endpoint(&e.from, from), endpoint(&e.to, to)) else {
                skipped += 1;
                continue;
            };
            let mut ne = e.clone();
            let mut k = 1;
            let id = loop {
                let id = format!("e{k}");
                k += 1;
                if !self.doc.edges.iter().any(|x| x.id == id) {
                    break id;
                }
            };
            ne.id = id;
            ne.from = f;
            ne.to = t;
            ne.group = group_of.get(&e.group).cloned().unwrap_or(e.group.clone());
            ne.layer = layer_of.get(&e.layer).cloned().unwrap_or_default();
            ne.via = e.via.iter().map(|&(x, y)| (x + dx, y + dy)).collect();
            self.doc.edges.push(ne);
            nb_edges += 1;
        }
        self.doc.rebuild();
        self.view.sel = None;
        // 旧命中表里的下标对不上新文档了,直接丢掉
        self.hit = render::FrameHit::default();
        self.view.cand = 0;
        // 光标落到第一份新框上:下一次 d/x/样式命令默认作用于刚粘的那份
        if let Some(&i) = new_boxes.first() {
            self.view.cursor = self.cell_of(i);
        }
        self.refresh_pick();
        self.end_edit();
        let nb = clip.boxes.len();
        let mut zh = format!("粘贴 {nb} 框 {nb_edges} 边");
        let mut en = format!("pasted {nb} box(es), {nb_edges} edge(s)");
        if skipped > 0 {
            zh.push_str(&format!(",跳过 {skipped} 条悬空边"));
            en.push_str(&format!(", skipped {skipped} dangling edge(s)"));
        }
        if crowded {
            zh.push_str(",位置拥挤,已叠放");
            en.push_str(", crowded: pasted on top");
        }
        self.view.note = Some(self.view.lang.pick_owned(zh, en));
    }

    /// 剪贴板内容的宽高(框的外包盒,至少 1)
    fn clip_size(&self, clip: &Clipboard) -> (i32, i32) {
        let mut x1 = clip.origin.0 + 1;
        let mut y1 = clip.origin.1 + 1;
        for b in &clip.boxes {
            x1 = x1.max(b.at.0 + b.size.0 as i32);
            y1 = y1.max(b.at.1 + b.size.1 as i32);
        }
        ((x1 - clip.origin.0).max(1), (y1 - clip.origin.1).max(1))
    }

    /// 按这个偏移粘过去,会不会压到文档里已有的框
    fn paste_overlaps(&self, clip: &Clipboard, d: (i32, i32)) -> bool {
        clip.boxes.iter().any(|b| {
            let (w, h) = (b.size.0 as i32, b.size.1 as i32);
            let (x, y) = (b.at.0 + d.0, b.at.1 + d.1);
            self.doc.boxes.iter().any(|o| {
                let (ow, oh) = (o.size.0 as i32, o.size.1 as i32);
                x < o.at.0 + ow && o.at.0 < x + w && y < o.at.1 + oh && o.at.1 < y + h
            })
        })
    }

    /// 粘贴落点:先试光标处的精确位置,压到别的框就按右 -> 下 -> 右下错开,
    /// 最多找 32 个候选;都挤就退回精确位置(回显里说明)
    fn paste_offset(&self, clip: &Clipboard) -> (i32, i32, bool) {
        let base = (
            self.view.cursor.0 - clip.origin.0,
            self.view.cursor.1 - clip.origin.1,
        );
        if clip.boxes.is_empty() {
            return (base.0, base.1, false);
        }
        let (w, h) = self.clip_size(clip);
        for k in 0..32i32 {
            let (ox, oy) = if k == 0 {
                (0, 0)
            } else {
                let n = (k + 2) / 3;
                match k % 3 {
                    0 => (n * (w + 1), 0),
                    1 => (0, n * (h + 1)),
                    _ => (n * (w + 1), n * (h + 1)),
                }
            };
            let d = (base.0 + ox, base.1 + oy);
            if !self.paste_overlaps(clip, d) {
                return (d.0, d.1, false);
            }
        }
        (base.0, base.1, true)
    }

    /// 新框放哪:光标那一行的右边,离开现有内容两格
    /// 新框放哪:光标那一格;压到别的框就一格一格往右让,直到空出来
    fn free_spot(&self, label: &str) -> (i32, i32) {
        let w = (crate::text::plain_width(label).max(2) + 2).max(4) as i32;
        let mut at = self.view.cursor;
        loop {
            let hit = self.doc.boxes.iter().any(|b| {
                let (bw, bh) = (b.size.0 as i32, b.size.1 as i32);
                at.0 < b.at.0 + bw && b.at.0 < at.0 + w && at.1 < b.at.1 + bh && b.at.1 < at.1 + 3
            });
            if !hit {
                return at;
            }
            at.0 += 1;
        }
    }

    /// 新边挂哪个分组:光标节点已有的分组,没有就用第一个分组
    fn link_group(&self, i: usize) -> &str {
        self.doc
            .box_groups(i)
            .first()
            .map(|&g| self.doc.groups[g].id.as_str())
            .or_else(|| self.doc.groups.first().map(|g| g.id.as_str()))
            .unwrap_or("g0")
    }

    /// 按 id 或文字找框;文字命中多个就不算(要写 id)
    fn resolve_box(&self, target: &str) -> Option<usize> {
        let t = target.trim();
        if let Some(i) = self.doc.box_idx(t) {
            return Some(i);
        }
        let hits: Vec<usize> = (0..self.doc.boxes.len())
            .filter(|&i| label_of(&self.doc, i) == t)
            .collect();
        match hits.len() {
            1 => hits.first().copied(),
            _ => None,
        }
    }

    fn targets(&self) -> TargetSet {
        let p = render::panels(&self.doc, self.area, &self.view);
        let mut set = TargetSet::default();
        if self.view.sel.is_some() {
            let (boxes, edges) = render::selected_all(&self.doc, &p, &self.view, self.usable_hit());
            set = TargetSet { boxes, edges };
        } else {
            // 没选区就按光标下的东西
            match self.view.pick {
                render::Pick::Box(i) => set.boxes.push(i),
                render::Pick::Edge(e) => set.edges.push(e),
                render::Pick::None => {}
            }
        }
        // 只看一层的时候,动词也只动这一层
        if let Some(layer) = &self.view.layer {
            set.boxes.retain(|i| &self.doc.boxes[*i].layer == layer);
            set.edges.retain(|e| &self.doc.edges[*e].layer == layer);
        }
        set
    }

    /// 连边的源与目标现在对应的框 id(结构变动前后对照用)
    fn link_ids(&self) -> (Vec<String>, Vec<String>) {
        let Some(link) = &self.view.link else {
            return (Vec::new(), Vec::new());
        };
        let ids = |v: &[usize]| -> Vec<String> {
            v.iter()
                .filter_map(|i| self.doc.boxes.get(*i).map(|b| b.id.clone()))
                .collect()
        };
        (ids(&link.sources), ids(&link.targets))
    }

    /// 文档结构变了(删除 / 撤销 / 重做):按 id 把连边的源和目标找回来;
    /// 源一个都不剩就退出连边模式
    fn rebind_link(&mut self, ids: (Vec<String>, Vec<String>)) {
        let doc = &self.doc;
        let Some(link) = self.view.link.as_mut() else {
            return;
        };
        link.sources = ids.0.iter().filter_map(|id| doc.box_idx(id)).collect();
        link.targets = ids.1.iter().filter_map(|id| doc.box_idx(id)).collect();
        link.confirming = false;
        if link.sources.is_empty() {
            self.view.link = None;
            let note = self.view.lang.pick_owned(
                "源框已经被删掉,连边取消".to_string(),
                "the source boxes are gone: linking cancelled".to_string(),
            );
            self.view.note = Some(note);
        }
    }

    /// e 第一段:确定源框,进入连边模式
    fn begin_link(&mut self) {
        // 源选区没确认就先确认(选区本来就是源)
        if matches!(self.view.sel, Some(sel) if !sel.confirmed) {
            self.confirm_selection();
        }
        let set = self.targets();
        let mut sources = set.boxes;
        sources.sort_unstable();
        sources.dedup();
        if sources.is_empty() {
            let note = self.view.lang.pick_owned(
                "先在框上开始(光标停在框上,或用 v 框住几个)".to_string(),
                "start on a box (put the cursor on one, or v to select several)".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        let ignored = set.edges.len();
        self.require_edit_scale();
        // 选区让给"选目标"用
        self.view.sel = None;
        self.view.link = Some(LinkState {
            sources: sources.clone(),
            targets: Vec::new(),
            confirming: false,
        });
        // 源数、目标数这些由状态栏实时报,这里只在"有边被忽略"时说一句
        if ignored > 0 {
            let note = self.view.lang.pick_owned(
                format!("选区里的 {ignored} 条边不参与连边"),
                format!("{} selected edge(s) ignored by linking", ignored),
            );
            self.view.note = Some(note);
        }
    }

    /// e 第二段:确定目标,数量确认后建边
    fn link_commit(&mut self) {
        let Some(link) = self.view.link.clone() else {
            return;
        };
        // 目标选区没确认就先确认
        if matches!(self.view.sel, Some(sel) if !sel.confirmed) {
            self.confirm_selection();
        }
        let p = render::panels(&self.doc, self.area, &self.view);
        let mut targets = render::link_targets(&self.doc, &p, &self.view);
        targets.retain(|t| *t < self.doc.boxes.len());
        if targets.is_empty() {
            let note = self.view.lang.pick_owned(
                "还没有目标:光标移到目标框上,或用 v 框住再 Enter".to_string(),
                "no target yet: move onto a target box, or v to select some".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        let total = link.sources.len() * targets.len();
        if total > 512 {
            let note = self.view.lang.pick_owned(
                format!("一次最多建 512 条(现在 {total} 条),把选区缩小一点"),
                format!("at most 512 edges at a time ({total} now): narrow the selection"),
            );
            self.view.note = Some(note);
            return;
        }
        if total > 64 && !link.confirming {
            if let Some(l) = self.view.link.as_mut() {
                l.confirming = true;
                l.targets = targets;
            }
            let note = self.view.lang.pick_owned(
                format!("再按 Enter 创建 {total} 条"),
                format!("press Enter again to create {total} edges"),
            );
            self.view.note = Some(note);
            return;
        }
        self.create_edges(&link.sources, &targets, total);
    }

    /// 批量建边:源 x 目标各一条 Forward,整次只算一步撤销
    fn create_edges(&mut self, sources: &[usize], targets: &[usize], total: usize) {
        let _ = total;
        self.require_edit_scale();
        // 只聚焦一个分组就用它,否则每个源用自己第一个分组
        let one = match self.view.focus.as_slice() {
            [gr] if *gr < self.doc.groups.len() => Some(self.doc.groups[*gr].id.clone()),
            _ => None,
        };
        // 最近选中过边:新边照抄它的样式与分组(聚焦了某个分组时聚焦优先)
        let look = self.edge_look_now();
        let mut made = 0usize;
        let mut dup = 0usize;
        let mut last = None;
        self.begin_edit("连边");
        for &s in sources {
            if s >= self.doc.boxes.len() {
                continue;
            }
            let group = one
                .clone()
                .or_else(|| look.as_ref().map(|l| l.group.clone()))
                .unwrap_or_else(|| self.link_group(s).to_string());
            let from = self.doc.boxes[s].id.clone();
            for &t in targets {
                if t >= self.doc.boxes.len() {
                    continue;
                }
                // 源就是目标:自环,照建
                let to = self.doc.boxes[t].id.clone();
                match self.doc.add_edge(&from, &to, &group, Dir::Forward) {
                    Ok(e) => {
                        if let Some(l) = &look {
                            self.apply_edge_look(e, l);
                        }
                        made += 1;
                        last = Some(t);
                    }
                    Err(_) => dup += 1,
                }
            }
        }
        self.end_edit();
        self.view.link = None;
        self.view.sel = None;
        if let Some(t) = last {
            self.jump_to(t);
        }
        self.refresh_pick();
        let mut zh = format!("建了 {made} 条边");
        let mut en = format!("created {made} edge(s)");
        if dup > 0 {
            zh.push_str(&format!(",重复 {dup}"));
            en.push_str(&format!(", {dup} already existed"));
        }
        self.view.note = Some(self.view.lang.pick_owned(zh, en));
    }

    /// Esc:退出连边模式,不动文档
    fn cancel_link(&mut self) {
        self.view.link = None;
        self.view.sel = None;
        let note = self
            .view
            .lang
            .pick_owned("取消连边".to_string(), "linking cancelled".to_string());
        self.view.note = Some(note);
    }

    /// i:光标下/选区里有东西就改它们的文字,什么都没有才新建框
    /// `w` / `b`:从光标往外射一条直线,光标跳到第一个撞到的**别的**框或边.
    ///
    /// 方向跟图的形状走:图比高宽就横着(前=右,后=左),否则竖着(前=下,后=上);
    /// 这样横着排的地铁图和竖着排的树都能用.撞不到东西就只在状态行说一句.
    fn jump_ray(&mut self, back: bool) {
        let (x0, y0, x1, y1) = self.doc.bbox();
        let horizontal = (x1 - x0) >= (y1 - y0);
        let (dx, dy) = match (horizontal, back) {
            (true, false) => (1, 0),
            (true, true) => (-1, 0),
            (false, false) => (0, 1),
            (false, true) => (0, -1),
        };
        let p = render::panels(&self.doc, self.area, &self.view);
        let start_box = self.doc.box_at_cell(self.view.cursor);
        let start_screen = p
            .proj
            .cell((self.view.cursor.0 as f64, self.view.cursor.1 as f64));
        let start_edge = self.hit.edge_at(start_screen);
        let steps = if horizontal {
            p.map.width as i32
        } else {
            p.map.height as i32
        };
        let mut cell = self.view.cursor;
        for _ in 0..steps {
            cell = (cell.0 + dx, cell.1 + dy);
            let screen = p.proj.cell((cell.0 as f64, cell.1 as f64));
            if screen.0 < p.map.x as i32
                || screen.1 < p.map.y as i32
                || screen.0 >= (p.map.x + p.map.width) as i32
                || screen.1 >= (p.map.y + p.map.height) as i32
            {
                break; // 射线走出画面:后面看不见的东西不跳
            }
            // 先看框(框盖住的东西不算),再看边
            if let Some(b) = self.doc.box_at_cell(cell) {
                if Some(b) != start_box {
                    self.goto_cell(cell);
                    return;
                }
            } else if let Some(e) = self.hit.edge_at(screen) {
                if Some(e) != start_edge {
                    self.goto_cell(cell);
                    return;
                }
            }
        }
        let note = if back {
            self.view.lang.pick_owned(
                "这个方向后面没有框或边了".to_string(),
                "nothing behind the cursor".to_string(),
            )
        } else {
            self.view.lang.pick_owned(
                "这个方向前面没有框或边了".to_string(),
                "nothing ahead of the cursor".to_string(),
            )
        };
        self.view.note = Some(note);
    }

    /// `i`:有框就在框里直接改字(不弹底行输入框),选的是边或空白就走原来的路
    fn cmd_edit_inline(&mut self) {
        // 只编辑光标下的那一个框:多选/区块选择不改形成"批量写同一段文字"
        if self.view.sel.is_some() {
            let note = self.view.lang.pick_owned(
                "选中了一个区块:请只把光标放在一个框上再按 i".to_string(),
                "a block is selected: put the cursor on one box to edit".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        if let Some(i) = self.doc.box_at_cell(self.view.cursor) {
            self.begin_box_edit(i, false);
            return;
        }
        if matches!(self.view.pick, render::Pick::Edge(_)) {
            // 光标在边上:仍是"建注释框 + 输入框"
            self.cmd_edit_or_new();
            return;
        }
        // 空白处:先建一个空框,再直接进编辑(建框一步,提交文字另一步)
        self.require_edit_scale();
        self.begin_edit("建框");
        let (x, y) = self.free_spot("");
        let look = self.box_look_now();
        let style = look.map(|l| l.style).unwrap_or_default();
        let Ok(i) = self.doc.add_box("", (x, y), style) else {
            self.end_edit();
            return;
        };
        if let Some(l) = look {
            self.doc.boxes[i].color = l.color;
            self.doc.boxes[i].fill = l.fill;
            if let Some(ti) = self.doc.text_of(i) {
                self.doc.texts[ti].color = l.text_color;
                self.doc.texts[ti].bg = l.text_bg;
                self.doc.texts[ti].align = l.align;
            }
            self.doc.rebuild();
        }
        self.view.cursor = self.cell_of(i);
        self.view.sel = None;
        self.view.cand = 0;
        self.refresh_pick();
        self.end_edit();
        self.begin_box_edit(i, true);
    }

    fn cmd_edit_or_new(&mut self) {
        let set = self.targets();
        if !set.boxes.is_empty() {
            if self.blocked_by_selection() {
                return;
            }
            // 预填现有文字:进来就能改,不用重打一遍
            let text = self.common_text(&set.boxes);
            self.open_prompt(Prompt::Text(set.boxes), text);
            return;
        }
        if !set.edges.is_empty() {
            // 选中边:给这条边建(或打开)注释框
            self.require_edit_scale();
            self.begin_edit("注释");
            let mut boxes = Vec::new();
            let mut fresh: Vec<String> = Vec::new();
            for &e in &set.edges {
                let had = self.doc.note_of_edge(&self.doc.edges[e].id).is_some();
                if let Some(b) = self.doc.new_note(e) {
                    boxes.push(b);
                    if !had {
                        fresh.push(self.doc.boxes[b].id.clone());
                    }
                }
            }
            self.end_edit();
            self.refresh_pick();
            if boxes.is_empty() {
                return;
            }
            // 新建的空框记下来:Esc 或空提交时删掉
            self.note_draft = fresh;
            let first = boxes[0];
            let text = self
                .doc
                .text(first)
                .map(|t| t.text.clone())
                .unwrap_or_default();
            self.open_prompt(Prompt::Text(boxes), text);
            return;
        }
        self.open_prompt(Prompt::NewBox, String::new());
    }

    /// 输入框取消:把这次刚建、还没写字的注释框删掉
    fn drop_note_draft(&mut self) {
        let ids = std::mem::take(&mut self.note_draft);
        if ids.is_empty() {
            return;
        }
        self.begin_edit("撤销注释");
        let mut gone = 0;
        for id in ids {
            let Some(i) = self.doc.box_idx(&id) else {
                continue;
            };
            let empty = self.doc.text(i).map(|t| t.text.is_empty()).unwrap_or(true);
            if empty {
                let _ = self.doc.remove_box(&id);
                gone += 1;
            }
        }
        self.end_edit();
        if gone > 0 {
            self.refresh_pick();
        }
    }

    /// Enter:把选区钉住(之后移动光标不再改变它)
    fn confirm_selection(&mut self) {
        let Some(sel) = self.view.sel else {
            return;
        };
        if sel.confirmed {
            return;
        }
        let p = render::panels(&self.doc, self.area, &self.view);
        let (boxes, edges) = render::selected_all(&self.doc, &p, &self.view, &self.hit);
        if boxes.is_empty() && edges.is_empty() {
            let note = self
                .view
                .lang
                .pick_owned("选区为空".to_string(), "the selection is empty".to_string());
            self.view.note = Some(note);
            return;
        }
        if let Some(s) = self.view.sel.as_mut() {
            s.end = self.view.cursor;
            s.confirmed = true;
        }
        let note = self.view.lang.pick_owned(
            format!("已确认:{} 框 {} 边", boxes.len(), edges.len()),
            format!(
                "confirmed: {} box(es), {} edge(s)",
                boxes.len(),
                edges.len()
            ),
        );
        self.view.note = Some(note);
    }

    /// 有未确认的选区时,改文档的命令一律拦住
    fn blocked_by_selection(&mut self) -> bool {
        if let Some(sel) = self.view.sel {
            if !sel.confirmed {
                let note = self.view.lang.pick_owned(
                    "先按 Enter 确认选区".to_string(),
                    "press Enter to confirm the selection first".to_string(),
                );
                self.view.note = Some(note);
                return true;
            }
        }
        false
    }

    /// 一批边一起改:一次 begin/end,只算一步撤销
    fn edit_edges(
        &mut self,
        mut f: impl FnMut(&mut crate::document::Document, usize),
        zh: String,
        en: String,
    ) {
        if self.blocked_by_selection() {
            return;
        }
        let es = self.targets().edges;
        if es.is_empty() {
            self.note_no_edge();
            return;
        }
        self.require_edit_scale();
        self.begin_edit("改边");
        for e in es {
            if e < self.doc.edges.len() {
                f(&mut self.doc, e);
            }
        }
        self.end_edit();
        self.view.note = Some(self.view.lang.pick_owned(zh, en));
    }

    /// Ctrl-hjkl:把选中的框(没选区就是光标框)按整数单元格挪一格
    fn move_boxes(&mut self, dir: (i32, i32), steps: i32) {
        let steps = steps.max(1);
        if self.blocked_by_selection() {
            return;
        }
        self.require_edit_scale();
        let ids = self.targets().boxes;
        if ids.is_empty() {
            let note = self
                .view
                .lang
                .pick_owned("没有可移动的框".to_string(), "no box to move".to_string());
            self.view.note = Some(note);
            return;
        }
        let (dx, dy) = (dir.0 * steps, dir.1 * steps);
        self.begin_edit("挪框");
        let n = ids.len();
        for i in ids {
            self.doc.move_box(i, dx, dy);
        }
        // 框挪了,光标与选择框跟着挪:不然按几下之后就"脱手"了,
        // 光标还停在原地、框已经走开
        self.view.cursor = (self.view.cursor.0 + dx, self.view.cursor.1 + dy);
        if let Some(sel) = self.view.sel.as_mut() {
            sel.anchor = (sel.anchor.0 + dx, sel.anchor.1 + dy);
            sel.end = (sel.end.0 + dx, sel.end.1 + dy);
        }
        self.refresh_pick();
        // 光标挪出画面边距时视图跟过去(和普通方向键同一套)
        self.follow_cursor();
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
            format!("挪了 {steps} 格{zh}"),
            format!("moved {steps} cell(s){en}"),
        ));
        self.end_edit();
    }

    /// 换层之后说一句:空字符串就是默认层
    fn note_layer(&mut self) {
        let text = match self.view.layer.clone() {
            None => self
                .view
                .lang
                .pick_owned("图层 = 全部".to_string(), "layer = all".to_string()),
            Some(l) => {
                let name = if l.is_empty() {
                    self.view.lang.pick("默认层", "default layer").to_string()
                } else {
                    l
                };
                self.view
                    .lang
                    .pick_owned(format!("图层 = {name}"), format!("layer = {name}"))
            }
        };
        self.view.note = Some(text);
    }

    /// L:全部 -> 各图层 -> 全部(层从 doc.layer_names() 来,默认层排最前)
    fn cycle_layer(&mut self) {
        let layers = self.doc.layer_names();
        let next = match self.view.layer.clone() {
            None => layers.first().cloned(),
            Some(cur) => {
                let at = layers.iter().position(|l| *l == cur);
                match at {
                    Some(i) if i + 1 < layers.len() => Some(layers[i + 1].clone()),
                    _ => None,
                }
            }
        };
        self.view.layer = next;
        self.note_layer();
    }

    /// :tolayer:把选中的框(连同它的文字),以及两端都在选区内的边,一起挪到某一层
    fn move_to_layer(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            let note = self.view.lang.pick_owned(
                "tolayer 缺参数:要移到哪一层".to_string(),
                "tolayer needs a layer name".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        self.require_edit_scale();
        if !self.doc.layers.iter().any(|l| l.name == name) {
            let order = self.doc.layers.iter().map(|l| l.order).max().unwrap_or(0) + 1;
            self.doc.layers.push(Layer {
                name: name.to_string(),
                order,
            });
        }
        let ids = self.targets().boxes;
        let mut inside = vec![false; self.doc.boxes.len()];
        for &i in &ids {
            if i < inside.len() {
                inside[i] = true;
            }
        }
        for &i in &ids {
            if i < self.doc.boxes.len() {
                self.doc.boxes[i].layer = name.to_string();
                if let Some(ti) = self.doc.text_of(i) {
                    self.doc.texts[ti].layer = name.to_string();
                }
            }
        }
        let mut edges = 0usize;
        let index: std::collections::HashMap<String, usize> = self
            .doc
            .boxes
            .iter()
            .enumerate()
            .map(|(i, b)| (b.id.clone(), i))
            .collect();
        for e in self.doc.edges.iter_mut() {
            let both = index
                .get(&e.from)
                .and_then(|i| inside.get(*i))
                .copied()
                .unwrap_or(false)
                && index
                    .get(&e.to)
                    .and_then(|i| inside.get(*i))
                    .copied()
                    .unwrap_or(false);
            if both {
                e.layer = name.to_string();
                edges += 1;
            }
        }
        let n = ids.len();
        self.view.note = Some(self.view.lang.pick_owned(
            format!("{n} 个框,{edges} 条边移到图层 {name}"),
            format!("moved {n} box(es) and {edges} edge(s) to layer {name}"),
        ));
    }

    /// 按画布大小开一个够大的 Buffer,把整幅图画进去再转成 SVG
    fn export_svg(&mut self, path: &str) {
        let path = &crate::paths::expand(path);
        let (x0, y0, x1, y1) = self.doc.bbox();
        // 上限 1000x400:再大就不是一张终端图了
        let w = ((x1 - x0 + 1).max(8) as u16)
            .saturating_add(4)
            .clamp(8, 1000);
        let h = ((y1 - y0 + 1).max(4) as u16)
            .saturating_add(2)
            .clamp(4, 400);
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        // 导出永远 1:1:缓冲已经按文档包围盒开好,缩放会把格子压扁、文字行被边框吃掉
        render::render_map(&mut buf, area, &self.doc, &self.view, 1.0);
        let body = crate::svg::of_buffer(&buf);
        // .png 交给 rsvg-convert 栅格化(终端自己画不了字),其余按文本落盘
        if path.to_ascii_lowercase().ends_with(".png") {
            let res =
                crate::paths::ensure_parent(path).and_then(|()| crate::svg::write_png(&body, path));
            self.note_export(path, res);
        } else {
            self.write_export(path, &body);
        }
    }

    /// :export <path>.dot:best-effort 导出,把警告放在回显位(最多三条)
    fn export_dot(&mut self, path: &str) {
        let path = &crate::paths::expand(path);
        match crate::dot_write::export(&self.doc) {
            Ok((body, warnings)) => {
                let ok = self.write_export(path, &body);
                if ok && !warnings.is_empty() {
                    let shown: Vec<&str> = warnings.iter().take(3).map(|w| w.as_str()).collect();
                    let mut text = shown.join("; ");
                    if warnings.len() > shown.len() {
                        text.push_str("; ...");
                    }
                    self.view.note = Some(self.view.lang.pick_owned(
                        format!("已导出到 {path}(警告:{text})"),
                        format!("exported to {path} (warnings: {text})"),
                    ));
                }
            }
            Err(e) => self.view.note = Some(e),
        }
    }

    /// 导出结果说一句(成功 / 失败原因)
    fn note_export(&mut self, path: &str, res: Result<(), String>) {
        self.view.note = Some(match res {
            Ok(()) => self
                .view
                .lang
                .pick_owned(format!("已导出到 {path}"), format!("exported to {path}")),
            Err(e) => e,
        });
    }

    /// :export-canvas:交给 canvas_io 出 JSON Canvas(它会带回表达不了的警告)
    fn export_canvas(&mut self, path: &str) {
        let path = &crate::paths::expand(path);
        let (body, warnings) = crate::canvas_io::export(&self.doc);
        if self.write_export(path, &body) {
            self.note_export_warnings(path, &warnings);
        }
    }

    /// 导出成功后把警告叠在回显上(最多三条,多的说一句"还有 N 条")
    fn note_export_warnings(&mut self, path: &str, warnings: &[String]) {
        if warnings.is_empty() {
            return;
        }
        let mut text = String::new();
        for w in warnings.iter().take(3) {
            text.push_str(w);
            text.push(' ');
        }
        if warnings.len() > 3 {
            text.push_str(&self.view.lang.pick_owned(
                format!("…还有 {} 条", warnings.len() - 3),
                format!("…+{} more", warnings.len() - 3),
            ));
        }
        let note = self.view.lang.pick_owned(
            format!("已导出到 {path};{text}"),
            format!("exported to {path}; {text}"),
        );
        self.view.note = Some(note);
    }

    /// 导出的落盘:和存盘一样先写 .tmp 再改名,失败把原因说在回显位.
    /// 返回是否写成功(调用方要在成功时再叠加警告).
    fn write_export(&mut self, path: &str, body: &str) -> bool {
        if let Err(e) = crate::paths::ensure_parent(path) {
            self.view.note = Some(e);
            return false;
        }
        let tmp = format!("{path}.tmp");
        let res = std::fs::write(&tmp, body.as_bytes()).and_then(|()| std::fs::rename(&tmp, path));
        match res {
            Ok(()) => {
                self.view.note = Some(
                    self.view
                        .lang
                        .pick_owned(format!("已导出到 {path}"), format!("exported to {path}")),
                );
                true
            }
            Err(err) => {
                let _ = std::fs::remove_file(&tmp);
                self.view.note = Some(self.view.lang.pick_owned(
                    format!("导出失败 {path}: {err}"),
                    format!("cannot export {path}: {err}"),
                ));
                false
            }
        }
    }

    /// "没选边"的提示(style / edgecolor / via 都要用)
    fn note_no_edge(&mut self) {
        let note = self.view.lang.pick_owned(
            "没有选中边(先按 E 选一条)".to_string(),
            "no edge selected (press E to pick one)".to_string(),
        );
        self.view.note = Some(note);
    }

    /// E:在跟光标框相连的边里按下标顺序循环下一条
    /// E:只亮光标节点的邻居与相连的边,其余压暗;再按 E 或 Esc 退出
    fn toggle_ego(&mut self) {
        // 开 / 关:开着就是"只亮光标所在节点的邻居",换节点(点击或挪光标)跟着走
        if self.view.ego.is_some() {
            self.view.ego = None;
            let note = self
                .view
                .lang
                .pick_owned("退出邻居高亮".to_string(), "neighbour view off".to_string());
            self.view.note = Some(note);
            return;
        }
        let Some(n) = self.doc.box_at_cell(self.view.cursor) else {
            let note = self.view.lang.pick_owned(
                "光标下没有节点".to_string(),
                "no node under the cursor".to_string(),
            );
            self.view.note = Some(note);
            return;
        };
        // 注释框:亮它绑的那条边的两头
        let n = match self.doc.edge_of_box(n) {
            Some(e) => self.doc.edge_ends(e).map(|(a, _)| a).unwrap_or(n),
            None => n,
        };
        self.view.ego = Some(n);
        let edges = self.doc.adj(n).len();
        let mut neighbours: Vec<usize> = Vec::new();
        for &e in self.doc.adj(n) {
            if let Some((a, b)) = self.doc.edge_ends(e) {
                let other = if a == n { b } else { a };
                if !neighbours.contains(&other) {
                    neighbours.push(other);
                }
            }
        }
        let name = label_of(&self.doc, n).to_string();
        // 相关的注释框有几条(开关关着时它们会被临时露出来)
        let notes = self
            .doc
            .adj(n)
            .iter()
            .filter(|e| self.doc.note_of_edge(&self.doc.edges[**e].id).is_some())
            .count();
        let note = self.view.lang.pick_owned(
            format!(
                "邻居高亮:{name}({} 个邻居 / {edges} 条边 / {notes} 条注释),E 或 Esc 退出",
                neighbours.len()
            ),
            format!(
                "neighbours of {name} ({} / {edges} edges / {notes} notes), E or Esc to exit",
                neighbours.len()
            ),
        );
        self.view.note = Some(note);
    }

    fn cycle_edge(&mut self) {
        self.sync_hit();
        // 完全重叠堆在一起时,先在最上面的框之间轮(E 也能选中被压住的那些)
        let stack = self.doc.boxes_at_cell(self.view.cursor);
        if stack.len() > 1 {
            // 从"当前选中的那个"往下轮:文档改过时 cand 会被清零,靠它记不住位置
            let pos = stack
                .iter()
                .position(|&i| self.view.pick == render::Pick::Box(i));
            let next = pos.map(|p| (p + 1) % stack.len()).unwrap_or(0);
            self.view.cand = next;
            let i = stack[next];
            self.view.pick = render::Pick::Box(i);
            let name = label_of(&self.doc, i).to_string();
            self.view.note = Some(self.view.lang.pick_owned(
                format!("框 {name}(重叠 {}/{})", self.view.cand + 1, stack.len()),
                format!("box {name} (stack {}/{})", self.view.cand + 1, stack.len()),
            ));
            return;
        }
        // 光标压在节点上:只在这个节点与邻居之间的边里转(不从这儿路过的线)
        if let Some(c) = self.doc.box_at_cell(self.view.cursor) {
            self.cycle_at_node(c);
            return;
        }
        // 光标不在节点上(在线上/空白处):按这个格子上的边候选转.
        // 命中表按**屏幕**格记,光标存的是画布格,得先投影过去,不然视图一挪就选不中
        let p = render::panels(&self.doc, self.area, &self.view);
        let screen = p
            .proj
            .cell((self.view.cursor.0 as f64, self.view.cursor.1 as f64));
        let cands: Vec<usize> = self.usable_hit().candidates_at(screen).to_vec();
        if cands.is_empty() {
            let note = self.view.lang.pick_owned(
                "光标下没有可选的边".to_string(),
                "nothing to select here".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        self.view.cand = (self.view.cand + 1) % cands.len();
        let e = cands[self.view.cand];
        self.view.pick = render::Pick::Edge(e);
        self.note_edge(e);
    }

    /// 在节点 c 的相连边里轮(E):按文档顺序,到头绕回去
    fn cycle_at_node(&mut self, c: usize) {
        let incident: Vec<usize> = self.doc.adj(c).to_vec();
        let Some(&first) = incident.first() else {
            self.view.pick = render::Pick::None;
            let note = self.view.lang.pick_owned(
                "这个框没有相连的边".to_string(),
                "this box has no edges".to_string(),
            );
            self.view.note = Some(note);
            return;
        };
        let next = match self.view.pick.edge() {
            Some(cur) => incident.iter().copied().find(|&e| e > cur).unwrap_or(first),
            None => first,
        };
        self.view.pick = render::Pick::Edge(next);
        self.note_edge(next);
    }

    /// 选中边之后说一句
    fn note_edge(&mut self, e: usize) {
        let (a, b) = match self.doc.edge_ends(e) {
            Some((a, b)) => (label_of(&self.doc, a), label_of(&self.doc, b)),
            None => (String::new(), String::new()),
        };
        let note = self
            .doc
            .note_of_edge(&self.doc.edges[e].id)
            .and_then(|b| self.doc.text(b).map(|t| t.text.clone()))
            .unwrap_or_default();
        self.view.note = Some(self.view.lang.pick_owned(
            if note.is_empty() {
                format!("选边 {a} - {b} (#{e}) -> 无注释")
            } else {
                format!("选边 {a} - {b} (#{e}) -> 注释: {note}")
            },
            if note.is_empty() {
                format!("edge {a} - {b} (#{e}) -> no note")
            } else {
                format!("edge {a} - {b} (#{e}) -> note: {note}")
            },
        ));
    }

    /// 把 View 里属于文档的状态写回 doc.view(存盘前必须做)
    fn sync_view(&mut self) {
        let center = self.view.center.unwrap_or_else(|| bbox_center(&self.doc));
        let cursor = Some(crate::document::Cursor::Cell(self.view.cursor));
        let focus: Vec<String> = self
            .view
            .focus
            .iter()
            .filter_map(|&g| self.doc.groups.get(g).map(|x| x.id.clone()))
            .collect();
        // 文件里的 view.layer 必须引用已存在的图层,否则下次打不开
        let layer = self
            .view
            .layer
            .clone()
            .filter(|l| self.doc.layers.iter().any(|x| &x.name == l));
        let view = ViewState {
            center,
            zoom: self.view.zoom,
            layout: self.view.layout.name().to_string(),
            cursor,
            focus,
            labels: self.view.labels,
            lang: self.view.lang.code().to_string(),
            layer,
            side: self.view.side,
            cycle: self.view.cycle,
            orientation: self.view.orientation.name().to_string(),
            notes: self.view.notes,
        };
        self.doc.view = view;
    }

    /// 把当前文档写到 path:先 .tmp 再 rename;写前校验,失败不覆盖原文件
    pub fn save_document(&mut self, path: &str) -> Result<(), String> {
        let path = &crate::paths::expand(path);
        // 存档头里的来源标记:写的是谁保存的,不是导入自哪种格式
        self.doc.source = format!("graph-tui {}", env!("CARGO_PKG_VERSION"));
        self.sync_view();
        self.doc.validate()?;
        let body = self.doc.save();
        crate::paths::write_atomic(path, &body)
    }

    /// 默认的 native 存档路径:`.gtui` 写回原处,`.dot`/`.canvas` 写同目录 <stem>.gtui.
    /// stdin 来源没有路径,返回 None(要求显式给一个).
    fn default_save_path(&self) -> Option<String> {
        let src = self.source.as_deref()?;
        if src.to_ascii_lowercase().ends_with(".gtui") {
            Some(src.to_string())
        } else {
            Some(
                Path::new(src)
                    .with_extension("gtui")
                    .to_string_lossy()
                    .into_owned(),
            )
        }
    }

    /// 换来源文件的同时记下磁盘标记:后面 `check_disk` 拿它比,判断有没有被外部改过
    pub fn set_source(&mut self, path: Option<String>) {
        self.src_stamp = path.as_deref().and_then(FileStamp::of);
        self.src_rev = self.rev;
        self.view.stale = false;
        self.reload_armed = None;
        self.source = path;
    }

    /// 看一眼源文件还在不在、有没有被别的程序改过(main 的轮询里每次超时都调,开销一次 stat).
    /// 返回 true 表示画面该重画:标了 stale,或者已经自动重新载入了
    pub fn check_disk(&mut self) -> bool {
        let Some(path) = self.source.clone() else {
            return false;
        };
        let now = FileStamp::of(&path);
        if now == self.src_stamp {
            return false;
        }
        // 第一次记标记(比如 `--file` 用绝对路径打开):以当前状态为准,不算改动
        if self.src_stamp.is_none() {
            self.src_stamp = now;
            self.src_rev = self.rev;
            return false;
        }
        // 本地没动过:直接换掉,和 nvim 的 autoread 一样省一步.
        // 载入失败(文件正在写 / 内容坏了)就退回"只提醒",别每 250ms 重试一次
        if !self.view.stale && self.rev == self.src_rev && now.is_some() {
            if self.reload_now() {
                self.view.note = Some(self.view.lang.pick_owned(
                    "文件已在磁盘上更新,已重新载入".to_string(),
                    "the file changed on disk; reloaded".to_string(),
                ));
            } else {
                self.view.stale = true;
            }
            return true;
        }
        if !self.view.stale {
            self.view.stale = true;
            let gone = now.is_none();
            self.view.note = Some(crate::render::stale_note(&self.view, gone));
            return true;
        }
        false
    }

    /// 从磁盘重新读一遍源文件(丢掉本地改动).返回是否成功
    fn reload_now(&mut self) -> bool {
        if let Some(path) = self.source.as_deref() {
            // 新文件还没落盘:别拿"空文档"把它盖掉
            if !std::path::Path::new(path).exists() {
                self.view.note = Some(self.view.lang.pick_owned(
                    format!("{path} 还不存在:先 :w 保存"),
                    format!("{path} does not exist yet: save it with :w"),
                ));
                self.src_stamp = None;
                return false;
            }
        }
        let Some(path) = self.source.clone() else {
            self.view.note = Some(self.view.lang.pick_owned(
                "没有来源文件,无从载入".to_string(),
                "no source file to reload from".to_string(),
            ));
            return false;
        };
        match Self::read_doc(&path) {
            Ok((doc, _)) => {
                self.doc = doc;
                self.view_from_doc();
                self.undo.clear();
                self.rev += 1;
                self.src_stamp = FileStamp::of(&path);
                self.src_rev = self.rev;
                self.view.stale = false;
                self.reload_armed = None;
                let note = self
                    .view
                    .lang
                    .pick_owned(format!("已重新载入 {path}"), format!("reloaded {path}"));
                self.after_history(&note);
                true
            }
            Err(e) => {
                self.view.note = Some(
                    self.view
                        .lang
                        .pick_owned(format!("重新载入失败: {e}"), format!("cannot reload: {e}")),
                );
                false
            }
        }
    }

    /// R:从磁盘重新载入.本地有改动时要按两次(第一次只是提醒)
    fn reload_step(&mut self) {
        if self.source.is_none() {
            self.view.note = Some(self.view.lang.pick_owned(
                "没有来源文件,无从载入".to_string(),
                "no source file to reload from".to_string(),
            ));
            return;
        }
        let dirty = self.rev != self.src_rev;
        let armed = self
            .reload_armed
            .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(4));
        if dirty && !armed {
            self.reload_armed = Some(std::time::Instant::now());
            self.view.note = Some(self.view.lang.pick_owned(
                "有没保存的改动:再按一次 R 丢弃本地改动并重新载入".to_string(),
                "unsaved changes: press R again to drop them and reload".to_string(),
            ));
            return;
        }
        self.reload_now();
    }

    /// 按扩展名把文本解析成文档(:e / :e! / R 共用).
    /// 和 nvim 一样:`.gtui` 还不存在就当"新文件"——空白文档 + 这个名字,`:w` 才落盘.
    /// `.dot` / `.canvas` 只是导入源,编不出来,缺了照旧报错.
    /// 返回的第二项表示"这是还没写过盘的新文件"
    fn read_doc(path: &str) -> Result<(Document, bool), String> {
        let lower = path.to_ascii_lowercase();
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && lower.ends_with(".gtui") => {
                return Ok((Document::scratch(), true));
            }
            Err(e) => return Err(format!("cannot read {path}: {e}")),
        };
        let doc = if lower.ends_with(".gtui") {
            Document::load(&text)
        } else if lower.ends_with(".canvas") {
            crate::canvas_io::import(&text)
        } else {
            crate::dot::parse_document(&text, ViewState::default(), "dot")
        }?;
        Ok((doc, false))
    }

    /// 存盘:只写 .gtui.显式路径也必须是 .gtui,绝不写回 .dot.
    /// 返回 true 表示真的写成功了(`:wq` 要靠它决定退不退)
    fn save(&mut self, path: Option<&str>) -> bool {
        let target = match path {
            Some(p) => {
                let p = p.trim();
                if p.is_empty() {
                    self.note_save_hint();
                    return false;
                }
                if !p.to_ascii_lowercase().ends_with(".gtui") {
                    let note = self.view.lang.pick_owned(
                        format!("只写 .gtui:不写 {p}"),
                        format!("only .gtui is written: refusing {p}"),
                    );
                    self.view.note = Some(note);
                    return false;
                }
                p.to_string()
            }
            None => match self.default_save_path() {
                Some(p) => p,
                None => {
                    self.note_save_hint();
                    return false;
                }
            },
        };
        // 存下的来源路径要展开过的,不然下次 :w(不给路径)还会拿到 `~`
        let target = crate::paths::expand(&target);
        match self.save_document(&target) {
            Ok(()) => {
                self.set_source(Some(target.clone()));
                self.view.note = Some(
                    self.view
                        .lang
                        .pick_owned(format!("已保存到 {target}"), format!("saved to {target}")),
                );
                true
            }
            Err(err) => {
                self.view.note = Some(self.view.lang.pick_owned(
                    format!("保存失败 {target}: {err}"),
                    format!("cannot save {target}: {err}"),
                ));
                false
            }
        }
    }

    /// 还没有文件名时(空白缓冲 / stdin)存盘要说一句:要一个显式路径
    fn note_save_hint(&mut self) {
        let note = self.view.lang.pick_owned(
            "还没有文件名:用 :w <路径.gtui>(或 :wq <路径.gtui> 存完就退)".to_string(),
            "no file name yet: use :w <path.gtui> (or :wq <path.gtui> to save and quit)"
                .to_string(),
        );
        self.view.note = Some(note);
    }

    /// :e <path>:按扩展名打开一份文档;失败时保持当前文档不动
    fn open_doc(&mut self, path: &str) {
        let path = &crate::paths::expand(path);
        match Self::read_doc(path) {
            Ok((doc, fresh)) => {
                self.doc = doc;
                self.set_source(Some(path.to_string()));
                self.view_from_doc();
                self.reset_view();
                // 换了文档,撤销历史不再适用
                self.undo.clear();
                if !self.doc.boxes.is_empty() {
                    self.note_here();
                }
                self.view.note = Some(if fresh {
                    self.view.lang.pick_owned(
                        format!("新文件 {path}(:w 写入)"),
                        format!("new file {path} (:w to write it)"),
                    )
                } else {
                    self.view
                        .lang
                        .pick_owned(format!("已打开 {path}"), format!("opened {path}"))
                });
            }
            Err(e) => self.view.note = Some(e),
        }
    }

    /// :e 不带参数:把当前目录与源文件目录里的可编辑文件列在回显位(最多 8 个)
    /// 当前目录 + 数据来源所在目录里所有能打开的文档
    fn collect_candidates(&self) -> Vec<String> {
        let mut dirs: Vec<std::path::PathBuf> = vec![std::path::PathBuf::from(".")];
        if let Some(src) = &self.source {
            if let Some(parent) = std::path::Path::new(src).parent() {
                dirs.push(parent.to_path_buf());
            }
        }
        let mut seen: std::collections::HashSet<std::path::PathBuf> =
            std::collections::HashSet::new();
        let mut found: Vec<String> = Vec::new();
        for d in dirs {
            let Ok(rd) = std::fs::read_dir(&d) else {
                continue;
            };
            for entry in rd.flatten() {
                let p = entry.path();
                let lower = p.to_string_lossy().to_ascii_lowercase();
                if !(lower.ends_with(".gtui")
                    || lower.ends_with(".dot")
                    || lower.ends_with(".canvas"))
                {
                    continue;
                }
                let key = std::fs::canonicalize(&p).unwrap_or_else(|_| p.clone());
                if !seen.insert(key) {
                    continue;
                }
                found.push(p.to_string_lossy().into_owned());
            }
        }
        found.sort();
        found
    }

    /// `:e` 不带参数:弹出选择浮层(过滤 + 上下选 + 回车打开)
    fn open_picker(&mut self) {
        let items = self.collect_candidates();
        if items.is_empty() {
            let note = self.view.lang.pick_owned(
                "这个目录里没有可编辑的文件(.gtui/.dot/.canvas)".to_string(),
                "no editable file here (.gtui/.dot/.canvas)".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        self.view.file_pick = Some(FilePick {
            items,
            at: 0,
            filter: String::new(),
        });
    }

    /// 选择浮层里的按键:过滤、上下选、回车打开、esc 取消.
    /// 只有方向键能挪(字母全部归过滤词,不然文件名里的 j/k 会被当成导航)
    fn handle_pick(&mut self, code: KeyCode) {
        let Some(mut pick) = self.view.file_pick.take() else {
            return;
        };
        match code {
            KeyCode::Esc => return,
            KeyCode::Enter => {
                if let Some(p) = pick.current().map(|s| s.to_string()) {
                    self.open_doc(&p);
                }
                return;
            }
            KeyCode::Up => {
                let n = pick.shown().len();
                if n > 0 {
                    pick.at = (pick.at + n - 1) % n;
                }
            }
            KeyCode::Down => {
                let n = pick.shown().len();
                if n > 0 {
                    pick.at = (pick.at + 1) % n;
                }
            }
            KeyCode::Backspace => {
                pick.filter.pop();
                pick.at = 0;
            }
            KeyCode::Char(c) => {
                pick.filter.push(c);
                pick.at = 0;
            }
            _ => {}
        }
        self.view.file_pick = Some(pick);
    }

    /// 换布局:调 layout::apply 写回整数单元格坐标.任意缩放下都可执行;
    /// 一直保持 1:1.
    fn apply_layout(&mut self, l: Layout) {
        self.begin_edit("换布局");
        match layout::apply_with(&mut self.doc, l, self.spacing) {
            Ok(report) => {
                let k = self.layout_quarters(l);
                if k != 0 {
                    self.doc.rotate_quarters(k);
                }
                self.view.angle = self.view.orientation.degrees();
                self.view.layout = l;
                self.view.pan = None;
                self.view.target = None;
                self.note_layout(l, &report);
            }
            Err(e) => self.view.note = Some(e),
        }
        self.end_edit();
    }

    /// 换布局后把名字说在提示行上;布局报告里有告警就把前三条接在后面
    fn note_layout(&mut self, l: Layout, report: &layout::LayoutReport) {
        // auto:把实际选中的布局一起报出来(auto(tree) / auto(layered))
        let n = if l == Layout::Auto && !report.profile.is_empty() {
            format!("auto({})", report.profile)
        } else {
            l.name().to_string()
        };
        let mut note = self
            .view
            .lang
            .pick_owned(format!("布局 = {n}"), format!("layout = {n}"));
        let _ = &n;
        if !report.warnings.is_empty() {
            note.push_str("  (");
            note.push_str(&report.summary(3));
            note.push(')');
        }
        self.view.note = Some(note);
    }

    /// r:轮到下一套布局
    fn cycle_layout(&mut self) {
        let l = self.view.layout.next();
        self.apply_layout(l);
    }

    /// ":" 输入行
    /// `:` 的补全候选:还没输参数就补动词,已经写了动词就补它的取值
    fn completions(&self, line: &str) -> Vec<String> {
        // 还没打空格:这一整个词就是动词前缀
        if !line.contains(' ') {
            let low = line.to_lowercase();
            return VERBS
                .iter()
                .map(|v| v.to_string())
                .filter(|c| c.to_lowercase().starts_with(&low))
                .collect();
        }
        // 打了空格:参数是"动词之后的整行"(路径里可以有空格),不按空格切词.
        // 路径类动词按真实目录补,只有按 Tab 时才会扫盘
        let (verb, rest) = line.split_once(' ').unwrap_or((line, ""));
        let path_exts: Option<&[&str]> = match verb {
            "e" => Some(&["gtui", "dot", "canvas"]),
            "w" | "save" | "export" | "export-canvas" => Some(&[]),
            _ => None,
        };
        if let Some(exts) = path_exts {
            return paths::complete_path(rest, exts);
        }
        let prefix = rest.trim();
        let pick = |all: &[String]| -> Vec<String> {
            all.iter()
                .filter(|c| c.to_lowercase().starts_with(&prefix.to_lowercase()))
                .cloned()
                .collect()
        };
        let names = |doc: &Document| -> Vec<String> {
            let mut out = Vec::new();
            for b in &doc.boxes {
                out.push(b.id.clone());
                let label = label_of(doc, doc.box_idx(&b.id).unwrap_or(0));
                if !label.is_empty() && !out.contains(&label) {
                    out.push(label);
                }
            }
            out
        };
        let values: Vec<String> = match verb {
            "box" => ["point", "none", "single", "rounded", "double", "heavy"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            "style" => ["solid", "dashed", "dotted"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            "dir" => ["none", "forward", "back", "both"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            "corner" => ["rounded", "sharp"].iter().map(|s| s.to_string()).collect(),
            "head" => ["auto", "ascii", "arrows", "triangles"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            "textalign" => ["left", "center", "right"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            "layout" => ["auto", "geo", "relax", "force", "layered", "tree", "chain"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            "lang" => ["zh", "en"].iter().map(|s| s.to_string()).collect(),
            "notes" => ["on", "off"].iter().map(|s| s.to_string()).collect(),
            "rotate" => ["auto", "0", "90", "180", "270"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            "cycle" => vec!["auto".to_string()],
            "layer" => std::iter::once("all".to_string())
                .chain(self.doc.layer_names())
                .collect(),
            "tolayer" => self.doc.layer_names(),
            "link" => names(&self.doc),
            _ => Vec::new(),
        };
        pick(&values)
    }

    /// Tab:用下一/上一个候选替换当前这个词(第一次先把候选表定下来,之后在原地轮换)
    fn complete_cmd(&mut self, back: bool) {
        let Some(line) = self.view.cmd.clone() else {
            return;
        };
        // `:w` / `:w ` 这类空参数按 Tab:把默认存盘路径整段填进去(还能接着改)
        if !back {
            for verb in ["w", "wq", "save"] {
                if line != verb && line != format!("{verb} ") {
                    continue;
                }
                let Some(default) = self.default_save_path() else {
                    self.view.note = Some(self.view.lang.pick_owned(
                        "没有默认路径:敲一个".to_string(),
                        "no default path; type one".to_string(),
                    ));
                    return;
                };
                self.view.cmd = Some(format!("{verb} {}", paths::absolute_display(&default)));
                self.view.cmd_pos = self
                    .view
                    .cmd
                    .as_ref()
                    .map(|c| c.chars().count())
                    .unwrap_or(0);
                self.view.cmd_menu = None;
                self.refresh_cmd_hint();
                return;
            }
        }
        if self.view.cmd_menu.is_none() {
            let items = self.completions(&line);
            if items.is_empty() {
                return;
            }
            self.view.cmd_menu = Some((items, 0));
        }
        let Some((items, at)) = self.view.cmd_menu.clone() else {
            return;
        };
        if items.is_empty() {
            return;
        }
        let n = items.len();
        // at 是"下一个要插的";往回退就是回到当前这个的再前一个
        let idx = if back {
            if at == 0 {
                n - 1
            } else {
                (at + n - 2) % n
            }
        } else {
            at % n
        };
        let chosen = items[idx].clone();
        // 参数是"动词之后的整行"(路径里可以有空格),所以整段替掉,
        // 不能按最后一个空格切
        let new_line = match line.split_once(' ') {
            Some((verb, _)) => format!("{verb} {chosen}"),
            None => chosen.clone(),
        };
        self.view.cmd_pos = new_line.chars().count();
        self.view.cmd = Some(new_line);
        // 参数里可能带空格(文件名),补完就不用再加空格
        self.view.cmd_menu = Some((items, (idx + 1) % n));
        self.refresh_cmd_hint();
    }

    /// 这批框的文字(都一样才拿来预填,否则空)
    /// 进入就地编辑:草稿从框里现有的文字开始,光标落在画布光标指到的那个字符上
    /// (光标不在文字里时落到末尾)
    fn begin_box_edit(&mut self, i: usize, created: bool) {
        if i >= self.doc.boxes.len() {
            return;
        }
        if self.doc.boxes[i].style.is_point() {
            // 点节点只有一个名字,就地编辑没有意义:退回输入框那条路
            self.open_prompt(Prompt::Text(vec![i]), self.common_text(&[i]));
            return;
        }
        let text = self.doc.text(i).map(|t| t.text.clone()).unwrap_or_default();
        let caret = self
            .caret_from_cursor(i)
            .unwrap_or_else(|| text.chars().count());
        self.view.edit = Some(BoxEdit {
            box_idx: i,
            created,
            text,
            caret,
        });
        self.note_edit_help();
        self.sync_edit_cursor();
    }

    /// 画布光标落在框里第几个字符上(不在文字区域就是 None)
    fn caret_from_cursor(&self, i: usize) -> Option<usize> {
        let b = self.doc.boxes.get(i)?;
        let t = self.doc.text(i)?;
        let origin = if b.style.is_boxed() {
            (b.at.0 + 1, b.at.1 + 1)
        } else {
            t.at
        };
        let (col, row) = (self.view.cursor.0 - origin.0, self.view.cursor.1 - origin.1);
        if col < 0 || row < 0 {
            return None;
        }
        let lines: Vec<&str> = t.text.split('\n').collect();
        let row = row as usize;
        if row >= lines.len() {
            return None;
        }
        let col = (col as usize).min(lines[row].chars().count());
        let mut at = 0usize;
        for line in lines.iter().take(row) {
            at += line.chars().count() + 1;
        }
        Some(at + col)
    }

    fn note_edit_help(&mut self) {
        let note = self.view.lang.pick_owned(
            "改字:Enter 换行,Esc 提交,Ctrl-C 取消".to_string(),
            "editing: Enter newline, Esc commits, Ctrl-C cancels".to_string(),
        );
        self.view.note = Some(note);
    }

    /// 编辑态下的按键:只动草稿,不动文档
    fn handle_edit_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        let ctrl = modifiers.contains(KeyModifiers::CONTROL);
        let alt = modifiers.contains(KeyModifiers::ALT);
        let Some(mut edit) = self.view.edit.take() else {
            return;
        };
        let len = edit.text.chars().count();
        let mut keep = true;
        match code {
            KeyCode::Esc => {
                self.view.edit = Some(edit);
                self.commit_box_edit(false);
                return;
            }
            KeyCode::Char('c') if ctrl => {
                self.view.edit = Some(edit);
                self.commit_box_edit(true);
                return;
            }
            KeyCode::Enter => {
                edit.text
                    .insert(Self::byte_of(&edit.text, edit.caret), '\n');
                edit.caret += 1;
            }
            KeyCode::Backspace => {
                if edit.caret > 0 {
                    let at = Self::byte_of(&edit.text, edit.caret - 1);
                    edit.text.remove(at);
                    edit.caret -= 1;
                }
            }
            KeyCode::Delete => {
                if edit.caret < len {
                    let at = Self::byte_of(&edit.text, edit.caret);
                    edit.text.remove(at);
                }
            }
            KeyCode::Home => edit.caret = Self::line_start(&edit.text, edit.caret),
            KeyCode::End => edit.caret = Self::line_end(&edit.text, edit.caret),
            KeyCode::Left if alt => edit.caret = word_left(&edit.text, edit.caret),
            KeyCode::Right if alt => edit.caret = word_right(&edit.text, edit.caret),
            KeyCode::Left => edit.caret = edit.caret.saturating_sub(1),
            KeyCode::Right => edit.caret = (edit.caret + 1).min(len),
            // 上下按"列"走,超出行尾就停在行尾
            KeyCode::Up => edit.caret = Self::caret_vertical(&edit.text, edit.caret, -1),
            KeyCode::Down => edit.caret = Self::caret_vertical(&edit.text, edit.caret, 1),
            KeyCode::Char(c) if !ctrl && !alt => {
                edit.text.insert(Self::byte_of(&edit.text, edit.caret), c);
                edit.caret += 1;
            }
            _ => keep = false,
        }
        if keep {
            self.view.edit = Some(edit);
            self.sync_edit_cursor();
        } else {
            self.view.edit = Some(edit);
        }
    }

    /// 第 idx 个字符的字节偏移(越界给末尾)
    fn byte_of(s: &str, idx: usize) -> usize {
        s.char_indices()
            .nth(idx)
            .map(|(b, _)| b)
            .unwrap_or_else(|| s.len())
    }

    fn line_start(s: &str, caret: usize) -> usize {
        let chars: Vec<char> = s.chars().take(caret).collect();
        chars.iter().rposition(|c| *c == '\n').map_or(0, |p| p + 1)
    }

    fn line_end(s: &str, caret: usize) -> usize {
        let mut idx = 0usize;
        for (i, c) in s.chars().enumerate() {
            if i < caret {
                continue;
            }
            if c == '\n' {
                return i;
            }
            idx = i + 1;
        }
        idx.max(caret.min(s.chars().count()))
    }

    /// 上下挪一行,尽量保住列号
    fn caret_vertical(s: &str, caret: usize, step: i32) -> usize {
        let lines: Vec<&str> = s.split('\n').collect();
        let mut row = 0usize;
        let mut col = 0usize;
        for (i, c) in s.chars().enumerate() {
            if i == caret {
                break;
            }
            if c == '\n' {
                row += 1;
                col = 0;
            } else {
                col += 1;
            }
        }
        let target = (row as i32 + step).clamp(0, lines.len() as i32 - 1) as usize;
        let mut want = col;
        let mut at = 0usize;
        for (i, line) in lines.iter().enumerate() {
            if i == target {
                want = want.min(line.chars().count());
                return at + want;
            }
            at += line.chars().count() + 1;
        }
        caret
    }

    /// 编辑时让画布光标跟着草稿里的光标走(状态行显示 x,y)
    fn sync_edit_cursor(&mut self) {
        let Some(edit) = self.view.edit.clone() else {
            return;
        };
        let Some(t) = self.doc.text(edit.box_idx) else {
            return;
        };
        let bare = !self.doc.boxes[edit.box_idx].style.is_boxed();
        let origin = if bare {
            (t.at.0, t.at.1)
        } else {
            (
                self.doc.boxes[edit.box_idx].at.0 + 1,
                self.doc.boxes[edit.box_idx].at.1 + 1,
            )
        };
        let (row, col) = Self::caret_row_col(&edit.text, edit.caret);
        self.view.cursor = (origin.0 + col as i32, origin.1 + row as i32);
        self.refresh_pick();
    }

    fn caret_row_col(s: &str, caret: usize) -> (usize, usize) {
        let (mut row, mut col) = (0usize, 0usize);
        for (i, c) in s.chars().enumerate() {
            if i == caret {
                break;
            }
            if c == '\n' {
                row += 1;
                col = 0;
            } else {
                col += 1;
            }
        }
        (row, col)
    }

    /// 退出编辑:提交(一步撤销)或丢掉草稿
    fn commit_box_edit(&mut self, cancel: bool) {
        let Some(edit) = self.view.edit.take() else {
            return;
        };
        if cancel {
            if edit.created {
                // 这个框是这次按 i 建出来的:整步撤掉,不留空框
                self.undo_step();
            }
            let note = self
                .view
                .lang
                .pick_owned("改字已取消".to_string(), "edit cancelled".to_string());
            self.view.note = Some(note);
            return;
        }
        let old = self
            .doc
            .text(edit.box_idx)
            .map(|t| t.text.clone())
            .unwrap_or_default();
        if old == edit.text {
            // 刚建的空框、什么都没打:撤掉它,别留下一个看不见的空节点
            if edit.created && edit.text.is_empty() {
                self.undo_step();
            }
            let note = self
                .view
                .lang
                .pick_owned("文字没变".to_string(), "text unchanged".to_string());
            self.view.note = Some(note);
            return;
        }
        // 和 :text 同一条路:单个普通框里的 `<|new box|>` 会拆成几个框
        let text = edit.text.clone();
        let ids = vec![edit.box_idx];
        self.cmd_set_text(&ids, &text);
        if self.view.note.is_none() {
            let note = self
                .view
                .lang
                .pick_owned("文字已更新".to_string(), "text updated".to_string());
            self.view.note = Some(note);
        }
    }

    /// Ctrl+G:$EDITOR 直接改选中/光标框的文字,编辑器一退就写回去.
    /// 不弹底行输入框,所以没有"先出现在命令行、再按 Enter"那一步
    fn edit_text_external(&mut self) {
        let ids = self.targets().boxes;
        if ids.is_empty() {
            let note = self.view.lang.pick_owned(
                "光标下没有可编辑的框".to_string(),
                "no box under the cursor to edit".to_string(),
            );
            self.view.note = Some(note);
            return;
        }
        if self.blocked_by_selection() {
            return;
        }
        let text = self.common_text(&ids);
        self.edit_request = Some((EditTarget::Boxes(ids), text));
    }

    /// 底行输入框的提交:Enter 与"外部编辑器回来"走同一条路
    fn submit_prompt(&mut self, what: Prompt, text: String) {
        match what {
            Prompt::NewBox => self.cmd_new(&text),
            Prompt::Text(ids) => self.cmd_set_text(&ids, &text),
            Prompt::Link(_) => self.cmd_link(&text),
        }
        // 注释框还没写字:删掉,不留下看不见的空框
        self.drop_note_draft();
    }

    fn common_text(&self, ids: &[usize]) -> String {
        let mut it = ids
            .iter()
            .filter_map(|&i| self.doc.text(i).map(|t| t.text.clone()));
        let first = it.next().unwrap_or_default();
        if it.all(|t| t == first) {
            first
        } else {
            String::new()
        }
    }

    /// 打开底行输入框,光标停在末尾
    fn open_prompt(&mut self, what: Prompt, text: String) {
        self.view.prompt_pos = text.chars().count();
        self.view.prompt = Some((what, text));
    }

    /// 底行提示:回车执行,esc 取消,其余按键按行编辑规则改缓冲.
    /// 光标在 `view.prompt_pos`(按字符数),支持 emacs 基本键和按词跳
    fn handle_prompt(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        let ctrl = modifiers.contains(KeyModifiers::CONTROL);
        let alt = modifiers.contains(KeyModifiers::ALT);
        let (len, pos) = match &self.view.prompt {
            Some((_, t)) => {
                let n = t.chars().count();
                (n, self.view.prompt_pos.min(n))
            }
            None => (0, 0),
        };
        let mut want: Option<usize> = None;
        match code {
            KeyCode::Esc => {
                self.view.prompt = None;
                self.drop_note_draft();
            }
            KeyCode::Enter => {
                let Some((what, text)) = self.view.prompt.take() else {
                    return;
                };
                self.view.prompt_pos = 0;
                self.submit_prompt(what, text);
            }
            // Ctrl+G:整串交给 $VISUAL / $EDITOR(退出后读回来替换)
            KeyCode::Char('g') if ctrl => {
                if let Some((what, buf)) = &self.view.prompt {
                    self.edit_request = Some((EditTarget::Prompt(what.clone()), buf.clone()));
                }
            }
            // 行首/行尾
            KeyCode::Home => want = Some(0),
            KeyCode::End => want = Some(len),
            KeyCode::Char('a') if ctrl => want = Some(0),
            KeyCode::Char('e') if ctrl => want = Some(len),
            // 一词(Alt-左右 / Alt-B / Alt-F),要排在"一字"前面
            KeyCode::Left if alt => {
                want = self.view.prompt.as_ref().map(|(_, t)| word_left(t, pos))
            }
            KeyCode::Right if alt => {
                want = self.view.prompt.as_ref().map(|(_, t)| word_right(t, pos))
            }
            KeyCode::Char('b') if alt => {
                want = self.view.prompt.as_ref().map(|(_, t)| word_left(t, pos))
            }
            KeyCode::Char('f') if alt => {
                want = self.view.prompt.as_ref().map(|(_, t)| word_right(t, pos))
            }
            // 一字
            KeyCode::Left => want = Some(pos.saturating_sub(1)),
            KeyCode::Right => want = Some((pos + 1).min(len)),
            KeyCode::Char('b') if ctrl => want = Some(pos.saturating_sub(1)),
            KeyCode::Char('f') if ctrl => want = Some((pos + 1).min(len)),
            // Alt+退格:删前一个词(和 Ctrl-W 一样)
            KeyCode::Backspace if alt => {
                if let Some((_, t)) = self.view.prompt.as_mut() {
                    let from = word_left(t, pos);
                    let (a, b) = (char_byte(t, from), char_byte(t, pos));
                    t.replace_range(a..b, "");
                    want = Some(from);
                }
            }
            // 删
            KeyCode::Backspace => {
                if let Some((_, t)) = self.view.prompt.as_mut() {
                    let mut p = pos;
                    edit_backspace(t, &mut p);
                    want = Some(p);
                }
            }
            KeyCode::Delete => {
                if let Some((_, t)) = self.view.prompt.as_mut() {
                    edit_delete(t, pos);
                }
            }
            KeyCode::Char('d') if ctrl => {
                if let Some((_, t)) = self.view.prompt.as_mut() {
                    edit_delete(t, pos);
                }
            }
            KeyCode::Char('k') if ctrl => {
                if let Some((_, t)) = self.view.prompt.as_mut() {
                    let b = char_byte(t, pos);
                    t.truncate(b);
                }
                want = Some(pos);
            }
            KeyCode::Char('u') if ctrl => {
                if let Some((_, t)) = self.view.prompt.as_mut() {
                    let b = char_byte(t, pos);
                    t.replace_range(..b, "");
                }
                want = Some(0);
            }
            KeyCode::Char('w') if ctrl => {
                if let Some((_, t)) = self.view.prompt.as_mut() {
                    let from = word_left(t, pos);
                    let (a, b) = (char_byte(t, from), char_byte(t, pos));
                    t.replace_range(a..b, "");
                    want = Some(from);
                }
            }
            KeyCode::Char(c) if !ctrl && !alt => {
                if let Some((_, b)) = self.view.prompt.as_mut() {
                    let mut p = pos;
                    edit_insert(b, &mut p, &c.to_string());
                    want = Some(p);
                }
            }
            _ => {}
        }
        if let Some(p) = want {
            self.view.prompt_pos = p;
        }
    }

    /// 有没有等着跑外部编辑器的请求
    /// 光标下的链接目标(画布格 -> 目标)
    fn link_at_cursor(&self) -> Option<String> {
        self.hit.links.get(&self.view.cursor).cloned()
    }

    /// Enter / gf:有未确认选区先确认(老行为),否则打开光标下的链接
    fn open_link(&mut self) {
        if self.view.sel.is_some_and(|s| !s.confirmed) {
            self.confirm_selection();
            return;
        }
        let Some(target) = self.link_at_cursor() else {
            let note = self.view.lang.pick_owned(
                "光标下没有链接".to_string(),
                "no link under cursor".to_string(),
            );
            self.view.note = Some(note);
            return;
        };
        self.request_open(&target);
    }

    /// 解析目标并执行:图内跳转当场做,其余登记给 main(它才碰终端和进程)
    fn request_open(&mut self, raw: &str) {
        let opener = self.opener_command();
        let cwd = self.cwd_path();
        let doc_dir = self.doc_dir();
        let ctx = crate::target::OpenContext {
            doc_dir: doc_dir.as_deref(),
            cwd: &cwd,
            opener: &opener,
            links: &self.links,
        };
        match crate::target::plan_raw(&ctx, raw) {
            crate::target::OpenPlan::Jump { path, cell } => self.open_jump(path, cell),
            crate::target::OpenPlan::Note(msg) => self.note_error(msg),
            plan => self.open_request = Some(plan),
        }
    }

    /// 图内跳转:当前文档直接跳,已开着的切过去,没开的开新 tab 再跳.
    /// 不动文档,也不进撤销栈
    fn open_jump(&mut self, path: Option<std::path::PathBuf>, cell: Option<(i32, i32)>) {
        // 先记下"从哪儿跳走的":换 tab 之后再记就记成新 tab 的默认格了,
        // Ctrl-o 会回到新 tab 的 (0,0) 而不是原来的位置
        self.note_here();
        if let Some(p) = path.as_ref() {
            let s = p.display().to_string();
            match self.tab_of_path(&s) {
                Some(i) => self.switch_tab(i),
                None => {
                    self.new_tab(Some(&s));
                }
            }
        }
        if let Some(cell) = cell {
            self.view.cursor = cell;
            self.view.cand = 0;
            self.refresh_pick();
            self.follow_cursor_jump();
        }
    }

    /// 打开链接用哪条命令:配置 > `$OPENER` > 自动挑
    pub fn opener_command(&self) -> String {
        if let Some(o) = self.opener.as_deref().filter(|o| !o.is_empty()) {
            return o.to_string();
        }
        if let Some(o) = std::env::var("OPENER")
            .ok()
            .filter(|o| !o.trim().is_empty())
        {
            return o;
        }
        // auto:PATH 里有 opener 就用它(它会自己决定 edit/open)
        if which("opener").is_some() {
            return "opener".to_string();
        }
        if cfg!(target_os = "macos") {
            "open".to_string()
        } else if cfg!(windows) {
            "explorer".to_string()
        } else {
            "xdg-open".to_string()
        }
    }

    /// 状态行上的错误提示(打开链接失败之类)
    pub fn note_error(&mut self, msg: String) {
        self.view.note = Some(msg);
    }

    /// 当前文档所在目录:`./`、`../` 这类链接按它解析
    pub fn doc_dir(&self) -> Option<std::path::PathBuf> {
        self.source.as_deref().and_then(|p| {
            std::path::Path::new(&crate::paths::expand(p))
                .parent()
                .map(|d| d.to_path_buf())
        })
    }

    /// 进程的工作目录(没有文档路径时当基准)
    pub fn cwd_path(&self) -> std::path::PathBuf {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    }

    /// 在 graph-tui 里打开一个本地图档:新标签(已经开着就切过去)
    pub fn open_path(&mut self, path: &std::path::Path) {
        let s = path.display().to_string();
        self.new_tab(Some(&s));
    }

    /// main 取一次待执行的打开计划(取走即清);Jump 不会出现在这里
    pub fn take_open_request(&mut self) -> Option<crate::target::OpenPlan> {
        self.open_request.take()
    }

    pub fn edit_pending(&self) -> bool {
        self.edit_request.is_some()
    }

    /// 要交给外部编辑器的初始内容(不清请求:回填时还要用它对应的输入目标)
    pub fn edit_initial(&self) -> Option<String> {
        self.edit_request.as_ref().map(|(_, text)| text.clone())
    }

    /// 外部编辑器的结论:框里的字直接写进框,输入框内容直接提交,都是一步撤销
    pub fn apply_edit_result(&mut self, out: crate::external::Outcome) {
        use crate::external::Outcome;
        let Some((target, _initial)) = self.edit_request.take() else {
            return;
        };
        match target {
            EditTarget::Boxes(ids) => {
                let note = match out {
                    Outcome::Changed(text) => {
                        self.cmd_set_text(&ids, &text);
                        Some(self.view.lang.pick_owned(
                            "框里的文字已更新".to_string(),
                            "box text updated".to_string(),
                        ))
                    }
                    Outcome::Unchanged => Some(self.view.lang.pick_owned(
                        "编辑器没有改动,框里的文字保持原样".to_string(),
                        "editor changed nothing: the box text is unchanged".to_string(),
                    )),
                    Outcome::Failed(n) => Some(self.view.lang.pick_owned(
                        format!("编辑器退出码 {n},框里的文字保持原样"),
                        format!("editor exited {n}: the box text is unchanged"),
                    )),
                    Outcome::NoFile => Some(self.view.lang.pick_owned(
                        "编辑器删了临时文件,什么都没写".to_string(),
                        "the editor removed the file: nothing was written".to_string(),
                    )),
                    Outcome::BadEncoding => Some(self.view.lang.pick_owned(
                        "编辑器写回的不是 UTF-8,什么都没写".to_string(),
                        "the editor wrote non-UTF-8: nothing was written".to_string(),
                    )),
                    Outcome::BadText => Some(self.view.lang.pick_owned(
                        "写回的内容里有回车或 NUL,没有采用".to_string(),
                        "the editor wrote CR or NUL: nothing was written".to_string(),
                    )),
                    Outcome::Unsupported => Some(self.view.lang.pick_owned(
                        "这个平台没有 unix shell,Ctrl+G 外部编辑器用不了".to_string(),
                        "no Unix shell on this platform: Ctrl+G editor is unavailable".to_string(),
                    )),
                };
                self.view.note = note;
            }
            EditTarget::Prompt(what) => {
                let note = match out {
                    Outcome::Changed(text) => {
                        // 连边的目标名是单行语义:多行直接拒,原内容不动
                        if matches!(what, Prompt::Link(_)) && text.contains('\n') {
                            Some(
                                self.view.lang.pick_owned(
                                    "这个字段只能是一行,编辑器里的换行没有注入".to_string(),
                                    "this field is single-line: the newlines were not injected"
                                        .to_string(),
                                ),
                            )
                        } else {
                            // 输入框这一趟已经结束了:按 Enter 的方式直接提交
                            self.view.prompt = None;
                            self.view.prompt_pos = 0;
                            self.submit_prompt(what, text);
                            Some(self.view.lang.pick_owned(
                                "编辑器内容已应用".to_string(),
                                "editor content applied".to_string(),
                            ))
                        }
                    }
                    Outcome::Unchanged => Some(self.view.lang.pick_owned(
                        "编辑器没有改动,输入行保持原样".to_string(),
                        "editor changed nothing: the line is unchanged".to_string(),
                    )),
                    Outcome::Failed(n) => Some(self.view.lang.pick_owned(
                        format!("编辑器退出码 {n},输入行保持原样"),
                        format!("editor exited {n}: the line is unchanged"),
                    )),
                    Outcome::NoFile => Some(self.view.lang.pick_owned(
                        "编辑器删了临时文件,什么都没注入".to_string(),
                        "editor removed the file: nothing injected".to_string(),
                    )),
                    Outcome::BadEncoding => Some(self.view.lang.pick_owned(
                        "编辑器写回的不是 UTF-8,输入行保持原样".to_string(),
                        "the editor wrote non-UTF-8: the line is unchanged".to_string(),
                    )),
                    Outcome::BadText => Some(self.view.lang.pick_owned(
                        "写回的内容里有回车或 NUL,输入行保持原样".to_string(),
                        "the editor wrote CR or NUL: the line is unchanged".to_string(),
                    )),
                    Outcome::Unsupported => Some(self.view.lang.pick_owned(
                        "这个平台没有 unix shell,Ctrl+G 外部编辑器用不了".to_string(),
                        "no Unix shell on this platform: Ctrl+G editor is unavailable".to_string(),
                    )),
                };
                self.view.note = note;
            }
        }
    }

    /// 拆分标记:文字里写它就切出一个新框
    const SPLIT_MARK: &'static str = "<|new box|>";

    /// 按未转义的标记切段.`\<|new box|>` 是字面量:不切,反斜杠也保留,
    /// 这样下次编辑还知道它是转义形式
    fn split_segments(text: &str) -> Vec<String> {
        let mark = Self::SPLIT_MARK;
        let mut out = Vec::new();
        let mut cur = String::new();
        let mut i = 0usize;
        while i < text.len() {
            if text[i..].starts_with(mark) {
                out.push(std::mem::take(&mut cur));
                i += mark.len();
                continue;
            }
            if text[i..].starts_with('\\') && text[i + 1..].starts_with(mark) {
                cur.push('\\');
                cur.push_str(mark);
                i += 1 + mark.len();
                continue;
            }
            let ch = text[i..].chars().next().expect("字符边界");
            cur.push(ch);
            i += ch.len_utf8();
        }
        out.push(cur);
        out
    }

    /// 文字里有没有未转义的分隔标记
    fn has_unescaped_mark(text: &str) -> bool {
        let mark = Self::SPLIT_MARK;
        let mut i = 0usize;
        while i < text.len() {
            if text[i..].starts_with(mark) {
                return true;
            }
            if text[i..].starts_with('\\') && text[i + 1..].starts_with(mark) {
                i += 1 + mark.len();
                continue;
            }
            i += text[i..].chars().next().map(char::len_utf8).unwrap_or(1);
        }
        false
    }

    /// 从 (x, y) 往右找第一个放得下 `size` 的空位;一行扫完就换下一行
    fn next_free_spot(&self, x: i32, y: i32, size: (i32, i32), skip: usize) -> (i32, i32) {
        let mut at = (x, y);
        let mut guard = 0;
        while guard < 5000 {
            let hit = self.doc.boxes.iter().enumerate().any(|(i, b)| {
                if i == skip {
                    return false;
                }
                let (bw, bh) = (b.size.0 as i32, b.size.1 as i32);
                at.0 < b.at.0 + bw
                    && b.at.0 < at.0 + size.0
                    && at.1 < b.at.1 + bh
                    && b.at.1 < at.1 + size.1
            });
            if !hit {
                return at;
            }
            at.0 += 1;
            if at.0 > x + 120 {
                at = (x, at.1 + 1);
            }
            guard += 1;
        }
        at
    }

    /// 用 `<|new box|>` 把一个框拆成多个:第一段留在原框,后面每段在右边新建.
    /// 返回 true 表示这次提交按拆分处理了(调用方不要再走普通的改文字)
    fn split_text(&mut self, i: usize, text: &str) -> bool {
        if !text.contains(Self::SPLIT_MARK) && !text.contains(&format!("\\{}", Self::SPLIT_MARK)) {
            return false;
        }
        let has_unescaped = Self::has_unescaped_mark(text);
        let segs: Vec<String> = Self::split_segments(text)
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect();
        // 只有一个普通框能拆:边注释框、点节点、多选都不动
        let ok = i < self.doc.boxes.len()
            && self.doc.boxes[i].edge.is_none()
            && !self.doc.boxes[i].style.is_point();
        if !ok {
            // 不能拆的框:标记当普通文字写进去
            return false;
        }
        if !has_unescaped {
            // 只有转义形式:按普通文字写回去
            return false;
        }
        if segs.len() < 2 {
            let note = self.view.lang.pick_owned(
                "没有可拆的内容(nothing to split)".to_string(),
                "nothing to split".to_string(),
            );
            self.view.note = Some(note);
            return true;
        }
        let (style, color, fill, layer) = {
            let b = &self.doc.boxes[i];
            (b.style, b.color, b.fill, b.layer.clone())
        };
        let (tcolor, tbg, align) = self
            .doc
            .text(i)
            .map(|t| (t.color, t.bg, t.align))
            .unwrap_or((None, None, crate::model::Align::Left));
        self.begin_edit("拆分文字");
        self.doc.set_text(i, segs[0].clone());
        let mut prev = i;
        for seg in segs.iter().skip(1) {
            let (px, py, pw) = {
                let b = &self.doc.boxes[prev];
                (b.at.0, b.at.1, b.size.0 as i32)
            };
            let Ok(n) = self.doc.add_box(seg, (px + pw + 1, py), style) else {
                continue;
            };
            {
                let b = &mut self.doc.boxes[n];
                b.color = color;
                b.fill = fill;
                b.layer = layer.clone();
            }
            if let Some(ti) = self.doc.text_of(n) {
                self.doc.texts[ti].color = tcolor;
                self.doc.texts[ti].bg = tbg;
                self.doc.texts[ti].align = align;
            }
            self.doc.set_text(n, seg.clone());
            let size = {
                let b = &self.doc.boxes[n];
                (b.size.0 as i32, b.size.1 as i32)
            };
            let at = self.next_free_spot(px + pw + 1, py, size, n);
            self.doc.place_box(n, at);
            prev = n;
        }
        self.doc.rebuild();
        self.refresh_pick();
        self.end_edit();
        let n = segs.len();
        let note = self
            .view
            .lang
            .pick_owned(format!("拆成 {n} 个框"), format!("split into {n} boxes"));
        self.view.note = Some(note);
        true
    }

    /// 提示行回车:改一批框的文字(整批一步撤销)
    fn cmd_set_text(&mut self, ids: &[usize], text: &str) {
        if ids.is_empty() {
            return;
        }
        // 单个普通框 + 拆分标记:走拆分(多选不拆)
        if ids.len() == 1 && self.split_text(ids[0], text) {
            return;
        }
        self.require_edit_scale();
        self.begin_edit("文字");
        for &i in ids {
            if i < self.doc.boxes.len() {
                self.doc.set_text(i, text.to_string());
            }
        }
        self.refresh_pick();
        self.end_edit();
        let name = label_of(&self.doc, ids[0]);
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
                .pick_owned(format!("文字 = {name}{zh}"), format!("text = {name}{en}")),
        );
    }

    fn handle_cmd(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        let ctrl = modifiers.contains(KeyModifiers::CONTROL);
        let alt = modifiers.contains(KeyModifiers::ALT);
        let (len, pos) = match &self.view.cmd {
            Some(t) => {
                let n = t.chars().count();
                (n, self.view.cmd_pos.min(n))
            }
            None => (0, 0),
        };
        let mut want: Option<usize> = None;
        // 改过内容才要重算路径提示 / 关掉补全菜单;纯移动光标不用
        let mut touched = false;
        match code {
            // Ctrl+G 只给文字输入用;命令行是单行语义,按了说一声
            KeyCode::Char('g') if ctrl => {
                let note = self.view.lang.pick_owned(
                    "Ctrl+G 只用于文字输入(框文字 / 新建节点)".to_string(),
                    "Ctrl+G only works for text input (box text / new node)".to_string(),
                );
                self.view.note = Some(note);
            }
            KeyCode::Esc => {
                self.view.cmd = None;
                self.view.cmd_menu = None;
                self.view.cmd_hint = None;
                self.view.cmd_pos = 0;
            }
            KeyCode::Enter => {
                self.view.cmd_menu = None;
                self.view.cmd_hint = None;
                self.view.cmd_pos = 0;
                let line = self.view.cmd.take().unwrap_or_default();
                self.cmd_apply(&line);
            }
            KeyCode::Tab => self.complete_cmd(false),
            KeyCode::BackTab => self.complete_cmd(true),
            // 行首/行尾
            KeyCode::Home => want = Some(0),
            KeyCode::End => want = Some(len),
            KeyCode::Char('a') if ctrl => want = Some(0),
            KeyCode::Char('e') if ctrl => want = Some(len),
            // 一词(要排在"一字"前面)
            KeyCode::Left if alt => want = self.view.cmd.as_ref().map(|t| word_left(t, pos)),
            KeyCode::Right if alt => want = self.view.cmd.as_ref().map(|t| word_right(t, pos)),
            KeyCode::Char('b') if alt => want = self.view.cmd.as_ref().map(|t| word_left(t, pos)),
            KeyCode::Char('f') if alt => want = self.view.cmd.as_ref().map(|t| word_right(t, pos)),
            // 一字
            KeyCode::Left => want = Some(pos.saturating_sub(1)),
            KeyCode::Right => want = Some((pos + 1).min(len)),
            KeyCode::Char('b') if ctrl => want = Some(pos.saturating_sub(1)),
            KeyCode::Char('f') if ctrl => want = Some((pos + 1).min(len)),
            // Alt+退格:删前一个词(和 Ctrl-W 一样)
            KeyCode::Backspace if alt => {
                if let Some(b) = self.view.cmd.as_mut() {
                    let from = word_left(b, pos);
                    let (a, e) = (char_byte(b, from), char_byte(b, pos));
                    b.replace_range(a..e, "");
                    want = Some(from);
                }
                touched = true;
            }
            // 删
            KeyCode::Backspace => {
                if let Some(b) = self.view.cmd.as_mut() {
                    let mut p = pos;
                    edit_backspace(b, &mut p);
                    want = Some(p);
                }
                touched = true;
            }
            KeyCode::Delete => {
                if let Some(b) = self.view.cmd.as_mut() {
                    edit_delete(b, pos);
                }
                touched = true;
            }
            KeyCode::Char('d') if ctrl => {
                if let Some(b) = self.view.cmd.as_mut() {
                    edit_delete(b, pos);
                }
                touched = true;
            }
            KeyCode::Char('k') if ctrl => {
                if let Some(b) = self.view.cmd.as_mut() {
                    let at = char_byte(b, pos);
                    b.truncate(at);
                }
                want = Some(pos);
                touched = true;
            }
            KeyCode::Char('u') if ctrl => {
                if let Some(b) = self.view.cmd.as_mut() {
                    let at = char_byte(b, pos);
                    b.replace_range(..at, "");
                }
                want = Some(0);
                touched = true;
            }
            KeyCode::Char('w') if ctrl => {
                if let Some(b) = self.view.cmd.as_mut() {
                    let from = word_left(b, pos);
                    let (a, e) = (char_byte(b, from), char_byte(b, pos));
                    b.replace_range(a..e, "");
                    want = Some(from);
                }
                touched = true;
            }
            KeyCode::Char(c) if !ctrl && !alt => {
                if let Some(b) = self.view.cmd.as_mut() {
                    let mut p = pos;
                    edit_insert(b, &mut p, &c.to_string());
                    want = Some(p);
                }
                touched = true;
            }
            _ => {}
        }
        if let Some(p) = want {
            self.view.cmd_pos = p;
        }
        if touched {
            self.view.cmd_menu = None;
            self.refresh_cmd_hint();
        }
    }

    fn refresh_cmd_hint(&mut self) {
        let hint = match self.view.cmd.as_deref() {
            Some(line) => self.cmd_hint_for(line),
            None => None,
        };
        self.view.cmd_hint = hint;
    }

    fn cmd_hint_for(&self, line: &str) -> Option<String> {
        let (verb, arg) = match line.split_once(' ') {
            Some((v, rest)) => (v, rest.trim()),
            None => (line, ""),
        };
        match verb {
            // 存盘:参数空就显示"不写路径会存到哪"
            "w" | "wq" | "save" => {
                if !arg.is_empty() {
                    return Some(paths::absolute_display(arg));
                }
                let Some(default) = self.default_save_path() else {
                    return Some(self.view.lang.pick_owned(
                        "还没有文件名:敲个路径".to_string(),
                        "no file name yet; type a path".to_string(),
                    ));
                };
                let abs = paths::absolute_display(&default);
                let src = self.source.clone().unwrap_or_default();
                let text = if src.to_ascii_lowercase().ends_with(".gtui") {
                    self.view.lang.pick_owned(
                        format!("{abs}  (覆盖原文件)"),
                        format!("{abs}  (overwrite)"),
                    )
                } else {
                    let ext = src
                        .rsplit_once('.')
                        .map(|(_, e)| e.to_string())
                        .unwrap_or_default();
                    self.view.lang.pick_owned(
                        format!("{abs}  (源文件 .{ext} 不动)"),
                        format!("{abs}  (source .{ext} stays untouched)"),
                    )
                };
                Some(text)
            }
            "e" => {
                if arg.is_empty() {
                    return Some(self.view.lang.pick_owned(
                        "回车:文件列表(当前目录 + 源目录);Tab:补路径".to_string(),
                        "Enter: file list (cwd + source dir); Tab: complete a path".to_string(),
                    ));
                }
                Some(self.view.lang.pick_owned(
                    format!("打开 {}", paths::absolute_display(arg)),
                    format!("open {}", paths::absolute_display(arg)),
                ))
            }
            "e!" => Some(self.view.lang.pick_owned(
                if arg.is_empty() {
                    "强制重新载入(丢弃本地改动)".to_string()
                } else {
                    format!("强制打开 {}", paths::absolute_display(arg))
                },
                if arg.is_empty() {
                    "force reload (drops local edits)".to_string()
                } else {
                    format!("force open {}", paths::absolute_display(arg))
                },
            )),
            "export" | "export-canvas" => {
                if arg.is_empty() {
                    return Some(
                        self.view
                            .lang
                            .pick_owned("需要一个路径".to_string(), "needs a path".to_string()),
                    );
                }
                Some(self.view.lang.pick_owned(
                    format!("导出 {}", paths::absolute_display(arg)),
                    format!("export {}", paths::absolute_display(arg)),
                ))
            }
            _ => None,
        }
    }

    /// 搜索输入行:每敲一下都重算命中
    fn handle_search(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        let ctrl = modifiers.contains(KeyModifiers::CONTROL);
        let alt = modifiers.contains(KeyModifiers::ALT);
        let (len, pos) = match &self.view.query {
            Some(t) => {
                let n = t.chars().count();
                (n, self.view.query_pos.min(n))
            }
            None => (0, 0),
        };
        let mut want: Option<usize> = None;
        let mut touched = false;
        match code {
            // Ctrl+G 只给文字输入用;搜索词是单行
            KeyCode::Char('g') if ctrl => {
                let note = self.view.lang.pick_owned(
                    "Ctrl+G 只用于文字输入(框文字 / 新建节点)".to_string(),
                    "Ctrl+G only works for text input (box text / new node)".to_string(),
                );
                self.view.note = Some(note);
            }
            // esc:连人带命中一起退回去
            KeyCode::Esc => {
                self.view.query = None;
                self.view.query_pos = 0;
                self.view.hits.clear();
                self.view.focus = self.search_focus.clone();
                self.view.cursor = self.search_from;
            }
            // 回车:命中留着,之后可以按 n 一个个跳
            KeyCode::Enter => {
                self.view.query = None;
                self.view.query_pos = 0;
                // 搜索跳过去也算一次跳转:n / Tab 之外,Ctrl-o 也能退回来
                self.refresh_pick();
                self.note_here();
            }
            // 行首/行尾
            KeyCode::Home => want = Some(0),
            KeyCode::End => want = Some(len),
            KeyCode::Char('a') if ctrl => want = Some(0),
            KeyCode::Char('e') if ctrl => want = Some(len),
            // 一词(排在"一字"前面)
            KeyCode::Left if alt => want = self.view.query.as_ref().map(|t| word_left(t, pos)),
            KeyCode::Right if alt => want = self.view.query.as_ref().map(|t| word_right(t, pos)),
            KeyCode::Char('b') if alt => want = self.view.query.as_ref().map(|t| word_left(t, pos)),
            KeyCode::Char('f') if alt => {
                want = self.view.query.as_ref().map(|t| word_right(t, pos))
            }
            // 一字
            KeyCode::Left => want = Some(pos.saturating_sub(1)),
            KeyCode::Right => want = Some((pos + 1).min(len)),
            KeyCode::Char('b') if ctrl => want = Some(pos.saturating_sub(1)),
            KeyCode::Char('f') if ctrl => want = Some((pos + 1).min(len)),
            // Alt+退格:删前一个词(和 Ctrl-W 一样)
            KeyCode::Backspace if alt => {
                if let Some(q) = self.view.query.as_mut() {
                    let from = word_left(q, pos);
                    let (a, b) = (char_byte(q, from), char_byte(q, pos));
                    q.replace_range(a..b, "");
                    want = Some(from);
                }
                touched = true;
            }
            // 删
            KeyCode::Backspace => {
                if let Some(q) = self.view.query.as_mut() {
                    let mut p = pos;
                    edit_backspace(q, &mut p);
                    want = Some(p);
                }
                touched = true;
            }
            KeyCode::Delete => {
                if let Some(q) = self.view.query.as_mut() {
                    edit_delete(q, pos);
                }
                touched = true;
            }
            KeyCode::Char('d') if ctrl => {
                if let Some(q) = self.view.query.as_mut() {
                    edit_delete(q, pos);
                }
                touched = true;
            }
            KeyCode::Char('k') if ctrl => {
                if let Some(q) = self.view.query.as_mut() {
                    let at = char_byte(q, pos);
                    q.truncate(at);
                }
                want = Some(pos);
                touched = true;
            }
            KeyCode::Char('u') if ctrl => {
                if let Some(q) = self.view.query.as_mut() {
                    let at = char_byte(q, pos);
                    q.replace_range(..at, "");
                }
                want = Some(0);
                touched = true;
            }
            KeyCode::Char('w') if ctrl => {
                if let Some(q) = self.view.query.as_mut() {
                    let from = word_left(q, pos);
                    let (a, b) = (char_byte(q, from), char_byte(q, pos));
                    q.replace_range(a..b, "");
                    want = Some(from);
                }
                touched = true;
            }
            KeyCode::Char(c) if !ctrl && !alt => {
                if let Some(q) = self.view.query.as_mut() {
                    let mut p = pos;
                    edit_insert(q, &mut p, &c.to_string());
                    want = Some(p);
                }
                touched = true;
            }
            _ => {}
        }
        if let Some(p) = want {
            self.view.query_pos = p;
        }
        if touched {
            refresh(&self.doc, &mut self.view);
        }
    }

    /// 鼠标:滚轮缩放(按住 Ctrl 改成平移),左键点中框跳过去,按住拖动
    fn handle_mouse(&mut self, me: MouseEvent) {
        match me.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let up = matches!(me.kind, MouseEventKind::ScrollUp);
                let ctrl = me.modifiers.contains(KeyModifiers::CONTROL);
                let shift = me.modifiers.contains(KeyModifiers::SHIFT);
                if !ctrl && !shift {
                    let k = if up { WHEEL_STEP } else { 1.0 / WHEEL_STEP };
                    let at = (me.column as i32, me.row as i32);
                    self.zoom_at(self.view.zoom * k, at);
                } else {
                    let p = render::panels(&self.doc, self.area, &self.view);
                    let from = self.view.center.unwrap_or_else(|| bbox_center(&self.doc));
                    let step = if up { -3 } else { 3 };
                    let (dc, dr) = if shift { (step, 0) } else { (0, step) };
                    self.view.center = Some(render::pan_center(&p.proj, from, dc, dr));
                    self.view.target = None;
                    let note = if shift {
                        self.view
                            .lang
                            .pick_owned(format!("平移 {step} 列"), format!("pan {step} columns"))
                    } else {
                        self.view
                            .lang
                            .pick_owned(format!("平移 {step} 行"), format!("pan {step} rows"))
                    };
                    self.view.note = Some(note);
                }
            }
            // 按下:按在框上就准备拖这一批框,按在空白处才准备平移视图
            MouseEventKind::Moved => {
                // 菜单开着时,鼠标划到哪一项就预览哪一项
                if let Some(m) = &self.view.menu {
                    if let Some(i) = render::menu_hit(m, self.area, me.column, me.row) {
                        if i != m.sel {
                            if let Some(m) = self.view.menu.as_mut() {
                                m.sel = i;
                            }
                            self.menu_preview();
                        }
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Right) => {
                if self.view.menu.is_some() {
                    // 菜单开着:右键只取消,不开始拖动
                    self.menu_cancel();
                } else if self.drag.is_none() {
                    // 按住右键先记着:框上是"拖出来连边",别处是"拖动平移";
                    // 两种都是拖过两格才动,松开没拖过就弹菜单
                    let p = render::panels(&self.doc, self.area, &self.view);
                    let press = (me.column, me.row);
                    if let Some(i) = render::node_at(&self.doc, &p, self.area, me.column, me.row) {
                        self.drag = Some(Drag::RightLink {
                            source: i,
                            press,
                            moved: false,
                        });
                        return;
                    }
                    let center = self.view.center.unwrap_or_else(|| bbox_center(&self.doc));
                    self.drag = Some(Drag::RightPan {
                        press,
                        center,
                        proj: p.proj,
                        moved: false,
                    });
                }
            }
            MouseEventKind::Drag(MouseButton::Right) => {
                if let Some(Drag::RightLink { press, moved, .. }) = self.drag.as_mut() {
                    let dc = me.column as i32 - press.0 as i32;
                    let dr = me.row as i32 - press.1 as i32;
                    if !*moved && dc.abs().max(dr.abs()) < RIGHT_PAN_THRESHOLD {
                        return;
                    }
                    let first = !*moved;
                    *moved = true;
                    let source = match self.drag {
                        Some(Drag::RightLink { source, .. }) => source,
                        _ => return,
                    };
                    if first {
                        // 真的拖起来了:从按下的框开始连边,顺便清掉菜单/选区干扰
                        self.view.menu = None;
                        self.view.sel = None;
                        self.view.link = Some(LinkState {
                            sources: vec![source],
                            targets: Vec::new(),
                            confirming: false,
                        });
                    }
                    let p = render::panels(&self.doc, self.area, &self.view);
                    let cell = p.proj.canvas(me.column as i32, me.row as i32);
                    self.view.cursor = cell;
                    let target = render::node_at(&self.doc, &p, self.area, me.column, me.row);
                    if let Some(link) = self.view.link.as_mut() {
                        link.targets = target.filter(|t| *t != source).into_iter().collect();
                    }
                    self.view.cand = 0;
                    self.refresh_pick();
                    self.autopan(me.column, me.row);
                    return;
                }
                if let Some(Drag::RightPan {
                    press,
                    center,
                    proj,
                    moved,
                }) = self.drag.as_mut()
                {
                    let dc = me.column as i32 - press.0 as i32;
                    let dr = me.row as i32 - press.1 as i32;
                    if !*moved && dc.abs().max(dr.abs()) < RIGHT_PAN_THRESHOLD {
                        return;
                    }
                    *moved = true;
                    let (center, proj) = (*center, *proj);
                    self.view.center = Some(render::pan_center(&proj, center, -dc, -dr));
                    self.view.target = None;
                    self.view.pan = None;
                }
            }
            MouseEventKind::Up(MouseButton::Right) => {
                match self.drag.take() {
                    Some(Drag::RightPan { press, moved, .. }) => {
                        if !moved {
                            // 没拖过阈值:按"按下那一格"弹菜单,不是松开的地方
                            self.open_menu(press.0, press.1);
                        }
                    }
                    Some(Drag::RightLink {
                        source,
                        press,
                        moved,
                    }) => {
                        if !moved {
                            self.view.link = None;
                            self.open_menu(press.0, press.1);
                            return;
                        }
                        self.view.link = None;
                        let p = render::panels(&self.doc, self.area, &self.view);
                        let target = render::node_at(&self.doc, &p, self.area, me.column, me.row);
                        match target {
                            Some(t) => self.create_edges(&[source], &[t], 1),
                            None => {
                                let note = self.view.lang.pick_owned(
                                    "松开时不在框上:没连边".to_string(),
                                    "released outside a box: nothing linked".to_string(),
                                );
                                self.view.note = Some(note);
                            }
                        }
                    }
                    other => self.drag = other,
                }
            }
            MouseEventKind::Down(MouseButton::Middle) => {
                let p = render::panels(&self.doc, self.area, &self.view);
                let center = self.view.center.unwrap_or_else(|| bbox_center(&self.doc));
                self.drag = Some(Drag::Pan {
                    from: (me.column, me.row),
                    center,
                    proj: p.proj,
                });
            }
            MouseEventKind::Down(MouseButton::Left) if self.view.menu.is_some() => {
                let hit = self
                    .view
                    .menu
                    .as_ref()
                    .and_then(|m| render::menu_hit(m, self.area, me.column, me.row));
                match hit {
                    Some(i) => {
                        if let Some(m) = self.view.menu.as_mut() {
                            m.sel = i;
                        }
                        self.menu_choose();
                    }
                    None => self.menu_cancel(),
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let p = render::panels(&self.doc, self.area, &self.view);
                // 按下的位置上若正好是"选中边的端点格",这一下是拖端点改接.
                // 要在 refresh_pick 之前记下来:按格子选东西会把 pick 换掉
                if let Some(ei) = self.view.pick.edge() {
                    let press_cell = (me.column as i32, me.row as i32);
                    if let Some((a, b)) = self.usable_hit().edge_ends.get(&ei).copied() {
                        if press_cell == a || press_cell == b {
                            self.drag = Some(Drag::Reconnect {
                                edge: ei,
                                to_end: press_cell == b,
                                moved: false,
                            });
                            self.view.link = Some(LinkState {
                                sources: vec![self
                                    .doc
                                    .box_idx(&self.doc.edges[ei].from)
                                    .unwrap_or(0)],
                                targets: Vec::new(),
                                confirming: false,
                            });
                            let note = self.view.lang.pick_owned(
                                "拖到另一个框上改接(空白处松开取消)".to_string(),
                                "drop on another box to reconnect (empty space cancels)"
                                    .to_string(),
                            );
                            self.view.note = Some(note);
                            return;
                        }
                    }
                }
                // 连边模式 + Shift:在目标集合里加/减这个框(普通点击就是"设为目标")
                if self.view.link.is_some() && me.modifiers.contains(KeyModifiers::SHIFT) {
                    if let Some(i) = render::node_at(&self.doc, &p, self.area, me.column, me.row) {
                        // 先把手上的目标落进显式集合,再在这个集合里加/减
                        let mut targets = render::link_targets(&self.doc, &p, &self.view);
                        match targets.iter().position(|t| *t == i) {
                            Some(pos) => {
                                targets.remove(pos);
                            }
                            None => {
                                targets.push(i);
                                targets.sort_unstable();
                            }
                        }
                        let n = targets.len();
                        if let Some(link) = self.view.link.as_mut() {
                            link.targets = targets;
                            link.confirming = false;
                        }
                        let note = self
                            .view
                            .lang
                            .pick_owned(format!("目标 {n} 个"), format!("{n} target(s)"));
                        self.view.note = Some(note);
                    }
                    return;
                }
                // 点在地图之外(图例/状态行)不动光标,也不起拖动
                let inside = me.column >= p.map.x
                    && me.column < p.map.x + p.map.width
                    && me.row >= p.map.y
                    && me.row < p.map.y + p.map.height;
                if !inside {
                    return;
                }
                // 单击就把光标落在点到的这一格(不做"吸到节点的角上")
                let cell = p.proj.canvas(me.column as i32, me.row as i32);
                // 双击 = 按一下 i:空处新建节点(先弹输入框),节点上改文字
                let now = std::time::Instant::now();
                let dbl = matches!(
                    self.last_click,
                    Some((t, c)) if c == (me.column, me.row)
                        && now.duration_since(t) <= std::time::Duration::from_millis(400)
                );
                self.last_click = if dbl {
                    None
                } else {
                    Some((now, (me.column, me.row)))
                };
                // 只有点在"空处"(既不在节点上、也不在线上)才算新建;
                // 同一格连点一条线是"在重叠的边之间轮",不能被当成双击
                let empty = render::node_at(&self.doc, &p, self.area, me.column, me.row).is_none()
                    && self
                        .usable_hit()
                        .edge_at((me.column as i32, me.row as i32))
                        .is_none();
                if dbl && empty {
                    self.view.cursor = cell;
                    self.view.cand = 0;
                    self.refresh_pick();
                    self.cmd_edit_or_new();
                    return;
                }
                // 已经选好的一块,点它外面就是"取消";点在里面则留着(好继续操作)
                if let Some(sel) = self.view.sel {
                    if !rect_has(sel.anchor, sel.end, cell) {
                        self.view.sel = None;
                    }
                }
                let same_cell = self.view.cursor == cell;
                self.view.cursor = cell;
                if !same_cell {
                    self.view.cand = 0;
                }
                self.refresh_pick();
                self.note_here();
                if let Some(i) = render::node_at(&self.doc, &p, self.area, me.column, me.row) {
                    // 连边模式:点框就是"把目标设成它"
                    if let Some(link) = self.view.link.as_mut() {
                        link.targets = vec![i];
                        link.confirming = false;
                        let note = self.view.lang.pick_owned(
                            "目标 1 个(Shift+点击可多选)".to_string(),
                            "1 target (Shift+click to pick more)".to_string(),
                        );
                        self.view.note = Some(note);
                        return;
                    }
                    // 按在带框节点的边框上:拖出去是连边;按在框内才是挪框.
                    // 位置已经更新过了,单击(没拖)仍旧只是落光标
                    if self.doc.boxes[i].style.is_boxed() {
                        let r = render::rect_of(&self.doc, i, &p.proj);
                        let on_border = me.column as i32 == r.0
                            || me.column as i32 == r.0 + r.2 - 1
                            || me.row as i32 == r.1
                            || me.row as i32 == r.1 + r.3 - 1;
                        if on_border && self.view.sel.is_none() {
                            self.drag = Some(Drag::Link {
                                source: i,
                                moved: false,
                            });
                            self.view.link = Some(LinkState {
                                sources: vec![i],
                                targets: Vec::new(),
                                confirming: false,
                            });
                            return;
                        }
                    }
                    // 按在框上:拖起来挪这一批框.真的动了才切 1:1、才压撤销,
                    // 所以单击还是纯粹的"落光标"
                    let sel = render::selected(&self.doc, &p, &self.view);
                    let ids: Vec<usize> = if sel.contains(&i) { sel } else { vec![i] };
                    let at = ids
                        .into_iter()
                        .filter_map(|k| self.doc.boxes.get(k).map(|b| (k, b.at)))
                        .collect();
                    let sel0 = self.view.sel.map(|x| (x.anchor, x.end, x.confirmed));
                    self.drag = Some(Drag::Boxes {
                        from: (me.column, me.row),
                        at,
                        proj: p.proj,
                        cursor0: self.view.cursor,
                        sel0,
                        began: false,
                    });
                    return;
                }
                if let Some(e) = self.usable_hit().edge_at((me.column as i32, me.row as i32)) {
                    // 空处点到线:选中这条边(鼠标是选边的另一条路).
                    // 这一格压着好几条边时,再点一下就轮到下一条
                    let cell = (me.column as i32, me.row as i32);
                    let cands = self.usable_hit().candidates_at(cell).to_vec();
                    if cands.len() > 1 {
                        let here = cands.iter().position(|&x| x == e).unwrap_or(0);
                        self.view.cand = if same_cell {
                            match self
                                .view
                                .pick
                                .edge()
                                .and_then(|cur| cands.iter().position(|&x| x == cur))
                            {
                                Some(pos) => (pos + 1) % cands.len(),
                                None => here,
                            }
                        } else {
                            here
                        };
                    } else {
                        self.view.cand = 0;
                    }
                    self.view.pick = render::Pick::Edge(e);
                    self.refresh_pick();
                    let picked = self.view.pick.edge().unwrap_or(e);
                    let note = self.view.lang.pick_owned(
                        format!("选中边 {picked}"),
                        format!("edge {picked} selected"),
                    );
                    self.view.note = Some(note);
                }
                // 拖动:按住 Ctrl(或者在连边模式里)是平移,其余在空处拉选区
                let ctrl = me.modifiers.contains(KeyModifiers::CONTROL);
                if ctrl || self.view.link.is_some() {
                    let center = self.view.center.unwrap_or_else(|| bbox_center(&self.doc));
                    self.drag = Some(Drag::Pan {
                        from: (me.column, me.row),
                        center,
                        proj: p.proj,
                    });
                } else {
                    self.drag = Some(Drag::Select { anchor: cell });
                }
            }
            // 拖动:拖视图就是图跟着手走;拖框是按按下时的位置整格挪
            MouseEventKind::Drag(MouseButton::Middle) => {
                if let Some(Drag::Pan { from, center, proj }) = &self.drag {
                    let dc = me.column as i32 - from.0 as i32;
                    let dr = me.row as i32 - from.1 as i32;
                    if dc != 0 || dr != 0 {
                        self.view.center = Some(render::pan_center(proj, *center, -dc, -dr));
                        self.view.target = None;
                        self.view.pan = None;
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => match &self.drag {
                Some(Drag::Select { anchor }) => {
                    // 拖起来了才成选区;光标跟着鼠标跑,灰边跟着变.
                    // 旧选区(不管确认没有)直接换掉:新拖出来的就是新的
                    let anchor = *anchor;
                    let fresh = self.view.sel.is_none();
                    let p = render::panels(&self.doc, self.area, &self.view);
                    let end = p.proj.canvas(me.column as i32, me.row as i32);
                    self.view.sel = Some(RectSel {
                        anchor,
                        end,
                        confirmed: false,
                    });
                    if fresh {
                        let note = self.view.lang.pick_owned(
                            format!("区块选择:锚点 ({},{})", anchor.0, anchor.1),
                            format!("block select from ({},{})", anchor.0, anchor.1),
                        );
                        self.view.note = Some(note);
                    }
                    self.view.cursor = end;
                    self.view.cand = 0;
                    self.refresh_pick();
                }
                Some(Drag::Pan { from, center, proj }) => {
                    let dc = me.column as i32 - from.0 as i32;
                    let dr = me.row as i32 - from.1 as i32;
                    if dc != 0 || dr != 0 {
                        self.view.center = Some(render::pan_center(proj, *center, -dc, -dr));
                        self.view.target = None;
                        self.view.pan = None;
                    }
                }
                // 拖连边 / 拖端点:光标下是框就把它当目标(预览也跟过去),
                // 靠边时顺手把视图往那边推一格
                Some(Drag::Link { .. }) | Some(Drag::Reconnect { .. }) => {
                    match self.drag.as_mut() {
                        Some(Drag::Link { moved, .. }) | Some(Drag::Reconnect { moved, .. }) => {
                            *moved = true
                        }
                        _ => {}
                    }
                    let p = render::panels(&self.doc, self.area, &self.view);
                    let target = render::node_at(&self.doc, &p, self.area, me.column, me.row);
                    if let Some(link) = self.view.link.as_mut() {
                        link.targets = target.into_iter().collect();
                    }
                    if let Some(i) = target {
                        self.view.cursor = self.doc.boxes[i].at;
                        self.view.pick = render::Pick::Box(i);
                    } else {
                        // 还没到目标框:光标跟着指针走,预览才有终点可画
                        self.view.cursor = p.proj.canvas(me.column as i32, me.row as i32);
                        self.view.cand = 0;
                        self.refresh_pick();
                    }
                    self.autopan(me.column, me.row);
                }
                Some(Drag::Boxes {
                    from,
                    at,
                    proj,
                    cursor0,
                    sel0,
                    began,
                }) => {
                    // 屏幕格 -> 画布格:任何缩放下都按按下时的投影换算,
                    // 用"原始位置 + 总位移",不逐帧累加,避免取整误差
                    let a = proj.canvas_f(from.0 as i32, from.1 as i32);
                    let b = proj.canvas_f(me.column as i32, me.row as i32);
                    let d = ((b.0 - a.0).round() as i32, (b.1 - a.1).round() as i32);
                    if d == (0, 0) {
                        return;
                    }
                    let (from, at, began) = (*from, at.clone(), *began);
                    let (proj, cursor0, sel0) = (*proj, *cursor0, *sel0);
                    if !began {
                        self.begin_edit("挪框");
                        self.drag = Some(Drag::Boxes {
                            from,
                            at: at.clone(),
                            proj,
                            cursor0,
                            sel0,
                            began: true,
                        });
                    }
                    for &(k, at0) in &at {
                        let cur = self.doc.boxes[k].at;
                        self.doc
                            .move_box(k, at0.0 + d.0 - cur.0, at0.1 + d.1 - cur.1);
                    }
                    // 光标和选区跟着走同一个位移(不然看起来像框脱手了)
                    self.view.cursor = (cursor0.0 + d.0, cursor0.1 + d.1);
                    if let Some((anchor, end, confirmed)) = sel0 {
                        self.view.sel = Some(RectSel {
                            anchor: (anchor.0 + d.0, anchor.1 + d.1),
                            end: (end.0 + d.0, end.1 + d.1),
                            confirmed,
                        });
                    }
                    // 拖动期间把选区矩形藏起来(松手后再画)
                    self.view.drag_hide_sel = true;
                    self.refresh_pick();
                }
                // 右键正在拖:左键的拖动事件插进来也不关它的事
                Some(Drag::RightPan { .. }) | Some(Drag::RightLink { .. }) | None => {}
            },
            // 松开:拖动到此结束(单击在按下时就已经落好光标了).
            // 拖选区就是"按住拉一块、松开选完":抬手即确认,不用再按 Enter
            MouseEventKind::Up(MouseButton::Left) => {
                if matches!(
                    self.drag,
                    Some(Drag::RightPan { .. }) | Some(Drag::RightLink { .. })
                ) {
                    return; // 右键拖到一半时松了左键:不关它的事
                }
                let drag = self.drag.take();
                if let Some(Drag::Boxes { at, began, .. }) = &drag {
                    let moved = *began;
                    self.view.drag_hide_sel = false;
                    self.refresh_pick();
                    self.end_edit();
                    if moved {
                        let n = at.len();
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
                        let note = self
                            .view
                            .lang
                            .pick_owned(format!("拖动完成{zh}"), format!("drag done{en}"));
                        self.view.note = Some(note);
                    }
                    return;
                }
                if let Some(Drag::Link { source, moved }) = drag {
                    if !moved {
                        self.view.link = None;
                        return;
                    }
                    let target = render::node_at(
                        &self.doc,
                        &render::panels(&self.doc, self.area, &self.view),
                        self.area,
                        me.column,
                        me.row,
                    );
                    self.view.link = None;
                    if let Some(t) = target {
                        self.create_edges(&[source], &[t], 1);
                    } else {
                        let note = self.view.lang.pick_owned(
                            "松开时不在框上:没连边".to_string(),
                            "released outside a box: nothing linked".to_string(),
                        );
                        self.view.note = Some(note);
                    }
                    return;
                }
                if let Some(Drag::Reconnect {
                    edge,
                    to_end,
                    moved,
                }) = drag
                {
                    if !moved {
                        self.view.link = None;
                        return;
                    }
                    let target = render::node_at(
                        &self.doc,
                        &render::panels(&self.doc, self.area, &self.view),
                        self.area,
                        me.column,
                        me.row,
                    );
                    self.view.link = None;
                    let Some(t) = target else {
                        let note = self.view.lang.pick_owned(
                            "松开时不在框上:连线没改".to_string(),
                            "released outside a box: edge unchanged".to_string(),
                        );
                        self.view.note = Some(note);
                        return;
                    };
                    let id = self.doc.boxes[t].id.clone();
                    self.begin_edit("改接边");
                    if let Some(e) = self.doc.edges.get_mut(edge) {
                        if to_end {
                            e.to = id.clone();
                        } else {
                            e.from = id.clone();
                        }
                    }
                    self.doc.rebuild();
                    self.end_edit();
                    let note = self.view.lang.pick_owned(
                        format!("边 {edge} 改接到 {id}"),
                        format!("edge {edge} now ends at {id}"),
                    );
                    self.view.note = Some(note);
                    return;
                }
                if let Some(Drag::Select { .. }) = drag {
                    // 按下的那一下可能被终端/tmux 吃掉(切窗格):松手补一次"点这里"
                    if self.view.sel.is_none() {
                        let p = render::panels(&self.doc, self.area, &self.view);
                        let inside = me.column >= p.map.x
                            && me.column < p.map.x + p.map.width
                            && me.row >= p.map.y
                            && me.row < p.map.y + p.map.height;
                        if inside {
                            self.view.cursor = p.proj.canvas(me.column as i32, me.row as i32);
                            self.view.cand = 0;
                            self.refresh_pick();
                            self.note_here();
                        }
                        return;
                    }
                }
                if matches!(drag, Some(Drag::Select { .. })) && self.view.sel.is_some() {
                    self.confirm_selection();
                    if self.view.sel.is_some_and(|s| !s.confirmed) {
                        // 圈出来的地方什么都没有:不留一个空选区挡着编辑
                        self.view.sel = None;
                        let note = self.view.lang.pick_owned(
                            "选区为空".to_string(),
                            "the selection is empty".to_string(),
                        );
                        self.view.note = Some(note);
                    }
                }
            }
            _ => {}
        }
    }

    /// 跳到某个框:挪光标,记进历史,再保证它还在画面里
    pub fn jump_to(&mut self, i: usize) {
        if i >= self.doc.boxes.len() {
            return;
        }
        let cell = self.cell_of(i);
        if cell == self.view.cursor {
            // 光标没动,但选中的元素可能变了(比如刚从边上跳回来)
            self.view.pick = render::Pick::Box(i);
            return;
        }
        self.view.cursor = cell;
        self.view.cand = 0;
        self.refresh_pick();
        self.note_here();
        self.follow_cursor_jump();
    }

    /// 框的光标格:带框的落在文字起点,点节点就是它自己
    fn cell_of(&self, i: usize) -> render::Cell {
        match self.doc.text(i) {
            Some(t) => t.at,
            None => (self.doc.boxes[i].at.0 + 1, self.doc.boxes[i].at.1 + 1),
        }
    }

    /// v / Ctrl-q:从光标格开始区块选择
    fn start_rect_select(&mut self) {
        self.view.sel = Some(RectSel {
            anchor: self.view.cursor,
            end: self.view.cursor,
            confirmed: false,
        });
        let (x, y) = self.view.cursor;
        let note = self.view.lang.pick_owned(
            format!("区块选择:锚点 ({x},{y})(移动光标扩大,esc 取消)"),
            format!("block select from ({x},{y}) (move to grow, esc to cancel)"),
        );
        self.view.note = Some(note);
    }

    /// z / 空格:把光标那个框挪到画面正中
    fn center_cursor(&mut self) {
        let p = (self.view.cursor.0 as f64, self.view.cursor.1 as f64);
        pan_to(&mut self.view, &self.doc, p);
    }

    /// f:把聚焦的分组(没聚焦就是光标所在的分组)铺满画面
    fn fit_focus(&mut self) {
        let p = render::panels(&self.doc, self.area, &self.view);
        let groups = match self.cursor_box() {
            Some(c) if self.view.focus.is_empty() => self.doc.box_groups(c).to_vec(),
            _ => self.view.focus.clone(),
        };
        let pts: Vec<(f64, f64)> = (0..self.doc.boxes.len())
            .filter(|&i| self.doc.box_groups(i).iter().any(|g| groups.contains(g)))
            .map(|i| self.doc.center(i))
            .collect();
        if let Some((z, c)) = fit_to(p.map, pts.into_iter()) {
            self.view.zoom = z;
            self.view.center = Some(c);
            self.view.target = None;
        }
    }

    /// 键盘挪光标:贴到边距时把画面挪一小段,挪完光标离边距还有几格缓冲.
    /// 一次挪一块(而不是每挪一行就拉一次画面),也不把光标拽到正中间
    fn follow_cursor(&mut self) {
        // 还没画过一帧就没有视口,无从判断光标在不在画面里
        if self.area.width == 0 || self.area.height == 0 {
            return;
        }
        // 缓动还没走到目标时按目标算:不然每按一下都还在边距外,会被一路推着走
        let mut look = self.view.clone();
        if let Some(t) = self.view.target {
            look.center = Some(t);
        }
        let p = render::panels(&self.doc, self.area, &look);
        let (cx, cy) = p
            .proj
            .cell((self.view.cursor.0 as f64, self.view.cursor.1 as f64));
        // 留两格边距:光标贴着边界时就把画面挪过去,不要等它掉出去
        const M: i32 = 2;
        let (x0, y0) = (p.map.x as i32 + M, p.map.y as i32 + M);
        let (x1, y1) = (
            (p.map.x + p.map.width) as i32 - 1 - M,
            (p.map.y + p.map.height) as i32 - 1 - M,
        );
        if cx >= x0 && cx <= x1 && cy >= y0 && cy <= y1 {
            return;
        }
        // 像 nvim 那样:贴到边距之后,光标在屏幕上就钉在那儿,只有逻辑位置在走,
        // 画面一格一格跟着滚 —— 所以这里只补"越出去的那一点",不多挪.
        // 而且**当场落位**,不走缓动:缓动的话光标会先跟着逻辑位置走一格,
        // 再被追上来的画面带回原处,看着就是"先动一下又弹回来"
        let zoom = self.view.zoom.max(ZOOM_MIN);
        let cur = self.view.center.unwrap_or_else(|| bbox_center(&self.doc));
        let mut want = cur;
        if cx < x0 {
            want.0 = cur.0 + (cx - x0) as f64 / zoom;
        } else if cx > x1 {
            want.0 = cur.0 + (cx - x1) as f64 / zoom;
        }
        if cy < y0 {
            want.1 = cur.1 + (cy - y0) as f64 / zoom;
        } else if cy > y1 {
            want.1 = cur.1 + (cy - y1) as f64 / zoom;
        }
        self.view.center = Some(want);
        self.view.target = None;
        self.view.pan = None;
    }

    /// 序列变了就同步到右下角(showcmd)
    fn show_seq(&mut self) {
        self.view.showcmd = self.seq.clone();
    }

    /// 作废当前序列
    fn clear_seq(&mut self) {
        if self.seq.is_empty() && self.view.showcmd.is_empty() {
            return;
        }
        self.seq.clear();
        self.pending = None;
        self.show_seq();
    }

    /// 吃"还没定下来的按键":数字 / 逗号 / g / G.返回 true = 这一下已经用掉了
    fn seq_key(&mut self, c: char) -> bool {
        if c.is_ascii_digit() {
            // 开头的 0 不吞:0 是"回到 1:1"
            if c == '0' && self.seq.is_empty() {
                return false;
            }
            self.seq.push(c);
            if self.seq.contains(',') {
                self.pending = None; // 坐标里的第二段不是计数
            } else {
                count_digit(&mut self.pending, c);
            }
            self.show_seq();
            return true;
        }
        if c == ',' {
            if self.seq.is_empty() || !self.seq.chars().all(|x| x.is_ascii_digit()) {
                return false;
            }
            self.seq.push(c);
            self.pending = None;
            self.show_seq();
            return true;
        }
        if c == 'g' {
            if self.seq.is_empty() {
                self.seq.push('g');
                self.show_seq();
                return true;
            }
            if self.seq == "g" {
                self.seq.clear();
                self.pending = None;
                self.show_seq();
                self.goto_cell((0, 0)); // gg:回到原点
                return true;
            }
            self.clear_seq();
            return false;
        }
        if c == 'f' && self.seq == "g" {
            // gf:打开光标下的链接(不是"铺满画面"那个 f)
            self.clear_seq();
            self.open_link();
            return true;
        }
        if c == 'G' {
            if self.seq.is_empty() {
                return false;
            }
            let cell = parse_goto(&self.seq, self.view.cursor);
            self.clear_seq();
            if let Some(cell) = cell {
                self.goto_cell(cell);
            }
            return true;
        }
        // 别的键:序列作废,按键照常处理
        if !self.seq.is_empty() {
            self.seq.clear();
            self.show_seq();
        }
        false
    }

    /// 跳到画布上的某个格子(gg / a,bG):记进跳转表,视图跟过去
    fn goto_cell(&mut self, cell: (i32, i32)) {
        if self.view.cursor == cell {
            return;
        }
        self.note_here();
        self.view.cursor = cell;
        self.view.cand = 0;
        self.refresh_pick();
        self.follow_cursor_jump();
    }

    /// 把"现在这里"记进跳转历史(Ctrl-o / Ctrl-i 前后翻).记的是 tab + 格子,
    /// 所以在另一个 buffer 里也能退回来
    fn note_here(&mut self) {
        let cur = Jump {
            tab: self.tab_at,
            cell: self.view.cursor,
        };
        if self.jumps.get(self.jump_at) == Some(&cur) {
            return;
        }
        if self.jumps.last() == Some(&cur) {
            self.jump_at = self.jumps.len() - 1;
            return;
        }
        self.jumps.truncate(self.jump_at + 1); // 从历史中间跳走,后面那段丢掉
        self.jumps.push(cur);
        if self.jumps.len() > 64 {
            self.jumps.remove(0);
        }
        self.jump_at = self.jumps.len() - 1;
    }

    /// 回到历史里的上一个 / 下一个跳点(可能换 tab).返回 false 表示到头了
    fn jump_history(&mut self, back: bool) -> bool {
        if self.jumps.is_empty() {
            return false;
        }
        if back {
            // 站在一个没记过的地方(走过来的):先把自己记上,再往回退,
            // 这样 Ctrl-o 永远回到"上次动手/搜索的地方",不是再往前一格
            let here = Jump {
                tab: self.tab_at,
                cell: self.view.cursor,
            };
            if self.jumps.get(self.jump_at) != Some(&here) {
                self.note_here();
            }
            if self.jump_at == 0 {
                return false;
            }
            self.jump_at -= 1;
        } else {
            if self.jump_at + 1 >= self.jumps.len() {
                return false;
            }
            self.jump_at += 1;
        }
        let j = self.jumps[self.jump_at];
        if j.tab != self.tab_at {
            // 跨 buffer:先把那个 tab 切回来,再落光标
            self.switch_tab(j.tab);
        }
        self.view.cursor = j.cell;
        self.view.cand = 0;
        self.refresh_pick();
        self.view.note = None;
        self.follow_cursor_jump();
        true
    }

    /// 跳到别的地方(搜索 / 鼠标点 / 跳转历史):把光标往画面里带一把
    fn follow_cursor_jump(&mut self) {
        if self.area.width == 0 || self.area.height == 0 {
            return;
        }
        // 缓动还没走完就按目标算:不然会以为光标还在画面外,反复重设目标
        let mut look = self.view.clone();
        if let Some(t) = self.view.target {
            look.center = Some(t);
        }
        let p = render::panels(&self.doc, self.area, &look);
        let (cx, cy) = p
            .proj
            .cell((self.view.cursor.0 as f64, self.view.cursor.1 as f64));
        const M: i32 = 2;
        let (x0, y0) = (p.map.x as i32 + M, p.map.y as i32 + M);
        let (x1, y1) = (
            (p.map.x + p.map.width) as i32 - 1 - M,
            (p.map.y + p.map.height) as i32 - 1 - M,
        );
        if cx >= x0 && cx <= x1 && cy >= y0 && cy <= y1 {
            return;
        }
        // 跳到画面外:直接把光标放到正中.老的 0.85 拉近只适合"刚好越界",
        // 从 1000,1000 跳到 10,10 时它只走到半路,看着像没动
        let want = (self.view.cursor.0 as f64, self.view.cursor.1 as f64);
        pan_to(&mut self.view, &self.doc, want);
    }

    /// 半屏跳:从光标往 dir 方向挪半屏,落到离那儿最近的框上
    fn leap(&self, dir: (i32, i32)) -> usize {
        let p = render::panels(&self.doc, self.area, &self.view);
        let c = p
            .proj
            .cell((self.view.cursor.0 as f64, self.view.cursor.1 as f64));
        let dx = dir.0 * (p.map.width as i32 / 2).max(1);
        let dy = dir.1 * (p.map.height as i32 / 2).max(1);
        let (ax, ay) = (p.map.x as i32, p.map.y as i32);
        let x_max = ax + p.map.width as i32 - 1;
        let y_max = ay + p.map.height as i32 - 1;
        let x = (c.0 + dx).clamp(ax, x_max.max(ax));
        let y = (c.1 + dy).clamp(ay, y_max.max(ay));
        render::node_at(&self.doc, &p, self.area, x as u16, y as u16)
            .or_else(|| {
                // 那儿没有框:挑离它最近的框
                (0..self.doc.boxes.len()).min_by_key(|&i| {
                    let (bx, by) = p.proj.cell(self.doc.center(i));
                    (bx - x).abs() + (by - y).abs()
                })
            })
            .unwrap_or(0)
    }

    /// 方向键移动:走一步挑屏幕上最近的那个框,连着按计数前缀就走几步
    /// 光标移动:一格一步,数字前缀就是格数
    fn move_cursor(&mut self, dir: (i32, i32), steps: u32) {
        let steps = steps.max(1) as i32;
        self.view.cursor = (
            self.view.cursor.0 + dir.0 * steps,
            self.view.cursor.1 + dir.1 * steps,
        );
        self.view.cand = 0;
        self.refresh_pick();
        self.follow_cursor();
    }
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
mod tests {
    use super::*;
    use crate::document::{Box, Dir, DocEdge, Group, Text};

    fn 样例文档() -> Document {
        let g0 = Group {
            id: "g0".into(),
            label: "1".into(),
            color: (0xff, 0, 0),
        };
        let g1 = Group {
            id: "g1".into(),
            label: "2".into(),
            color: (0, 0xff, 0),
        };
        let layers = vec![Layer {
            name: String::new(),
            order: 0,
        }];
        let boxes = vec![
            Box {
                id: "b0".into(),
                at: (0, 0),
                size: (6, 3),
                style: BoxStyle::Rounded,
                color: None,
                fill: None,
                layer: String::new(),
                terms: vec!["jia".into()],
                edge: None,
            },
            Box {
                id: "b1".into(),
                at: (10, 0),
                size: (6, 3),
                style: BoxStyle::Rounded,
                color: None,
                fill: None,
                layer: String::new(),
                terms: vec!["yi".into()],
                edge: None,
            },
            Box {
                id: "b2".into(),
                at: (20, 0),
                size: (6, 3),
                style: BoxStyle::Rounded,
                color: None,
                fill: None,
                layer: String::new(),
                terms: Vec::new(),
                edge: None,
            },
        ];
        let texts = vec![
            Text {
                id: "t0".into(),
                text: "甲".into(),
                at: (1, 1),
                box_id: "b0".into(),
                color: None,
                bg: None,
                align: Align::Left,
                layer: String::new(),
            },
            Text {
                id: "t1".into(),
                text: "乙".into(),
                at: (11, 1),
                box_id: "b1".into(),
                color: None,
                bg: None,
                align: Align::Left,
                layer: String::new(),
            },
            Text {
                id: "t2".into(),
                text: "丙".into(),
                at: (21, 1),
                box_id: "b2".into(),
                color: None,
                bg: None,
                align: Align::Left,
                layer: String::new(),
            },
        ];
        let edges = vec![
            DocEdge {
                id: "e0".into(),
                from: "b0".into(),
                to: "b1".into(),
                dir: Dir::Forward,
                group: "g0".into(),
                style: EdgeStyle::Solid,
                color: None,
                bg: None,
                via: Vec::new(),
                layer: String::new(),
                corner: Corner::Rounded,
                from_side: None,
                to_side: None,
                head: None,
            },
            DocEdge {
                id: "e1".into(),
                from: "b2".into(),
                to: "b1".into(),
                dir: Dir::None,
                group: "g1".into(),
                style: EdgeStyle::Solid,
                color: None,
                bg: None,
                via: Vec::new(),
                layer: String::new(),
                corner: Corner::Rounded,
                from_side: None,
                to_side: None,
                head: None,
            },
        ];
        let view = ViewState {
            cursor: Some(crate::document::Cursor::Box("b0".into())),
            ..ViewState::default()
        };
        Document::from_parts(
            "graph-tui 0.2.0",
            vec![g0, g1],
            layers,
            boxes,
            texts,
            edges,
            view,
        )
        .expect("样例文档")
    }

    fn 临时路径(tag: &str, ext: &str) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir()
            .join(format!(
                "graph-tui-app-{tag}-{}-{nanos}.{ext}",
                std::process::id()
            ))
            .to_string_lossy()
            .into_owned()
    }

    /// 测试里统一用中文界面(默认已经改成英文;中文文案另有用例守着)
    fn zh_opts() -> Options {
        Options {
            lang: Lang::Zh,
            ..Options::default()
        }
    }

    fn 键(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn alt(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::ALT))
    }

    fn ctrl(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::CONTROL))
    }

    fn 鼠标(kind: MouseEventKind, col: u16, row: u16) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            column: col,
            row,
            modifiers: KeyModifiers::NONE,
        })
    }

    #[test]
    fn 一比一下挪一格同步文字且存读一致() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.view.zoom = 1.0;
        app.jump_to(app.doc.box_idx("b0").unwrap());
        app.handle(ctrl(KeyCode::Char('l')));
        assert_eq!(app.doc.boxes[0].at, (1, 0), "框该右移一格");
        assert_eq!(app.doc.text(0).unwrap().at, (2, 1), "文字跟着框走");
        let path = 临时路径("roundtrip", "gtui");
        app.save_document(&path).expect("存盘");
        let body = std::fs::read_to_string(&path).expect("读回");
        let back = Document::load(&body).expect("解析");
        assert_eq!(back.boxes, app.doc.boxes);
        assert_eq!(back.texts, app.doc.texts);
        assert_eq!(back.edges, app.doc.edges);
        assert_eq!(back.groups, app.doc.groups);
        assert_eq!(back.layers, app.doc.layers);
        assert_eq!(back.view, app.doc.view);
        let _ = std::fs::remove_file(&path);
    }

    /// 单击就把光标放到点到的这一格(不吸到节点的角上),也不会挪动框
    #[test]
    fn 单击落下光标() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.view.zoom = 1.0;
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        let p = render::panels(&app.doc, app.area, &app.view);
        // 点在框内部(中间那行)、不是边框,也不是文字起点
        let (x, y) = p.proj.cell((4.0, 1.0));
        let before = app.doc.boxes[0].at;
        app.handle(鼠标(
            MouseEventKind::Down(MouseButton::Left),
            x as u16,
            y as u16,
        ));
        app.handle(鼠标(
            MouseEventKind::Up(MouseButton::Left),
            x as u16,
            y as u16,
        ));
        assert_eq!(app.view.cursor, (4, 1), "光标就是点到的那一格");
        assert_eq!(app.view.pick.box_at(), Some(0), "点到的还是这个框");
        assert_eq!(app.doc.boxes[0].at, before, "单击不改几何");
        assert!(app.view.sel.is_none(), "单击不起选区");
    }

    /// 选区已经确认之后:再拖一次换成新选区;点它外面取消,点里面保留
    #[test]
    fn 选区确认后再拖或点外面() {
        use crossterm::event::{MouseButton, MouseEventKind};
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        let p = render::panels(&app.doc, app.area, &app.view);
        let at = |c: (i32, i32)| p.proj.cell((c.0 as f64, c.1 as f64));

        // 第一块:从空白处起手,盖住 b0(按在框上是挪框)
        let (ax, ay) = at((-2, 8));
        let (bx, by) = at((5, 2));
        app.handle(鼠标(
            MouseEventKind::Down(MouseButton::Left),
            ax as u16,
            ay as u16,
        ));
        app.handle(鼠标(
            MouseEventKind::Drag(MouseButton::Left),
            bx as u16,
            by as u16,
        ));
        app.handle(鼠标(
            MouseEventKind::Up(MouseButton::Left),
            bx as u16,
            by as u16,
        ));
        let first = app.view.sel.expect("第一块选区");
        assert!(first.confirmed);

        // 在里面点一下:选区留着(挑一格在选区矩形里、又不在框上的)
        let (cx, cy) = at((2, 4));
        app.handle(鼠标(
            MouseEventKind::Down(MouseButton::Left),
            cx as u16,
            cy as u16,
        ));
        app.handle(鼠标(
            MouseEventKind::Up(MouseButton::Left),
            cx as u16,
            cy as u16,
        ));
        assert!(app.view.sel.is_some(), "点选区里面不该取消");

        // 点它外面:取消
        // 地图里面、但离选区很远的一格(注意别点到状态行)
        let (ox, oy) = at((30, 9));
        app.handle(鼠标(
            MouseEventKind::Down(MouseButton::Left),
            ox as u16,
            oy as u16,
        ));
        app.handle(鼠标(
            MouseEventKind::Up(MouseButton::Left),
            ox as u16,
            oy as u16,
        ));
        assert!(app.view.sel.is_none(), "点选区外面该取消");

        // 再拖一块别的:直接换成新选区(旧的不能挡着)
        let (dx, dy) = at((10, 8));
        let (ex, ey) = at((16, 2));
        app.handle(鼠标(
            MouseEventKind::Down(MouseButton::Left),
            dx as u16,
            dy as u16,
        ));
        app.handle(鼠标(
            MouseEventKind::Drag(MouseButton::Left),
            ex as u16,
            ey as u16,
        ));
        app.handle(鼠标(
            MouseEventKind::Up(MouseButton::Left),
            ex as u16,
            ey as u16,
        ));
        let second = app.view.sel.expect("第二块选区");
        assert!(second.confirmed, "松手还是即确认");
        assert_eq!(second.anchor, (10, 8), "锚点换成了新按下的地方");
        assert_ne!(second.anchor, first.anchor, "不是旧的那一块");
    }

    /// 空处拖出来的选区:松开就确认,可以直接接着改(不用再按 Enter)
    #[test]
    fn 空处拖完松手就能直接改() {
        use crossterm::event::{MouseButton, MouseEventKind};
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        let p = render::panels(&app.doc, app.area, &app.view);
        // 从空白处起手(按在框上是挪框)
        let (ax, ay) = p.proj.cell((-2.0, 8.0));
        let (bx, by) = p.proj.cell((5.0, 2.0));
        app.handle(鼠标(
            MouseEventKind::Down(MouseButton::Left),
            ax as u16,
            ay as u16,
        ));
        app.handle(鼠标(
            MouseEventKind::Drag(MouseButton::Left),
            bx as u16,
            by as u16,
        ));
        app.handle(鼠标(
            MouseEventKind::Up(MouseButton::Left),
            bx as u16,
            by as u16,
        ));
        let sel = app.view.sel.expect("该有选区");
        assert!(sel.confirmed, "松手即确认");
        let note = app.view.note.clone().unwrap_or_default();
        assert!(note.contains("已确认"), "回显要说一声:{note}");
        // 接着就能直接改,不会被"先按 Enter 确认"拦住
        app.cmd_apply("box double");
        assert!(
            app.doc.boxes[0].style == BoxStyle::Double,
            "拖完的选区该能直接改:{}",
            app.view.note.clone().unwrap_or_default()
        );
    }

    /// 两条边压在同一格上时,连点这一格就在它们之间轮(否则永远只能选到一条)
    #[test]
    fn 叠在同一格的两条边点得到() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        // 给两条边同一组手工折点:它们会走同一条路,互相压着
        app.doc.edges[0].via = vec![(8, 8)];
        app.doc.edges[1].via = vec![(8, 8)];
        app.doc.rebuild();
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        let p = render::panels(&app.doc, app.area, &app.view);
        // 找一格压着两条边、而且不在框上的
        let cell = app
            .hit
            .candidates
            .iter()
            .filter(|(_, v)| v.len() > 1)
            .map(|(c, _)| *c)
            .find(|c| render::node_at(&app.doc, &p, app.area, c.0 as u16, c.1 as u16).is_none())
            .expect("该有一格压着两条边");
        let (cx, cy) = cell;
        let cands = app.hit.candidates_at(cell).to_vec();
        assert!(cands.len() > 1, "这一格该压着两条边:{cands:?}");
        let at = |kind| {
            Event::Mouse(MouseEvent {
                kind,
                column: cx as u16,
                row: cy as u16,
                modifiers: KeyModifiers::NONE,
            })
        };
        app.handle(at(MouseEventKind::Down(MouseButton::Left)));
        let first = app.view.pick.edge().expect("先选到一条");
        app.handle(at(MouseEventKind::Up(MouseButton::Left)));
        app.handle(at(MouseEventKind::Down(MouseButton::Left)));
        let second = app.view.pick.edge().expect("再点一下选到下一条");
        assert_ne!(first, second, "连点同一格该在两条边之间轮");
    }

    /// 空白处双击 = 按 i:弹新建节点的输入框;节点上双击 = 改文字
    #[test]
    fn 双击空处新建节点() {
        use crossterm::event::{MouseButton, MouseEventKind};
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        let p = render::panels(&app.doc, app.area, &app.view);
        // 空白处:样例的框都在最上面两行,底下是空的(别落到状态行上)
        let (x, y) = p.proj.cell((8.0, 8.0));
        assert!(y < (p.map.y + p.map.height) as i32, "测试点该在地图里");
        let click = |app: &mut App| {
            app.handle(鼠标(
                MouseEventKind::Down(MouseButton::Left),
                x as u16,
                y as u16,
            ));
            app.handle(鼠标(
                MouseEventKind::Up(MouseButton::Left),
                x as u16,
                y as u16,
            ));
        };
        click(&mut app);
        assert!(app.view.prompt.is_none(), "单击不该弹输入框");
        click(&mut app);
        assert!(
            matches!(app.view.prompt, Some((Prompt::NewBox, _))),
            "双击空处该和按 i 一样弹新建:{:?}",
            app.view.prompt.is_some()
        );
        assert_eq!(app.view.cursor, p.proj.canvas(x, y), "光标落在双击那一格");
    }

    /// 选区里的东西被删光之后,空选区自动取消(和点选区外面一个效果)
    #[test]
    fn 选区内容没了就自动取消() {
        use crossterm::event::{MouseButton, MouseEventKind};
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        let p = render::panels(&app.doc, app.area, &app.view);
        let at = |c: (i32, i32)| p.proj.cell((c.0 as f64, c.1 as f64));
        // 从空处拉一块盖住 b0
        let (ax, ay) = at((-2, 8));
        let (bx, by) = at((5, 2));
        app.handle(鼠标(
            MouseEventKind::Down(MouseButton::Left),
            ax as u16,
            ay as u16,
        ));
        app.handle(鼠标(
            MouseEventKind::Drag(MouseButton::Left),
            bx as u16,
            by as u16,
        ));
        app.handle(鼠标(
            MouseEventKind::Up(MouseButton::Left),
            bx as u16,
            by as u16,
        ));
        assert!(app.view.sel.is_some(), "先有一块选区");

        // 把选区里的框删掉:选区自动消失
        app.handle(键(KeyCode::Char('d')));
        app.draw(&mut buf, app.area);
        assert!(app.view.sel.is_none(), "选区空了就该自己取消");
        assert_eq!(app.doc.boxes.len(), 2, "只删掉被选中的那个框");
    }

    /// 在节点上拖:挪框(整段拖动算一步撤销);空处拖才是拉选区
    #[test]
    fn 在节点上拖是挪框() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.view.zoom = 1.0;
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        let p = render::panels(&app.doc, app.area, &app.view);
        // 框内部起拖:仍然是挪框(边框起拖才是连边)
        let (x, y) = p.proj.cell((4.0, 1.0));
        let before = app.doc.boxes[0].at;
        let undo_before = app.undo.counts();
        app.handle(鼠标(
            MouseEventKind::Down(MouseButton::Left),
            x as u16,
            y as u16,
        ));
        assert!(app.view.sel.is_none(), "按下不该起选区");
        app.handle(鼠标(
            MouseEventKind::Drag(MouseButton::Left),
            (x + 2) as u16,
            (y + 1) as u16,
        ));
        app.handle(鼠标(
            MouseEventKind::Up(MouseButton::Left),
            (x + 2) as u16,
            (y + 1) as u16,
        ));
        assert_eq!(
            app.doc.boxes[0].at,
            (before.0 + 2, before.1 + 1),
            "拖了两格就该挪两格"
        );
        assert!(app.view.sel.is_none(), "拖框不该留下选区");
        assert_eq!(app.undo.counts().0, undo_before.0 + 1, "整段拖动算一步");
        app.undo_step();
        assert_eq!(app.doc.boxes[0].at, before, "一步撤销回到原位");
    }

    /// `:e` 不带参数:弹出选择浮层,过滤后回车打开
    #[test]
    fn 冒号e不带参数弹出选择器并过滤打开() {
        let one = 临时路径("pick-one", "gtui");
        let two = 临时路径("pick-two", "gtui");
        let body = 样例文档().save();
        std::fs::write(&one, &body).expect("写 gtui");
        std::fs::write(&two, &body).expect("写 gtui");
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.source = Some(one.clone());
        // :e 不带参数
        for c in ":e".chars() {
            app.handle(键(KeyCode::Char(c)));
        }
        app.handle(键(KeyCode::Enter));
        let pick = app.view.file_pick.as_ref().expect("该弹出选择浮层");
        assert!(pick.items.len() >= 2, "该列出候选:{:?}", pick.items);
        // 输入过滤词(文件名本身)缩小到一个候选
        let stem = std::path::Path::new(&two)
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string();
        for c in stem.chars() {
            app.handle(键(KeyCode::Char(c)));
        }
        assert_eq!(app.view.file_pick.as_ref().unwrap().shown().len(), 1);
        // 回车打开
        app.handle(键(KeyCode::Enter));
        assert!(app.view.file_pick.is_none(), "打开后该关掉浮层");
        assert_eq!(app.source.as_deref(), Some(two.as_str()));
        assert!(app.view.note.as_deref().unwrap_or("").contains("opened"));
        let _ = std::fs::remove_file(&one);
        let _ = std::fs::remove_file(&two);
    }

    /// 规格:不在 1:1 时"先切到 1:1,再执行",不是拒绝执行
    #[test]
    fn 缩放不是一比一时先切回一比一再执行() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.view.zoom = 2.0;
        app.handle(ctrl(KeyCode::Char('l')));
        assert_eq!(app.view.zoom, 1.0, "修改前该切回 1:1");
        assert_eq!(app.doc.boxes[0].at, (1, 0), "切回 1:1 之后这一下就生效");
        assert_eq!(app.doc.text(0).unwrap().at, (2, 1), "文字同量平移");
        assert!(app.view.note.is_some(), "该给一句回显");
    }

    /// i:有选中就改文字(整批),空处才是新建
    /// w / b:射线跳。横排图:w 向右、b 向左;撞不到就只提示,光标不动
    #[test]
    fn w_b_射线跳到下一个框或边() {
        let mut app = 连边应用(横排文档(3));
        app.jump_to(0);
        let start = app.view.cursor;
        // 框 0 在 (0,0)-(5,2),下一个东西在右边
        app.handle(键(KeyCode::Char('w')));
        assert!(
            app.view.cursor.0 > start.0,
            "w 该往右走:{start:?} -> {:?}",
            app.view.cursor
        );
        let first = app.view.cursor;
        // 再按一次继续往右,不会停在原地
        app.handle(键(KeyCode::Char('w')));
        assert!(app.view.cursor.0 > first.0, "第二次 w 还要往前走");
        // 到头了:提示,光标不动
        let at_end = app.view.cursor;
        app.handle(键(KeyCode::Char('w')));
        assert_eq!(app.view.cursor, at_end, "没东西可跳时光标不动");
        assert!(
            app.view
                .note
                .clone()
                .unwrap_or_default()
                .contains("前面没有"),
            "{:?}",
            app.view.note
        );
        // b 往回走
        app.handle(键(KeyCode::Char('b')));
        assert!(app.view.cursor.0 < at_end.0, "b 该往左走");
        // 一直 b 到最左边,再按就提示
        for _ in 0..20 {
            app.handle(键(KeyCode::Char('b')));
        }
        assert!(
            app.view
                .note
                .clone()
                .unwrap_or_default()
                .contains("后面没有"),
            "{:?}",
            app.view.note
        );
    }

    /// S:在光标行与下一行之间拆框;最后一行 / 点节点 / 注释框拒绝
    #[test]
    fn 拆分_在光标行下面切一刀() {
        let mut app = 连边应用(横排文档(1));
        let i = 0usize;
        app.doc.set_text(i, "a\nb\nc");
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        let at = app.doc.text(i).unwrap().at;
        app.view.cursor = at;
        app.refresh_pick();
        let n0 = app.doc.boxes.len();
        let undo = app.undo.counts().0;
        app.handle(键(KeyCode::Char('S')));
        assert_eq!(app.doc.boxes.len(), n0 + 1, "拆出一个新框");
        assert_eq!(app.doc.text(i).unwrap().text, "a", "第 0 行留在原框");
        let new = app.doc.boxes.len() - 1;
        assert_eq!(app.doc.text(new).unwrap().text, "b\nc", "其余进新框");
        assert!(
            app.doc.boxes[new].at.0 > app.doc.boxes[i].at.0,
            "新框在右边"
        );
        assert_eq!(app.undo.counts().0, undo + 1, "一步撤销");
        assert!(app.doc.edges.is_empty(), "新框不连边");

        // 光标在最后一行:提示,不压撤销
        let undo = app.undo.counts().0;
        let n0 = app.doc.boxes.len();
        app.view.cursor = (at.0, at.1);
        app.refresh_pick();
        app.handle(键(KeyCode::Char('S')));
        assert_eq!(app.undo.counts().0, undo, "没东西可拆就不压撤销");
        assert_eq!(app.doc.boxes.len(), n0);
        assert!(app
            .view
            .note
            .clone()
            .unwrap_or_default()
            .contains("最后一行"));
    }

    /// M:把选中的框并成一个:文字按阅读顺序拼,内部边删掉,边界边改接到目标框
    #[test]
    fn 合并_选中的多个框() {
        let mut app = 连边应用(横排文档(3));
        app.doc
            .add_edge("b0", "b1", "g0", Dir::Forward)
            .expect("内部边");
        app.doc
            .add_edge("b1", "b2", "g0", Dir::Forward)
            .expect("边界边");
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        // 选中前两个框
        app.view.cursor = (0, 0);
        app.handle(键(KeyCode::Char('v')));
        app.view.cursor = (13, 2);
        app.handle(键(KeyCode::Enter));
        let undo = app.undo.counts().0;
        app.handle(键(KeyCode::Char('M')));
        assert_eq!(app.doc.boxes.len(), 2, "两个框并成一个");
        let merged = app
            .doc
            .texts
            .iter()
            .find(|t| t.text == "n0\nn1")
            .expect("文字按阅读顺序接起来");
        assert_eq!(merged.box_id, "b1", "目标框 = 光标所在的选中框(b1)");
        assert_eq!(app.doc.edges.len(), 1, "内部边没了,边界边还在");
        assert_eq!(app.doc.edges[0].from, "b1", "边界边接在目标框上");
        assert_eq!(app.doc.edges[0].to, "b2");
        assert_eq!(app.undo.counts().0, undo + 1, "一步撤销");
        app.undo_step();
        assert_eq!(app.doc.boxes.len(), 3, "撤销能整个还原");
        assert_eq!(app.doc.edges.len(), 2);

        // 少于两个框:只提示
        app.view.sel = None;
        app.jump_to(0);
        let undo = app.undo.counts().0;
        app.handle(键(KeyCode::Char('M')));
        assert_eq!(app.undo.counts().0, undo);
        assert!(app.view.note.clone().unwrap_or_default().contains("至少"));
    }

    /// y:只复制不改文档;边界边不进剪贴板
    #[test]
    fn 复制_不改文档且跳过边界边() {
        let mut app = 连边应用(横排文档(2));
        app.doc
            .add_edge("b0", "b1", "g0", Dir::Forward)
            .expect("加边");
        app.jump_to(0);
        let (boxes, edges) = (app.doc.boxes.len(), app.doc.edges.len());
        let undo = app.undo.counts().0;
        app.handle(键(KeyCode::Char('y')));
        assert_eq!((app.doc.boxes.len(), app.doc.edges.len()), (boxes, edges));
        assert_eq!(app.undo.counts().0, undo, "y 不压撤销");
        let clip = app.clipboard.clone().expect("剪贴板");
        assert_eq!(clip.boxes.len(), 1);
        assert!(clip.edges.is_empty(), "边界边不进剪贴板");
        assert!(app
            .view
            .note
            .clone()
            .unwrap_or_default()
            .starts_with("yanked 1 boxes"));

        // 光标在空白处:提示
        app.view.cursor = (-20, -20);
        app.refresh_pick();
        app.handle(键(KeyCode::Char('y')));
        assert!(app
            .view
            .note
            .clone()
            .unwrap_or_default()
            .contains("没有可复制"));
    }

    /// i 只动光标下的一个框:区块选择时只提示,不改任何文字
    #[test]
    fn i_只改光标下的一个框() {
        let mut app = 连边应用(横排文档(2));
        app.view.sel = Some(RectSel {
            anchor: (-2, -2),
            end: (14, 5),
            confirmed: true,
        });
        let before: Vec<String> = app.doc.texts.iter().map(|t| t.text.clone()).collect();
        app.handle(键(KeyCode::Char('i')));
        assert!(app.view.edit.is_none(), "区块选择时不进编辑");
        assert!(app.view.note.clone().unwrap_or_default().contains("一个框"));
        let after: Vec<String> = app.doc.texts.iter().map(|t| t.text.clone()).collect();
        assert_eq!(before, after, "一个字都不改");

        // 取消选区后:i 只编辑光标下的那个框,别的框不动
        app.view.sel = None;
        app.jump_to(0);
        app.handle(键(KeyCode::Char('i')));
        let edit = app.view.edit.clone().expect("该进就地编辑");
        assert_eq!(edit.box_idx, 0);
        let _ = app;
    }

    #[test]
    fn i_在框里直接改字() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        let n0 = app.doc.boxes.len();
        // 光标压在文字的第一个字上:i 就地编辑,光标就在那个字上(不是末尾)
        let at = app.doc.text(0).unwrap().at;
        app.view.cursor = at;
        app.refresh_pick();
        app.handle(键(KeyCode::Char('i')));
        let edit = app.view.edit.clone().expect("该进就地编辑");
        assert_eq!(edit.box_idx, 0);
        assert_eq!(edit.text, "甲", "草稿从原文开始");
        assert_eq!(edit.caret, 0, "光标落在画布光标指到的那个字上");
        assert!(app.view.prompt.is_none(), "不弹底行输入框");
        // End 到行尾,再打两个字
        app.handle(键(KeyCode::End));
        assert_eq!(app.view.edit.as_ref().map(|e| e.caret), Some(1));
        for c in "XY".chars() {
            app.handle(键(KeyCode::Char(c)));
        }
        assert_eq!(
            app.view.edit.as_ref().map(|e| e.text.as_str()),
            Some("甲XY")
        );
        assert_eq!(app.doc.text(0).unwrap().text, "甲", "提交前文档不变");
        app.handle(键(KeyCode::Esc));
        assert!(app.view.edit.is_none());
        assert_eq!(app.doc.text(0).unwrap().text, "甲XY");
        assert_eq!(app.doc.boxes.len(), n0, "不该新建框");
        assert_eq!(app.undo.counts().0, 1, "改文字一步撤销");
        app.undo_step();
        assert_eq!(app.doc.text(0).unwrap().text, "甲");

        // Ctrl-C 取消:文档不动
        app.handle(键(KeyCode::Char('i')));
        app.handle(键(KeyCode::Char('X')));
        app.handle(ctrl(KeyCode::Char('c')));
        assert!(app.view.edit.is_none());
        assert_eq!(app.doc.text(0).unwrap().text, "甲", "取消不写回");
        assert!(app.view.note.clone().unwrap_or_default().contains("取消"));

        // 换行:Enter 插一个换行,提交后框变高
        app.handle(键(KeyCode::Char('i')));
        app.handle(键(KeyCode::End));
        app.handle(键(KeyCode::Enter));
        app.handle(键(KeyCode::Char('乙')));
        app.handle(键(KeyCode::Esc));
        assert_eq!(app.doc.text(0).unwrap().text, "甲\n乙");
        assert!(app.doc.boxes[0].size.1 >= 2, "框跟着长高");

        // 空白处按 i:直接建框并进编辑;Ctrl-C 把框整步撤掉
        app.view.cursor = (-20, -20);
        app.refresh_pick();
        let n1 = app.doc.boxes.len();
        app.handle(键(KeyCode::Char('i')));
        assert!(app.view.edit.is_some(), "空白处也进就地编辑");
        assert_eq!(app.doc.boxes.len(), n1 + 1, "先建了一个空框");
        app.handle(ctrl(KeyCode::Char('c')));
        assert_eq!(app.doc.boxes.len(), n1, "取消时新建的框一起撤掉");

        // 空白处按 i 打几个字再 Esc:框里就是这些字
        app.view.cursor = (-20, -20);
        app.refresh_pick();
        app.handle(键(KeyCode::Char('i')));
        for c in "hello".chars() {
            app.handle(键(KeyCode::Char(c)));
        }
        app.handle(键(KeyCode::Esc));
        assert_eq!(
            app.doc.texts.last().map(|t| t.text.as_str()),
            Some("hello"),
            "新框里是刚打的字"
        );
    }

    /// 光标移到边的笔画上就该选中那条边(以前被 sync_edge_sel 清掉了)
    #[test]
    fn 光标落在边上选中边() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        // 找一条边的可见格里、换算回画布后不属于任何框的那一格
        let p = render::panels(&app.doc, app.area, &app.view);
        let mut target = None;
        for (e, cells) in app.hit.edge_cells.iter() {
            for c in cells {
                let canvas = p.proj.canvas(c.0, c.1);
                if app.doc.box_at_cell(canvas).is_none() {
                    target = Some((*e, canvas));
                    break;
                }
            }
            if target.is_some() {
                break;
            }
        }
        let (e, cell) = target.expect("样例该有边笔画");
        app.view.cursor = cell;
        app.refresh_pick();
        assert!(
            app.view.pick.edge().is_some(),
            "光标在边上该选中边(不一定是 {e},同格可能有多条)"
        );
        let picked = app.view.pick;
        // 走一次不改光标的按键,选中项不该被清掉
        app.handle(键(KeyCode::Char('B')));
        assert_eq!(app.view.pick, picked, "按键后仍选中同一条边");
        // 移开一格就不再是它
        app.handle(键(KeyCode::Left));
        app.handle(键(KeyCode::Left));
        app.handle(键(KeyCode::Up));
        app.handle(键(KeyCode::Up));
        assert_ne!(app.view.pick, picked, "移开就该换选中项");
    }

    /// e 把选中的框按顺序连成边,只算一步撤销
    /// n 个框横排、一个分组、没有任何边
    fn 横排文档(n: usize) -> Document {
        let groups = vec![Group {
            id: "g0".into(),
            label: "线".into(),
            color: (0x4c, 0x8e, 0xf8),
        }];
        let layers = vec![Layer {
            name: String::new(),
            order: 0,
        }];
        let boxes: Vec<Box> = (0..n)
            .map(|i| Box {
                id: format!("b{i}"),
                at: ((i as i32) * 8, 0),
                size: (6, 3),
                style: BoxStyle::Rounded,
                color: None,
                fill: None,
                layer: String::new(),
                terms: Vec::new(),
                edge: None,
            })
            .collect();
        let texts: Vec<Text> = (0..n)
            .map(|i| Text {
                id: format!("t{i}"),
                text: format!("n{i}"),
                at: ((i as i32) * 8 + 1, 1),
                box_id: format!("b{i}"),
                color: None,
                bg: None,
                align: Align::Left,
                layer: String::new(),
            })
            .collect();
        Document::from_parts(
            "graph-tui 0.2.0",
            groups,
            layers,
            boxes,
            texts,
            Vec::new(),
            ViewState::default(),
        )
        .expect("横排文档")
    }

    fn 连边应用(doc: Document) -> App {
        let mut app = App::new(doc, zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 120, 30);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        app
    }

    /// 直接摆好源/目标,走 link_commit(批量规则的单测入口)
    fn 直连(app: &mut App, sources: Vec<usize>, targets: Vec<usize>) {
        app.view.link = Some(LinkState {
            sources,
            targets,
            confirming: false,
        });
        app.link_commit();
    }

    fn 边名(app: &App) -> Vec<(String, String)> {
        app.doc
            .edges
            .iter()
            .map(|e| (e.from.clone(), e.to.clone()))
            .collect()
    }

    #[test]
    fn 连边_一比一() {
        let mut app = 连边应用(横排文档(2));
        app.jump_to(0);
        app.handle(键(KeyCode::Char('e')));
        assert!(app.view.link.is_some(), "该进入连边模式");
        app.jump_to(1);
        app.handle(键(KeyCode::Enter));
        assert_eq!(边名(&app), vec![("b0".into(), "b1".into())]);
        assert!(app.view.link.is_none(), "建完退出连边模式");
        assert_eq!(app.doc.edges[0].dir, Dir::Forward);
    }

    #[test]
    fn 连边_一对多() {
        let mut app = 连边应用(横排文档(4));
        app.jump_to(0);
        app.handle(键(KeyCode::Char('e')));
        // 目标用 v 框住 1、2、3
        app.handle(键(KeyCode::Char('v')));
        app.view.cursor = app.cell_of(3);
        app.handle(键(KeyCode::Enter));
        let made = 边名(&app);
        // 从 b0 起框,选区含源本身:自环 b0->b0 也建
        assert_eq!(made.len(), 4);
        for (from, to) in made {
            assert_eq!(from, "b0");
            assert!(to == "b0" || to == "b1" || to == "b2" || to == "b3", "{to}");
        }
    }

    #[test]
    fn 连边_多对一() {
        let mut app = 连边应用(横排文档(4));
        // 源:0、1、2 选区
        app.view.cursor = app.cell_of(0);
        app.handle(键(KeyCode::Char('v')));
        app.view.cursor = app.cell_of(2);
        app.handle(键(KeyCode::Enter));
        app.handle(键(KeyCode::Char('e')));
        assert_eq!(app.view.link.as_ref().unwrap().sources, vec![0, 1, 2]);
        app.jump_to(3);
        app.handle(键(KeyCode::Enter));
        assert_eq!(边名(&app).len(), 3);
        assert!(app.doc.edges.iter().all(|e| e.to == "b3"));
    }

    #[test]
    fn 连边_多对多() {
        let mut app = 连边应用(横排文档(5));
        直连(&mut app, vec![0, 1], vec![2, 3, 4]);
        assert_eq!(边名(&app).len(), 6, "2x3 该建 6 条");
    }

    #[test]
    fn 连边_自环与重复() {
        let mut app = 连边应用(横排文档(2));
        直连(&mut app, vec![0, 1], vec![0, 1]);
        // 四对:两条自环(b1->b1、b2->b2)与 0->1、1->0
        assert_eq!(边名(&app).len(), 4, "自环也该建出来");
        assert_eq!(
            app.doc.edges.iter().filter(|e| e.from == e.to).count(),
            2,
            "两条自环"
        );
        // 再来一次:全是重复
        let undo0 = app.undo.counts().0;
        直连(&mut app, vec![0], vec![1]);
        assert_eq!(边名(&app).len(), 4, "重复的不再加");
        assert_eq!(app.undo.counts().0, undo0, "一条没建不压撤销");
    }

    #[test]
    fn 连边_整批一步撤销() {
        let mut app = 连边应用(横排文档(5));
        let undo0 = app.undo.counts().0;
        直连(&mut app, vec![0, 1], vec![2, 3, 4]);
        assert_eq!(app.doc.edges.len(), 6);
        assert_eq!(app.undo.counts().0, undo0 + 1, "整批只算一步");
        app.undo_step();
        assert!(app.doc.edges.is_empty(), "撤一次全部消失");
    }

    #[test]
    fn 连边_esc取消不动文档() {
        let mut app = 连边应用(横排文档(3));
        app.jump_to(0);
        app.handle(键(KeyCode::Char('e')));
        app.jump_to(2);
        app.handle(键(KeyCode::Esc));
        assert!(app.view.link.is_none());
        assert!(app.doc.edges.is_empty(), "取消不该建边");
    }

    #[test]
    fn 连边_源选区未确认先确认() {
        let mut app = 连边应用(横排文档(3));
        app.view.cursor = app.cell_of(0);
        app.handle(键(KeyCode::Char('v')));
        app.view.cursor = app.cell_of(1);
        // 没按 Enter 直接按 e
        app.handle(键(KeyCode::Char('e')));
        assert_eq!(app.view.link.as_ref().unwrap().sources, vec![0, 1]);
        assert!(app.view.sel.is_none(), "源选区交给目标用");
    }

    #[test]
    fn 连边_目标选区未确认也能建() {
        let mut app = 连边应用(横排文档(4));
        app.jump_to(0);
        app.handle(键(KeyCode::Char('e')));
        app.handle(键(KeyCode::Char('v')));
        app.view.cursor = app.cell_of(2);
        // 没确认目标选区,Enter 一气确认 + 建边(选区含源,多一条自环)
        app.handle(键(KeyCode::Enter));
        assert_eq!(边名(&app).len(), 3);
    }

    #[test]
    fn 连边_目标阶段的v不覆盖源() {
        let mut app = 连边应用(横排文档(4));
        app.jump_to(0);
        app.handle(键(KeyCode::Char('e')));
        app.handle(键(KeyCode::Char('v')));
        app.view.cursor = app.cell_of(3);
        assert_eq!(app.view.link.as_ref().unwrap().sources, vec![0]);
    }

    #[test]
    fn 连边_数量大要二次确认() {
        let mut app = 连边应用(横排文档(20));
        // 8 源 x 9 目标 = 72 条:第一次 Enter 只进确认态
        直连(&mut app, (0..8).collect(), (9..18).collect());
        assert!(app.doc.edges.is_empty(), "第一次 Enter 不建");
        let link = app.view.link.clone().expect("还在连边模式");
        assert!(link.confirming);
        let note = app.view.note.clone().unwrap_or_default();
        assert!(note.contains("再按 Enter"), "{note}");
        app.handle(键(KeyCode::Enter));
        assert_eq!(app.doc.edges.len(), 72);
    }

    #[test]
    fn 连边_超过上限直接拒绝() {
        let mut app = 连边应用(横排文档(60));
        直连(&mut app, (0..30).collect(), (30..50).collect());
        assert!(app.doc.edges.is_empty(), "600 条超上限,一条都不建");
        let note = app.view.note.clone().unwrap_or_default();
        assert!(note.contains("512"), "{note}");
        assert!(app.view.link.is_some(), "还在连边模式等缩小选区");
    }

    #[test]
    fn 连边_分组按聚焦或源自己的分组() {
        // 样例文档里 b0 属于 g0、b2 属于 g0;只聚焦 g1 时全体用 g1
        let mut app = 连边应用(样例文档());
        app.view.focus = vec![1];
        直连(&mut app, vec![0], vec![2]);
        assert_eq!(app.doc.edges.last().unwrap().group, "g1");
        // 不聚焦:用源框自己的第一个分组
        let mut app = 连边应用(样例文档());
        直连(&mut app, vec![0], vec![2]);
        assert_eq!(app.doc.edges.last().unwrap().group, "g0");
    }

    #[test]
    fn 连边_shift点击加目标() {
        let mut app = 连边应用(横排文档(4));
        app.jump_to(0);
        app.handle(键(KeyCode::Char('e')));
        let p = render::panels(&app.doc, app.area, &app.view);
        let at: Vec<(u16, u16)> = (0..app.doc.boxes.len())
            .map(|i| {
                let (x, y) = p.proj.cell(app.doc.center(i));
                (x as u16, y as u16)
            })
            .collect();
        let 点 = |app: &mut App, i: usize, shift: bool| {
            app.handle(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: at[i].0,
                row: at[i].1,
                modifiers: if shift {
                    KeyModifiers::SHIFT
                } else {
                    KeyModifiers::NONE
                },
            }));
        };
        // 普通点击:目标就设成这一个
        点(&mut app, 2, false);
        assert_eq!(app.view.link.as_ref().unwrap().targets, vec![2]);
        // Shift 点击:加进来
        点(&mut app, 3, true);
        assert_eq!(app.view.link.as_ref().unwrap().targets, vec![2, 3]);
        // 再 Shift 点一次:移除
        点(&mut app, 3, true);
        assert_eq!(app.view.link.as_ref().unwrap().targets, vec![2]);
    }

    /// 剪贴板里带边时,粘贴的落点要和没边时一样(不能被边的另一头带跑)
    #[test]
    fn 粘贴位置不受边影响() {
        // b 在左上、a 在右下,两者之间有边;删掉 a 会把这条边也带进剪贴板
        let mut app = 连边应用(横排文档(2));
        app.doc
            .add_edge("b0", "b1", "g0", Dir::Forward)
            .expect("加一条边");
        app.doc.move_box(1, -20, 10);
        app.doc.move_box(0, 20, 0);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        let a = app.doc.box_idx("b0").unwrap();
        app.jump_to(a);
        // d = 带边剪切:边从文档里删掉,但它是边界边(另一头还在),不进剪贴板
        app.handle(键(KeyCode::Char('d')));
        assert_eq!(
            app.clipboard.as_ref().map(|c| c.edges.len()).unwrap_or(1),
            0,
            "边界边不进剪贴板"
        );
        app.view.cursor = (20, 0);
        app.handle(键(KeyCode::Char('p')));
        let pasted = app
            .doc
            .boxes
            .iter()
            .find(|b| b.id != "b0" && b.id != "b1")
            .expect("该粘回来一个框");
        assert_eq!(pasted.at, (20, 0), "粘贴落点该是光标处,和边无关");
    }

    #[test]
    fn 连边_删掉源之后按id重新定位() {
        let mut app = 连边应用(横排文档(3));
        app.jump_to(2);
        app.handle(键(KeyCode::Char('e'))); // 源 = b2
        let src = app.doc.boxes[2].id.clone();
        app.view.cursor = app.cell_of(0);
        app.refresh_pick();
        // 源框被删:连边状态该自己退出
        app.doc.remove_box(&src).expect("删源");
        app.doc.rebuild();
        app.handle(键(KeyCode::Enter));
        assert!(app.view.link.is_none(), "源都没了,连边该退出");
    }

    /// d 剪贴板含关联边,x 只含框和文字;两者都把边从文档里删干净
    #[test]
    fn d_带边剪切带走关联边() {
        let mut app = 连边应用(横排文档(2));
        app.doc
            .add_edge("b0", "b1", "g0", Dir::Forward)
            .expect("加边");
        app.jump_to(0);
        app.handle(键(KeyCode::Char('d')));
        assert_eq!(app.doc.edges.len(), 0, "关联边要一起删,不能留悬空");
        let clip = app.clipboard.clone().expect("剪贴板");
        assert_eq!(clip.boxes.len(), 1);
        assert_eq!(clip.texts.len(), 1);
        assert!(clip.edges.is_empty(), "边界边(另一头还在文档里)不进剪贴板");
        // x 不再是绑定键
        let mut app = 连边应用(横排文档(2));
        app.jump_to(0);
        app.handle(键(KeyCode::Char('x')));
        assert!(app.clipboard.is_none(), "x 不该再剪切任何东西");
    }

    /// x 粘出来的内容没有边;d 粘出来的内容带边
    #[test]
    fn 粘贴不带边界边() {
        let mut app = 连边应用(横排文档(2));
        app.doc
            .add_edge("b0", "b1", "g0", Dir::Forward)
            .expect("加边");
        app.jump_to(0);
        app.handle(键(KeyCode::Char('d')));
        app.handle(键(KeyCode::Char('p')));
        assert_eq!(app.doc.edges.len(), 0, "边界边粘不出来(另一端不在片段里)");
    }

    /// 连按 p:三份不会完全重叠,id 全不同
    #[test]
    fn 连按粘贴错开且id不重复() {
        let mut app = 连边应用(横排文档(2));
        app.jump_to(0);
        app.handle(键(KeyCode::Char('d')));
        let mut seen: Vec<(i32, i32)> = Vec::new();
        for _ in 0..3 {
            app.handle(键(KeyCode::Char('p')));
            let b = app.doc.boxes.last().expect("粘出来的框");
            assert!(
                !seen.contains(&b.at),
                "第 {} 份和前面重叠了",
                seen.len() + 1
            );
            seen.push(b.at);
        }
        let ids: std::collections::HashSet<&str> =
            app.doc.boxes.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(ids.len(), app.doc.boxes.len(), "id 该各不相同");
    }

    /// 第一份粘在光标的精确位置,并且选择指向它
    #[test]
    fn 第一份粘贴落在光标处并被选中() {
        let mut app = 连边应用(横排文档(2));
        app.jump_to(0);
        app.handle(键(KeyCode::Char('d')));
        app.view.cursor = (40, 12);
        app.handle(键(KeyCode::Char('p')));
        let i = app.doc.boxes.len() - 1;
        assert_eq!(app.doc.boxes[i].at, (40, 12), "第一份该精确落在光标处");
        assert_eq!(app.view.pick, render::Pick::Box(i), "该选中刚粘的那份");
        assert_eq!(
            app.view.cursor,
            app.cell_of(i),
            "光标该落到刚粘的那份上(下一次命令默认动它)"
        );
    }

    /// 深拷贝:改一份不影响别的;via 也各是各的
    #[test]
    fn 粘贴内容是深拷贝() {
        // 三个框,剪前两个:b0-b1 是内部边(进剪贴板),b1-b2 是边界边(只从文档删)
        let mut app = 连边应用(横排文档(3));
        app.doc
            .add_edge("b0", "b1", "g0", Dir::Forward)
            .expect("加边");
        app.doc
            .add_edge("b1", "b2", "g0", Dir::Forward)
            .expect("再加一条");
        app.view.sel = Some(RectSel {
            anchor: (-2, -2),
            end: (14, 5),
            confirmed: true,
        });
        app.handle(键(KeyCode::Char('d')));
        assert_eq!(app.doc.boxes.len(), 1, "只剩第三个框");
        assert_eq!(app.doc.edges.len(), 0, "边界边一起删掉");
        app.handle(键(KeyCode::Char('p')));
        app.handle(键(KeyCode::Char('p')));
        assert_eq!(app.doc.edges.len(), 2, "两次粘贴各带一条边");
        // 改第一条边的折点,第二条不受影响
        let via = vec![(99, 99)];
        app.doc.edges[0].via = via.clone();
        app.doc.rebuild();
        let other = app
            .doc
            .edges
            .iter()
            .enumerate()
            .find(|(i, _)| *i != 0)
            .map(|(_, e)| e.via.clone())
            .unwrap();
        assert_ne!(other, via, "via 该各自独立");
        // 剪贴板不被后续修改影响
        let clip = app.clipboard.clone().unwrap();
        assert!(clip.edges[0].via.is_empty(), "剪贴板保持删除时的样子");
    }

    /// 主回归:三份里删掉第三份的边,前两份的边还在(不再删错下标)
    #[test]
    fn 删一份的边不影响别的份() {
        // 选区里两个框 + 它们之间的边:这条边是内部的,进剪贴板
        let mut app = 连边应用(横排文档(3));
        app.doc
            .add_edge("b0", "b1", "g0", Dir::Forward)
            .expect("加边");
        app.view.sel = Some(RectSel {
            anchor: (-2, -2),
            end: (14, 5),
            confirmed: true,
        });
        app.handle(键(KeyCode::Char('d')));
        assert_eq!(app.view.sel, None);
        assert_eq!(
            app.clipboard.as_ref().map(|c| c.edges.len()).unwrap_or(0),
            1,
            "内部边进剪贴板"
        );
        for _ in 0..3 {
            app.handle(键(KeyCode::Char('p')));
        }
        assert_eq!(app.doc.edges.len(), 3, "三份各带一条边(边界边已经剪掉)");
        // 删掉最后粘的那份:
        let last = app.doc.boxes.len() - 1;
        app.view.cursor = app.cell_of(last);
        app.refresh_pick();
        app.handle(键(KeyCode::Char('d')));
        assert_eq!(app.doc.edges.len(), 2, "只删掉那一份的边");
    }

    /// 文档改过之后,旧命中表里的边下标不能再当选择用
    #[test]
    fn 旧命中表不能跨修改选边() {
        let mut app = 连边应用(横排文档(2));
        app.doc
            .add_edge("b0", "b1", "g0", Dir::Forward)
            .expect("加边");
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area); // 生成一张命中表
        assert_eq!(app.hit.rev, app.doc.rev(), "命中表和文档同版");
        // 改文档(粘贴一个框)
        app.jump_to(0);
        app.handle(键(KeyCode::Char('d')));
        app.handle(键(KeyCode::Char('p')));
        assert_ne!(app.hit.rev, app.doc.rev(), "旧命中表该作废");
        assert!(app.usable_hit().candidates.is_empty(), "作废后不返回候选");
    }

    /// 重叠的框:取最上面那个,E 能在它们之间轮
    #[test]
    fn 重叠的框取最上面且能轮选() {
        let mut app = 连边应用(横排文档(2));
        // 把第二个框挪到和第一个完全重合
        let at0 = app.doc.boxes[0].at;
        app.doc.move_box(
            1,
            at0.0 - app.doc.boxes[1].at.0,
            at0.1 - app.doc.boxes[1].at.1,
        );
        let cell = (at0.0 + 1, at0.1 + 1);
        assert_eq!(app.doc.box_at_cell(cell), Some(1), "重叠时该取后画的那个");
        let stack = app.doc.boxes_at_cell(cell);
        assert_eq!(stack, vec![1, 0], "从上到下");
        app.view.cursor = cell;
        app.refresh_pick();
        app.handle(ctrl(KeyCode::Char('e')));
        let first = app.view.pick.box_at().expect("该选中一个框");
        app.handle(ctrl(KeyCode::Char('e')));
        let second = app.view.pick.box_at().expect("该选中另一个框");
        assert_ne!(first, second, "E 该在重叠的框之间轮");
    }

    /// 连着删两条边,中间不重绘:第二条不能因为下标前移而删错
    #[test]
    fn 连续删边不错位() {
        let mut app = 连边应用(横排文档(3));
        app.doc
            .add_edge("b0", "b1", "g0", Dir::Forward)
            .expect("e1");
        app.doc
            .add_edge("b1", "b2", "g0", Dir::Forward)
            .expect("e2");
        app.jump_to(1); // 停在中间那个框上,它两边各连着一条
        let mut gone = Vec::new();
        for _ in 0..2 {
            app.handle(ctrl(KeyCode::Char('e')));
            let e = app.view.pick.edge().expect("该选到一条边");
            gone.push(app.doc.edges[e].id.clone());
            app.handle(键(KeyCode::Char('d')));
        }
        assert!(app.doc.edges.is_empty(), "两条边该都删掉");
        assert_ne!(gone[0], gone[1], "两次删的该是两条不同的边");
    }

    /// 粘贴/删除各一步撤销;撤销重做不动剪贴板
    #[test]
    fn 粘贴单步撤销且不动剪贴板() {
        let mut app = 连边应用(横排文档(2));
        app.doc
            .add_edge("b0", "b1", "g0", Dir::Forward)
            .expect("加边");
        app.jump_to(0);
        app.handle(键(KeyCode::Char('d')));
        let clip_after_cut = app.clipboard.clone().unwrap();
        let undo_after_cut = app.undo.counts().0;
        app.handle(键(KeyCode::Char('p')));
        assert_eq!(app.undo.counts().0, undo_after_cut + 1, "粘贴一步撤销");
        assert_eq!(
            app.clipboard.as_ref().map(|c| c.boxes.len()),
            Some(clip_after_cut.boxes.len()),
            "粘贴不改剪贴板"
        );
        let boxes_before = app.doc.boxes.len();
        app.handle(键(KeyCode::Char('u')));
        assert_eq!(app.doc.boxes.len(), boxes_before - 1, "撤销掉这次粘贴");
        assert_eq!(
            app.clipboard.as_ref().map(|c| c.boxes.len()),
            Some(clip_after_cut.boxes.len()),
            "撤销不改剪贴板"
        );
        app.handle(ctrl(KeyCode::Char('r')));
        assert_eq!(app.doc.boxes.len(), boxes_before, "重做回来");
    }

    /// 选区里的边:d 复制进剪贴板,x 只删不复制
    #[test]
    fn 选区里的边_剪贴板只收内部边() {
        let mut app = 连边应用(横排文档(3));
        app.doc
            .add_edge("b0", "b1", "g0", Dir::Forward)
            .expect("加边");
        // 框住前两个框(它们的边是内部边)
        app.view.cursor = (0, 0);
        app.handle(键(KeyCode::Char('v')));
        app.view.cursor = (13, 2);
        app.handle(键(KeyCode::Enter));
        app.handle(键(KeyCode::Char('d')));
        let clip = app.clipboard.clone().expect("剪贴板");
        assert_eq!(clip.boxes.len(), 2, "两个框");
        assert_eq!(clip.edges.len(), 1, "内部边进剪贴板");

        // 边界边:只选一个框,边从文档删掉但不进剪贴板
        let mut app = 连边应用(横排文档(2));
        app.doc
            .add_edge("b0", "b1", "g0", Dir::Forward)
            .expect("加边");
        app.jump_to(0);
        app.handle(键(KeyCode::Char('d')));
        let clip = app.clipboard.clone().expect("剪贴板");
        assert_eq!(clip.boxes.len(), 1);
        assert!(clip.edges.is_empty(), "边界边不进剪贴板");
        assert_eq!(app.doc.edges.len(), 0, "但文档里的边已经删掉");
    }

    /// 删除之后光标留在原地(不跳到最近的框上)
    #[test]
    fn 删除后光标不动() {
        let mut app = 连边应用(横排文档(3));
        app.jump_to(1);
        let before = app.view.cursor;
        app.handle(键(KeyCode::Char('d')));
        assert_eq!(app.view.cursor, before, "光标该留在原地");
        assert!(
            app.doc.box_at_cell(before).is_none(),
            "那个框已经删掉了,光标格上应该空了"
        );
    }

    #[test]
    fn 连边_右键菜单能确认和取消() {
        // 确认连边
        let mut app = 连边应用(横排文档(2));
        app.jump_to(0);
        app.handle(键(KeyCode::Char('e')));
        app.jump_to(1);
        app.open_menu(40, 10);
        let menu = app.view.menu.clone().expect("该弹出菜单");
        let titles: Vec<&str> = menu.level().iter().map(|i| i.title.as_str()).collect();
        assert!(titles.contains(&"确认连边"), "{titles:?}");
        app.menu_choose();
        assert_eq!(边名(&app), vec![("b0".into(), "b1".into())]);
        // 取消连边
        let mut app = 连边应用(横排文档(2));
        app.jump_to(0);
        app.handle(键(KeyCode::Char('e')));
        app.open_menu(40, 10);
        app.view.menu.as_mut().unwrap().sel = 1;
        app.menu_choose();
        assert!(app.view.link.is_none());
        assert!(app.doc.edges.is_empty());
    }

    #[test]
    fn 连边_要求一比一() {
        let mut app = 连边应用(横排文档(2));
        app.view.zoom = 2.0;
        app.jump_to(0);
        app.handle(键(KeyCode::Char('e')));
        assert!((app.view.zoom - 1.0).abs() < 1e-9, "连边前先切回 1:1");
    }

    /// s:框上开框菜单,边上开边菜单,空处只提示
    #[test]
    fn s_按类型打开样式菜单() {
        let mut app = 连边应用(横排文档(2));
        app.doc
            .add_edge("b0", "b1", "g0", Dir::Forward)
            .expect("加边");
        app.jump_to(0);
        app.handle(键(KeyCode::Char('s')));
        let menu = app.view.menu.clone().expect("框上该开菜单");
        assert_eq!(menu.level()[0].title, "框型");
        assert!(matches!(menu.level()[0].action, MenuAction::Sub(_)));
        app.handle(键(KeyCode::Esc));

        // 边上:先选到边
        app.handle(ctrl(KeyCode::Char('e')));
        assert!(app.view.pick.edge().is_some(), "Ctrl-e 该选到边");
        app.handle(键(KeyCode::Char('s')));
        let menu = app.view.menu.clone().expect("边上该开菜单");
        assert_eq!(menu.level()[0].title, "线型");
        app.handle(键(KeyCode::Esc));

        // 空处:不弹菜单,只提示
        app.view.cursor = (-50, -50);
        app.refresh_pick();
        app.handle(键(KeyCode::Char('s')));
        assert!(app.view.menu.is_none());
        let note = app.view.note.clone().unwrap_or_default();
        assert!(note.contains("没有可改样式"), "{note}");
    }

    /// 新建边照抄"最近选中的那条边"的分组/线型/拐角/颜色
    #[test]
    fn 新建边照抄最近选中的边() {
        let mut app = 连边应用(横排文档(3));
        let e0 = app
            .doc
            .add_edge("b0", "b1", "g0", Dir::Forward)
            .expect("加边");
        app.doc.edges[e0].style = crate::model::EdgeStyle::Dashed;
        app.doc.edges[e0].corner = crate::model::Corner::Sharp;
        app.doc.edges[e0].color = Some((9, 8, 7));
        app.doc.edges[e0].bg = Some((1, 1, 1));
        app.doc.rebuild();
        // 先记下这条边的样式,再建新边
        app.view.pick = render::Pick::Edge(e0);
        app.remember_look();
        assert!(app.edge_look.is_some(), "选中边该记下样式");

        直连(&mut app, vec![1], vec![2]);
        let e1 = app
            .doc
            .edges
            .iter()
            .position(|e| e.from == "b1" && e.to == "b2")
            .expect("建了新边");
        assert_eq!(app.doc.edges[e1].style, crate::model::EdgeStyle::Dashed);
        assert_eq!(app.doc.edges[e1].corner, crate::model::Corner::Sharp);
        assert_eq!(app.doc.edges[e1].color, Some((9, 8, 7)));
        assert_eq!(app.doc.edges[e1].bg, Some((1, 1, 1)));
        assert_eq!(app.doc.edges[e1].group, "g0", "分组也照抄");

        // 区块选区时回到默认样式
        app.view.sel = Some(RectSel {
            anchor: (0, 0),
            end: (3, 3),
            confirmed: true,
        });
        直连(&mut app, vec![2], vec![0]);
        let e2 = app
            .doc
            .edges
            .iter()
            .position(|e| e.from == "b2" && e.to == "b0")
            .expect("又建了一条");
        assert_eq!(app.doc.edges[e2].style, crate::model::EdgeStyle::Solid);
        assert_eq!(app.doc.edges[e2].color, None);
    }

    /// 新建框照抄"最近选中的那个框"的样式/颜色/对齐;区块选区时不照抄
    #[test]
    fn 新建框照抄最近选中的框() {
        let mut app = 连边应用(横排文档(2));
        app.doc.boxes[0].style = BoxStyle::Double;
        app.doc.boxes[0].color = Some((10, 20, 30));
        app.doc.boxes[0].fill = Some((1, 2, 3));
        let ti = app.doc.text_of(0).unwrap();
        app.doc.texts[ti].color = Some((200, 100, 50));
        app.doc.texts[ti].bg = Some((4, 5, 6));
        app.doc.texts[ti].align = Align::Right;
        app.doc.rebuild();
        app.view.cursor = app.cell_of(0);
        app.refresh_pick();
        assert_eq!(app.view.pick, render::Pick::Box(0));

        app.cmd_apply("new 下一个");
        let i = app.doc.boxes.len() - 1;
        assert_eq!(app.doc.boxes[i].style, BoxStyle::Double, "照抄框型");
        assert_eq!(app.doc.boxes[i].color, Some((10, 20, 30)));
        assert_eq!(app.doc.boxes[i].fill, Some((1, 2, 3)));
        let t = app.doc.text(i).unwrap();
        assert_eq!(t.color, Some((200, 100, 50)));
        assert_eq!(t.bg, Some((4, 5, 6)));
        assert_eq!(t.align, Align::Right);

        // 区块选区:不照抄,回到默认(只有文字)
        app.view.sel = Some(RectSel {
            anchor: (0, 0),
            end: (2, 2),
            confirmed: true,
        });
        app.cmd_apply("new 选区里的");
        let j = app.doc.boxes.len() - 1;
        assert_eq!(app.doc.boxes[j].style, BoxStyle::None, "选一片时不照抄");
        assert_eq!(app.doc.boxes[j].color, None);
        assert_eq!(app.doc.text(j).unwrap().align, Align::Left);
    }

    /// 底行输入框:i 预填原文,光标在末尾;Alt-B/F 与 Alt-左右按词跳;emacs 基本键照旧
    #[test]
    fn 输入框预填原文且能按词跳() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        app.jump_to(0);
        // 底行输入框:直接打开(默认键位里已经没有开它的键了,用 :text 或菜单)
        app.open_prompt(Prompt::Text(vec![0]), app.common_text(&[0]));
        match app.view.prompt.clone() {
            Some((Prompt::Text(_), text)) => assert_eq!(text, "甲", "预填原文"),
            other => panic!("该是改文字:{other:?}"),
        }
        assert_eq!(app.view.prompt_pos, 1, "光标在末尾");

        // Ctrl-U 清空,打三个词
        app.handle(ctrl(KeyCode::Char('u')));
        for c in "one two three".chars() {
            app.handle(键(KeyCode::Char(c)));
        }
        let text = |app: &App| app.view.prompt.as_ref().map(|(_, t)| t.clone()).unwrap();
        assert_eq!(text(&app), "one two three");
        assert_eq!(app.view.prompt_pos, 13);

        // Alt-左:跳到 three 前面;再跳两次到 one 前面
        app.handle(alt(KeyCode::Left));
        assert_eq!(app.view.prompt_pos, 8);
        app.handle(alt(KeyCode::Char('b')));
        assert_eq!(app.view.prompt_pos, 4);
        app.handle(alt(KeyCode::Char('b')));
        assert_eq!(app.view.prompt_pos, 0);
        // Alt-右:按词尾往后
        app.handle(alt(KeyCode::Right));
        assert_eq!(app.view.prompt_pos, 3);
        app.handle(alt(KeyCode::Char('f')));
        assert_eq!(app.view.prompt_pos, 7);

        // Ctrl-K 删到行尾,Ctrl-A/E 走首尾,插入落在光标处
        app.handle(ctrl(KeyCode::Char('k')));
        assert_eq!(text(&app), "one two");
        app.handle(ctrl(KeyCode::Char('a')));
        app.handle(键(KeyCode::Char('>')));
        assert_eq!(text(&app), ">one two");
        // Ctrl-W 删掉前一个词(光标前的 "one")
        app.handle(ctrl(KeyCode::Char('e')));
        app.handle(ctrl(KeyCode::Char('w')));
        assert_eq!(text(&app), ">one ");
        // Alt+退格:删掉前一个词(和 Ctrl-W 一样)
        app.handle(ctrl(KeyCode::Char('u')));
        for c in "alpha beta gamma".chars() {
            app.handle(键(KeyCode::Char(c)));
        }
        app.handle(键(KeyCode::End));
        app.handle(alt(KeyCode::Backspace));
        assert_eq!(text(&app), "alpha beta ");

        // Delete 删光标处的字符;Backspace 删光标前
        app.handle(ctrl(KeyCode::Char('a')));
        app.handle(键(KeyCode::Delete));
        assert_eq!(text(&app), "lpha beta ");
        app.handle(键(KeyCode::End));
        app.handle(键(KeyCode::Backspace));
        assert_eq!(text(&app), "lpha beta");
        app.handle(键(KeyCode::Esc));
        assert!(app.view.prompt.is_none());
    }

    /// 命令行(:)用同一套行编辑:光标能进能出,按词跳,Home/End
    #[test]
    fn 命令行也能按词跳() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.handle(键(KeyCode::Char(':')));
        for c in "w out/x.gtui".chars() {
            app.handle(键(KeyCode::Char(c)));
        }
        assert_eq!(app.view.cmd.as_deref(), Some("w out/x.gtui"));
        assert_eq!(app.view.cmd_pos, 12);
        app.handle(alt(KeyCode::Left));
        assert_eq!(app.view.cmd_pos, 2, "跳到 out/x.gtui 前面");
        app.handle(alt(KeyCode::Left));
        assert_eq!(app.view.cmd_pos, 0);
        app.handle(键(KeyCode::Home));
        assert_eq!(app.view.cmd_pos, 0);
        app.handle(键(KeyCode::Char('#')));
        assert_eq!(app.view.cmd.as_deref(), Some("#w out/x.gtui"));
        // 光标在 '#' 后面:Ctrl-K 只删到行尾,留下已经打的 '#';再 Ctrl-U 清掉行首
        app.handle(ctrl(KeyCode::Char('k')));
        assert_eq!(app.view.cmd.as_deref(), Some("#"));
        app.handle(ctrl(KeyCode::Char('u')));
        assert_eq!(app.view.cmd.as_deref(), Some(""));
        app.handle(键(KeyCode::Esc));
    }

    /// 拆分标记:一个框拆成多个,右边的框排在后面,一步撤销
    #[test]
    fn 用标记把一个框拆成多个() {
        let mut app = 连边应用(横排文档(2));
        app.jump_to(0);
        let i = app.cursor_box().unwrap();
        let n0 = app.doc.boxes.len();
        let before: Vec<String> = app.doc.texts.iter().map(|t| t.text.clone()).collect();
        改文字(&mut app, "hello,<|new box|>world!");
        let texts: Vec<&str> = app.doc.texts.iter().map(|t| t.text.as_str()).collect();
        assert!(texts.contains(&"hello,"), "{texts:?}");
        assert!(texts.contains(&"world!"), "{texts:?}");
        assert_eq!(app.doc.boxes.len(), n0 + 1, "多出一个框");
        // 光标还在原框
        assert_eq!(app.view.pick, render::Pick::Box(i));
        assert!(app
            .view
            .note
            .as_deref()
            .unwrap_or("")
            .contains("拆成 2 个框"));
        // 新框在原框右边、顶对齐
        let new = app.doc.boxes.len() - 1;
        assert!(app.doc.boxes[new].at.0 > app.doc.boxes[i].at.0, "放在右边");
        assert_eq!(app.doc.boxes[new].at.1, app.doc.boxes[i].at.1, "顶对齐");
        // 一步撤销:文字回到拆分前,新框一起消失
        app.undo_step();
        let back: Vec<String> = app.doc.texts.iter().map(|t| t.text.clone()).collect();
        assert_eq!(back, before, "撤销该恢复拆分前的文字");
        assert_eq!(app.doc.boxes.len(), n0, "新框一起撤销");
    }

    /// 在光标框上把 `t` 提示行改成 `text` 再回车
    fn 改文字(app: &mut App, text: &str) {
        // t 已经不是默认键了:直接走 :text 那条命令路径(和输入框提交同一个入口)
        let mut ids = app.targets().boxes;
        if ids.is_empty() {
            if let Some(i) = app.cursor_box() {
                ids.push(i);
            }
        }
        app.cmd_set_text(&ids, text);
    }

    /// 多个标记:从左到右依次排开
    #[test]
    fn 标记可以拆成三个框() {
        let mut app = 连边应用(横排文档(2));
        app.jump_to(0);
        let i = app.cursor_box().unwrap();
        let n0 = app.doc.boxes.len();
        改文字(&mut app, "a<|new box|>b<|new box|>c");
        assert_eq!(app.doc.boxes.len(), n0 + 2);
        let texts: Vec<&str> = app.doc.texts.iter().map(|t| t.text.as_str()).collect();
        for t in ["a", "b", "c"] {
            assert!(texts.contains(&t), "{t} 没切出来:{texts:?}");
        }
        // b、c 依次在原框右侧,互不重叠
        let mut xs: Vec<(i32, u16)> = (0..app.doc.boxes.len())
            .filter(|&k| k == i || k >= n0)
            .map(|k| (app.doc.boxes[k].at.0, app.doc.boxes[k].size.0))
            .collect();
        xs.sort();
        for w in xs.windows(2) {
            assert!(w[0].0 + w[0].1 as i32 <= w[1].0, "框互相压住:{xs:?}");
        }
    }

    /// 空段丢弃:开头 / 结尾 / 连续标记都不建空框;只剩一段就不拆
    #[test]
    fn 拆分的空段丢掉() {
        let mut app = 连边应用(横排文档(2));
        app.jump_to(0);
        let n0 = app.doc.boxes.len();
        改文字(&mut app, "<|new box|>a<|new box|><|new box|>b<|new box|>");
        assert_eq!(app.doc.boxes.len(), n0 + 1, "只有 a / b 两个非空段");
        let mut app = 连边应用(横排文档(2));
        app.jump_to(0);
        let before: Vec<String> = app.doc.texts.iter().map(|t| t.text.clone()).collect();
        let n0 = app.doc.boxes.len();
        改文字(&mut app, "<|new box|><|new box|>");
        assert_eq!(app.doc.boxes.len(), n0, "没有内容就不拆");
        assert_eq!(
            app.doc
                .texts
                .iter()
                .map(|t| t.text.clone())
                .collect::<Vec<_>>(),
            before,
            "失败时不写文字"
        );
        assert!(app
            .view
            .note
            .as_deref()
            .unwrap_or("")
            .contains("nothing to split"));
    }

    /// 转义:反斜杠保住字面标记,不拆
    #[test]
    fn 转义的标记当文字() {
        let mut app = 连边应用(横排文档(2));
        app.jump_to(0);
        let n0 = app.doc.boxes.len();
        改文字(&mut app, "x\\<|new box|>y");
        assert_eq!(app.doc.boxes.len(), n0, "转义不拆");
        let texts: Vec<&str> = app.doc.texts.iter().map(|t| t.text.as_str()).collect();
        assert!(texts.contains(&"x\\<|new box|>y"), "{texts:?}");
    }

    /// 右边有框就让开,绝不重叠
    #[test]
    fn 拆分的新框避开右边的框() {
        let mut app = 连边应用(横排文档(2));
        app.jump_to(0);
        let i = app.cursor_box().unwrap();
        let before: Vec<(i32, i32, u16, u16)> = app
            .doc
            .boxes
            .iter()
            .map(|b| (b.at.0, b.at.1, b.size.0, b.size.1))
            .collect();
        let n0 = app.doc.boxes.len();
        改文字(&mut app, "long text here<|new box|>z");
        assert_eq!(app.doc.boxes.len(), n0 + 1);
        let new = &app.doc.boxes[n0];
        let (nx, ny, nw, nh) = (new.at.0, new.at.1, new.size.0 as i32, new.size.1 as i32);
        for (k, b) in app.doc.boxes.iter().enumerate() {
            if k == n0 || k == i {
                continue;
            }
            let (bx, by, bw, bh) = (b.at.0, b.at.1, b.size.0 as i32, b.size.1 as i32);
            assert!(
                !(nx < bx + bw && bx < nx + nw && ny < by + bh && by < ny + nh),
                "新框压住了第 {k} 个框"
            );
        }
        assert_eq!(before.len(), n0, "旧框数量没变");
    }

    /// 不能拆的:多选 / 边注释框 / 点节点,都按普通文字写
    #[test]
    fn 不能拆的框标记当文字() {
        // 多选
        let mut app = 连边应用(横排文档(2));
        app.jump_to(0);
        app.view.sel = Some(RectSel {
            anchor: (-5, -5),
            end: (40, 20),
            confirmed: true,
        });
        let n0 = app.doc.boxes.len();
        改文字(&mut app, "a<|new box|>b");
        assert_eq!(app.doc.boxes.len(), n0, "多选不拆");
        let texts: Vec<&str> = app.doc.texts.iter().map(|t| t.text.as_str()).collect();
        assert!(
            texts.contains(&"a<|new box|>b"),
            "标记按普通文字写:{texts:?}"
        );
        // 点节点
        let mut doc = 横排文档(2);
        doc.boxes[0].style = crate::model::BoxStyle::Point;
        let mut app = 连边应用(doc);
        app.jump_to(0);
        let n0 = app.doc.boxes.len();
        改文字(&mut app, "a<|new box|>b");
        assert_eq!(app.doc.boxes.len(), n0, "点节点不拆");
        assert!(app.doc.texts.iter().any(|t| t.text == "a<|new box|>b"));
        // 边注释框
        let mut app = 连边应用(横排文档(2));
        let _ = app.doc.add_edge("b0", "b1", "g0", Dir::Forward);
        app.doc.set_note(0, "note");
        app.refresh_pick();
        let nb = app
            .doc
            .boxes
            .iter()
            .position(|b| b.edge.is_some())
            .expect("注释框");
        app.jump_to(nb);
        let n0 = app.doc.boxes.len();
        改文字(&mut app, "a<|new box|>b");
        assert_eq!(app.doc.boxes.len(), n0, "注释框不拆");
        assert!(app.doc.texts.iter().any(|t| t.text == "a<|new box|>b"));
    }

    /// 新框复制原框的框型、颜色、填充、对齐、图层
    #[test]
    fn 拆分的新框照抄样式() {
        let mut app = 连边应用(横排文档(2));
        app.jump_to(0);
        let i = app.cursor_box().unwrap();
        app.doc.boxes[i].style = crate::model::BoxStyle::Double;
        app.doc.boxes[i].color = Some((1, 2, 3));
        app.doc.boxes[i].fill = Some((4, 5, 6));
        let ti = app.doc.text_of(i).expect("文字");
        app.doc.texts[ti].color = Some((7, 8, 9));
        app.doc.texts[ti].bg = Some((10, 11, 12));
        app.doc.texts[ti].align = crate::model::Align::Center;
        let n0 = app.doc.boxes.len();
        改文字(&mut app, "p<|new box|>q");
        assert_eq!(app.doc.boxes.len(), n0 + 1);
        let b = &app.doc.boxes[n0];
        assert_eq!(b.style, crate::model::BoxStyle::Double, "框型");
        assert_eq!(b.color, Some((1, 2, 3)), "边框色");
        assert_eq!(b.fill, Some((4, 5, 6)), "填充");
        let t = app.doc.text(n0).expect("新框文字");
        assert_eq!(t.color, Some((7, 8, 9)), "文字色");
        assert_eq!(t.bg, Some((10, 11, 12)), "文字底色");
        assert_eq!(t.align, crate::model::Align::Center, "对齐");
        assert_eq!(t.text, "q");
    }

    /// 边留在原框;新框不连边;缩放不影响拆分
    #[test]
    fn 拆分后边留在原框() {
        let mut app = 连边应用(横排文档(3));
        let _ = app.doc.add_edge("b0", "b1", "g0", Dir::Forward);
        app.refresh_pick();
        app.jump_to(0);
        app.view.zoom = 2.0;
        app.view.center = Some((app.view.center.unwrap_or((0.0, 0.0)).0 + 1.0, 0.0));
        let i = app.cursor_box().unwrap();
        改文字(&mut app, "p<|new box|>q");
        assert_eq!(app.doc.edges.len(), 1, "边数不变");
        assert_eq!(app.doc.edges[0].from, app.doc.boxes[i].id, "边还挂在原框");
        assert!(
            app.doc.boxes.last().unwrap().edge.is_none(),
            "新框没有注释关系"
        );
    }

    /// `:text` 与 headless 都不拆:标记是普通文字
    #[test]
    fn 命令行走文本不拆() {
        let mut app = 连边应用(横排文档(2));
        app.jump_to(0);
        let n0 = app.doc.boxes.len();
        app.cmd_apply("text q<|new box|>r");
        assert_eq!(app.doc.boxes.len(), n0, ":text 不拆");
    }

    /// 英文界面:样式菜单(框/边)的标题不能出现 CJK;中文界面照旧
    #[test]
    fn 英文菜单不出现中文() {
        fn has_cjk(s: &str) -> bool {
            s.chars()
                .any(|c| matches!(c as u32, 0x3000..=0x303f | 0x4e00..=0x9fff | 0xff00..=0xffef))
        }
        let en_opts = Options {
            lang: Lang::En,
            ..Options::default()
        };
        for on_edge in [false, true] {
            let mut doc = 横排文档(2);
            doc.add_edge("b0", "b1", "g0", Dir::Forward).expect("加边");
            let mut app = 连边应用(doc);
            app.view.lang = Lang::En;
            if on_edge {
                app.view.pick = render::Pick::Edge(0);
            } else {
                app.view.pick = render::Pick::Box(0);
            }
            let titles: Vec<String> = app.menu_for().iter().map(|i| i.title.clone()).collect();
            assert!(!titles.is_empty());
            for title in &titles {
                assert!(!has_cjk(title), "英文菜单里有中文:{title}");
            }
            let _ = &en_opts;
        }
    }

    /// 默认界面英文;`--lang zh`(Options.lang)与文档里的 view.lang 都能切回中文
    #[test]
    fn 默认英文两种办法切中文() {
        assert_eq!(Lang::default(), Lang::En);
        assert_eq!(crate::document::ViewState::default().lang, "en");

        // 默认:英文
        let app = App::new(Document::scratch(), Options::default()).expect("开局");
        assert_eq!(app.view.lang, Lang::En);

        // 命令行 --lang zh
        let app = App::new(Document::scratch(), zh_opts()).expect("开局");
        assert_eq!(app.view.lang, Lang::Zh);

        // 文档里的 view.lang = zh:新开还是英文,打开这份文档就变中文
        let mut doc = Document::scratch();
        doc.view.lang = "zh".to_string();
        let app = App::new(doc, Options::default()).expect("开局");
        assert_eq!(app.view.lang, Lang::Zh, "文档里的语言要生效");
    }

    /// `s` 菜单改过样式的框也算"选中":光标挪到空白处再 :new,照样照抄它
    #[test]
    fn 改过样式的框也算选中() {
        let mut app = 连边应用(横排文档(2));
        app.view.cursor = app.cell_of(0);
        app.refresh_pick();
        // 走菜单改样式:框型 => double
        app.handle(键(KeyCode::Char('s')));
        app.handle(键(KeyCode::Char('l')));
        let want = app
            .view
            .menu
            .as_ref()
            .unwrap()
            .level()
            .iter()
            .position(|i| i.title == "double")
            .expect("有 double");
        for _ in 0..want {
            app.handle(键(KeyCode::Char('j')));
        }
        app.handle(键(KeyCode::Enter));
        assert_eq!(app.doc.boxes[0].style, BoxStyle::Double);

        // 光标挪到空白处(那里没有框),再新建
        app.view.cursor = (60, 20);
        app.view.pick = render::Pick::None;
        app.cmd_apply("new 远方");
        let i = app.doc.boxes.len() - 1;
        assert_eq!(
            app.doc.boxes[i].style,
            BoxStyle::Double,
            "刚用菜单改过样式的框该被照抄"
        );
    }

    /// 边菜单的"端口":起点/终点各一档 auto + 四侧,命令就是 :fromside / :toside
    #[test]
    fn 端口菜单能改边两端的侧() {
        let mut app = 连边应用(横排文档(2));
        let e = app
            .doc
            .add_edge("b0", "b1", "g0", Dir::Forward)
            .expect("加边");
        app.jump_to(0);
        app.handle(ctrl(KeyCode::Char('e')));
        assert!(app.view.pick.edge().is_some(), "Ctrl-e 该选到边");
        app.handle(键(KeyCode::Char('s')));
        let menu = app.view.menu.clone().expect("边上该开菜单");
        let port = menu
            .level()
            .iter()
            .find(|i| i.title == "端口")
            .expect("边菜单该有端口");
        let MenuAction::Sub(ends) = &port.action else {
            panic!("端口该是子菜单")
        };
        let titles: Vec<&str> = ends.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, ["起点", "终点"]);
        let MenuAction::Sub(sides) = &ends[0].action else {
            panic!("起点该是子菜单")
        };
        let cmds: Vec<&str> = sides
            .iter()
            .map(|i| match &i.action {
                MenuAction::Cmd(c) => c.as_str(),
                _ => panic!("每一侧都是一条命令"),
            })
            .collect();
        assert_eq!(
            cmds,
            [
                "fromside auto",
                "fromside left",
                "fromside right",
                "fromside up",
                "fromside down"
            ]
        );
        app.handle(键(KeyCode::Esc));
        // 关菜单会把 pick 交回光标下的元素,重新选一次边
        app.handle(ctrl(KeyCode::Char('e')));
        assert!(app.view.pick.edge().is_some(), "重新选到边");

        // 走菜单背后那条命令:起点 left、终点 up 各一步撤销
        app.cmd_apply("fromside left");
        app.cmd_apply("toside up");
        assert_eq!(app.doc.edges[e].from_side, Some(Side::Left));
        assert_eq!(app.doc.edges[e].to_side, Some(Side::Up));

        app.handle(键(KeyCode::Char('u')));
        assert_eq!(app.doc.edges[e].from_side, Some(Side::Left), "只回退一步");
        assert_eq!(app.doc.edges[e].to_side, None);
    }

    /// c:颜色菜单 = auto + 12 色,当前值打 *;只改边框色
    #[test]
    fn c_颜色菜单只改主颜色() {
        let mut app = 连边应用(横排文档(2));
        app.jump_to(0);
        app.handle(键(KeyCode::Char('c')));
        let menu = app.view.menu.clone().expect("该开颜色菜单");
        let items = menu.level().to_vec();
        assert_eq!(items.len(), 13, "auto + 12 色");
        assert_eq!(items[0].title, "auto");
        assert!(items[0].checked, "还没设过颜色,auto 该打 *");
        app.handle(键(KeyCode::Esc));
        // 设一个颜色和填充,再看标记
        app.cmd_apply("nodecolor #ef4b4b");
        app.cmd_apply("fill #22b46a");
        app.handle(键(KeyCode::Char('c')));
        // 再开一次:这次当前值在调色板里
        app.handle(键(KeyCode::Char('c')));
        let menu = app.view.menu.clone().expect("再开");
        let hit = menu
            .level()
            .iter()
            .position(|i| i.checked)
            .expect("该有打 * 的项");
        assert_eq!(hit, 2, "12 色里第二个 #ef4b4b");
        // 选第三个颜色(索引 3)回车:只改边框色
        app.handle(键(KeyCode::Char('j')));
        app.handle(键(KeyCode::Enter));
        assert_eq!(app.doc.boxes[0].color, Some((0x22, 0xb4, 0x6a)), "边框色");
        assert_eq!(app.doc.boxes[0].fill, Some((0x22, 0xb4, 0x6a)), "填充不变");
    }

    /// 预览不动撤销,Esc 还原
    #[test]
    fn 菜单预览不压撤销_esc还原() {
        let mut app = 连边应用(横排文档(2));
        app.jump_to(0);
        let undo0 = app.undo.counts().0;
        let before = app.doc.boxes[0].style;
        app.handle(键(KeyCode::Char('s'))); // 框型
        app.handle(键(KeyCode::Char('l'))); // 进子菜单(预览第一项 point)
        app.handle(键(KeyCode::Char('j'))); // 预览 single
        assert_ne!(app.doc.boxes[0].style, before, "预览该实时改文档");
        assert_eq!(app.undo.counts().0, undo0, "预览不压撤销");
        app.handle(键(KeyCode::Esc)); // 退回上一级
        app.handle(键(KeyCode::Esc)); // 关掉
        assert_eq!(app.doc.boxes[0].style, before, "Esc 该还原");
        assert_eq!(app.undo.counts().0, undo0);
    }

    /// 多选一次改多个,只算一步
    #[test]
    fn 菜单多选一次改多个() {
        let mut app = 连边应用(横排文档(3));
        app.view.cursor = (0, 0);
        app.handle(键(KeyCode::Char('v')));
        app.view.cursor = (13, 2);
        app.handle(键(KeyCode::Enter));
        // 光标回到框上,菜单按框类型走
        app.jump_to(0);
        let undo0 = app.undo.counts().0;
        app.handle(键(KeyCode::Char('s')));
        app.handle(键(KeyCode::Char('l')));
        // 子菜单里按标题找 rounded(插了 none 之后下标会变)
        let want = app
            .view
            .menu
            .as_ref()
            .unwrap()
            .level()
            .iter()
            .position(|i| i.title == "rounded")
            .expect("有 rounded");
        for _ in 0..want {
            app.handle(键(KeyCode::Char('j')));
        }
        app.handle(键(KeyCode::Enter));
        assert_eq!(app.doc.boxes[0].style, BoxStyle::Rounded);
        assert_eq!(app.doc.boxes[1].style, BoxStyle::Rounded, "两个都改");
        assert_eq!(app.undo.counts().0, undo0 + 1, "整批一步");
        assert!(app.view.sel.is_some(), "菜单不该动选区");
    }

    /// 边的线型菜单:应用后一步撤销
    #[test]
    fn 边菜单改线型() {
        let mut app = 连边应用(横排文档(2));
        app.doc
            .add_edge("b0", "b1", "g0", Dir::Forward)
            .expect("加边");
        app.jump_to(0);
        app.handle(ctrl(KeyCode::Char('e')));
        let undo0 = app.undo.counts().0;
        app.handle(键(KeyCode::Char('s')));
        app.handle(键(KeyCode::Char('l')));
        app.handle(键(KeyCode::Char('j')));
        app.handle(键(KeyCode::Enter));
        assert_eq!(app.doc.edges[0].style, EdgeStyle::Dashed);
        assert_eq!(app.undo.counts().0, undo0 + 1);
    }

    /// 同值选择不压撤销
    #[test]
    fn 菜单选同一个值不压撤销() {
        let mut app = 连边应用(横排文档(2));
        app.jump_to(0);
        app.cmd_apply("box single");
        let undo0 = app.undo.counts().0;
        app.handle(键(KeyCode::Char('s')));
        app.handle(键(KeyCode::Char('l')));
        // 子菜单里按标题找 single
        let single = app
            .view
            .menu
            .as_ref()
            .unwrap()
            .level()
            .iter()
            .position(|i| i.title == "single")
            .expect("有 single");
        for _ in 0..single {
            app.handle(键(KeyCode::Char('j')));
        }
        app.handle(键(KeyCode::Enter));
        assert_eq!(app.doc.boxes[0].style, BoxStyle::Single);
        assert_eq!(app.undo.counts().0, undo0, "同值不该压撤销");
    }

    /// 边菜单的"分组"项走 group 动词
    #[test]
    fn 边菜单改分组() {
        let mut app = 连边应用(样例文档());
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        app.jump_to(1); // b1 上有两条边
        app.handle(ctrl(KeyCode::Char('e')));
        let e = app.view.pick.edge().expect("选到边");
        let before = app.doc.edges[e].group.clone();
        app.handle(键(KeyCode::Char('s')));
        // 找到"分组"那一项并进去,选另一组
        let menu = app.view.menu.clone().expect("菜单");
        let pos = menu
            .level()
            .iter()
            .position(|i| i.title == "分组")
            .expect("边菜单该有分组项");
        for _ in 0..pos {
            app.handle(键(KeyCode::Char('j')));
        }
        app.handle(键(KeyCode::Char('l')));
        let sub = app.view.menu.clone().expect("子菜单");
        let other = (0..sub.level().len())
            .find(|i| !sub.level()[*i].checked)
            .expect("该有别的分组");
        for _ in 0..other {
            app.handle(键(KeyCode::Char('j')));
        }
        app.handle(键(KeyCode::Enter));
        assert_ne!(app.doc.edges[e].group, before, "分组该换掉");
    }

    /// textbg / edgebg:文字底色与边底色各改各的,而且能清掉
    #[test]
    fn 文字底色与边底色动词() {
        let mut app = 连边应用(横排文档(2));
        app.doc
            .add_edge("b0", "b1", "g0", Dir::Forward)
            .expect("加边");
        app.jump_to(0);
        app.cmd_apply("textbg #334455");
        assert_eq!(app.doc.text(0).unwrap().bg, Some((0x33, 0x44, 0x55)));
        assert_eq!(app.doc.boxes[0].fill, None, "文字底色不碰填充");
        // 边:先选中再改
        app.handle(ctrl(KeyCode::Char('e')));
        app.cmd_apply("edgebg #552211");
        let e = app.view.pick.edge().expect("选到边");
        assert_eq!(app.doc.edges[e].bg, Some((0x55, 0x22, 0x11)));
        assert_eq!(app.doc.edges[e].color, None, "边底色不该改边的前景色");
        // 清掉
        app.cmd_apply("edgebg none");
        assert_eq!(app.doc.edges[e].bg, None);
        app.jump_to(0);
        app.cmd_apply("textbg none");
        assert_eq!(app.doc.text(0).unwrap().bg, None);
    }

    /// 菜单里 h / l 只管层级:l 往下钻,h 往回收,叶子项不会被 l 执行
    #[test]
    fn 菜单hl切换层级() {
        let mut app = 连边应用(横排文档(2));
        app.jump_to(0);
        app.handle(键(KeyCode::Char('s')));
        assert_eq!(app.view.menu.as_ref().unwrap().levels.len(), 1);
        // l 进"框型"
        app.handle(键(KeyCode::Char('l')));
        assert_eq!(
            app.view.menu.as_ref().unwrap().levels.len(),
            2,
            "l 该进子菜单"
        );
        assert_eq!(app.view.menu.as_ref().unwrap().parents.len(), 1);
        // h 回上一层
        app.handle(键(KeyCode::Char('h')));
        assert_eq!(
            app.view.menu.as_ref().unwrap().levels.len(),
            1,
            "h 该回上一层"
        );
        assert!(app.view.menu.as_ref().unwrap().parents.is_empty());
        // 再 h:顶层就关掉
        app.handle(键(KeyCode::Char('h')));
        assert!(app.view.menu.is_none(), "顶层 h 该关掉菜单");
        // 叶子项上按 l 不执行
        let undo0 = app.undo.counts().0;
        let before = app.doc.boxes[0].style;
        app.handle(键(KeyCode::Char('s')));
        app.handle(键(KeyCode::Char('l'))); // 进框型
        app.handle(键(KeyCode::Char('j'))); // 预览 single
        app.handle(键(KeyCode::Char('l'))); // 叶子:不该执行
        assert!(app.view.menu.is_some(), "l 在叶子上不该关菜单");
        assert_eq!(app.undo.counts().0, undo0, "l 在叶子上不该压撤销");
        // Enter 才执行
        app.handle(键(KeyCode::Enter));
        assert!(app.view.menu.is_none());
        assert_ne!(app.doc.boxes[0].style, before, "Enter 该应用");
        assert_eq!(app.undo.counts().0, undo0 + 1);
    }

    /// 菜单画出来:色块用底色、当前值打 *、状态栏报数量和值
    #[test]
    fn 菜单的色块与状态栏() {
        let mut app = 连边应用(横排文档(2));
        app.jump_to(0);
        app.cmd_apply("nodecolor #ef4b4b");
        app.handle(键(KeyCode::Char('c')));
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        let mut swatch = 0;
        let mut star = 0;
        for y in 0..app.area.height {
            for x in 0..app.area.width {
                let cell = &buf[(x, y)];
                if cell.bg == ratatui::style::Color::Rgb(0x22, 0xb4, 0x6a) {
                    swatch += 1;
                }
                if cell.symbol() == "*" {
                    star += 1;
                }
            }
        }
        assert!(swatch >= 2, "色块该用底色画出来,只找到 {swatch}");
        assert_eq!(star, 1, "当前值只有一个 *");
        // 底部两行合起来看(菜单信息在下边那行,状态在上行)
        let rows: String = (app.area.height.saturating_sub(2)..app.area.height)
            .flat_map(|y| (0..app.area.width).map(move |x| (x, y)))
            .flat_map(|(x, y)| buf[(x, y)].symbol().chars())
            .filter(|c| !c.is_whitespace())
            .collect();
        assert!(rows.contains("改1个框"), "状态栏该报作用数量:{rows}");
        assert!(rows.contains("边框色"), "状态栏该报属性:{rows}");
    }

    /// 右键菜单和键盘菜单内容一致
    #[test]
    fn 右键菜单和键盘菜单一致() {
        let mut app = 连边应用(横排文档(2));
        app.jump_to(0);
        let keyboard = app.menu_for();
        let p = render::panels(&app.doc, app.area, &app.view);
        let (cx, cy) = p.proj.cell(app.doc.center(0));
        app.open_menu(cx as u16, cy as u16);
        let mouse = app.view.menu.clone().expect("右键菜单");
        let a: Vec<&str> = keyboard.iter().map(|i| i.title.as_str()).collect();
        let b: Vec<&str> = mouse.level().iter().map(|i| i.title.as_str()).collect();
        assert_eq!(a, b);
    }

    #[test]
    fn link_动词仍然单目标快速连边() {
        let mut app = 连边应用(横排文档(2));
        app.jump_to(0);
        app.cmd_apply("link b1");
        assert_eq!(边名(&app), vec![("b0".into(), "b1".into())]);
        // c 不再是连边:它开颜色菜单
        app.handle(键(KeyCode::Char('c')));
        assert!(app.view.menu.is_some(), "c 该打开颜色菜单");
        assert!(app.view.prompt.is_none(), "c 不该再进连边提示");
    }

    #[test]
    fn w_从dot来源写同目录gtui且不改原文件() {
        let dot = 临时路径("dot-src", "dot");
        std::fs::write(&dot, "// sentinel\n").expect("写 .dot");
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.source = Some(dot.clone());
        app.cmd_apply("w");
        let native = Path::new(&dot).with_extension("gtui");
        assert!(native.exists(), "该写出同目录的 .gtui");
        assert_eq!(std::fs::read_to_string(&dot).unwrap(), "// sentinel\n");
        let native_text = native.to_string_lossy().into_owned();
        assert_eq!(app.source.as_deref(), Some(native_text.as_str()));
        let _ = std::fs::remove_file(&dot);
        let _ = std::fs::remove_file(&native);
    }

    /// :wq 存成功才退;没有文件名时不退
    #[test]
    fn wq_存盘并退出() {
        let path = 临时路径("wq", "gtui");
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.source = Some(path.clone());
        app.cmd_apply("wq");
        assert!(app.quit, "存成功该退出");
        assert!(Path::new(&path).exists(), "该写出来");
        let body = std::fs::read_to_string(&path).expect("读回来");
        assert!(body.contains("\"graph-tui\""));
        let _ = std::fs::remove_file(&path);

        // 没有文件名:提示要路径,且不退出
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.source = None;
        app.cmd_apply("wq");
        assert!(!app.quit, "没存成不该退出");
        let note = app.view.note.clone().unwrap_or_default();
        assert!(note.contains("文件名"), "{note}");

        // :wq <路径> 也能用
        let p2 = 临时路径("wq-path", "gtui");
        app.cmd_apply(&format!("wq {p2}"));
        assert!(app.quit, "带路径存成功该退出");
        assert!(Path::new(&p2).exists());
        let _ = std::fs::remove_file(&p2);
    }

    /// :wq 存失败也不退出(比如给了 .dot 路径)
    #[test]
    fn wq_存失败不退出() {
        let dot = 临时路径("wq-refuse", "dot");
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.cmd_apply(&format!("wq {dot}"));
        assert!(!app.quit, "存失败要留在原地");
        assert!(!Path::new(&dot).exists());
    }

    #[test]
    fn w_从gtui来源写回原路径() {
        let path = 临时路径("gtui-src", "gtui");
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.source = Some(path.clone());
        app.cmd_apply("w");
        let body = std::fs::read_to_string(&path).expect("该写出来");
        assert!(body.contains("\"graph-tui\""), "该是 native 文档:{body}");
        assert_eq!(app.source.as_deref(), Some(path.as_str()));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn stdin来源存盘要求显式路径() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.source = None;
        app.cmd_apply("w");
        let note = app.view.note.clone().unwrap_or_default();
        assert!(note.contains("文件名"), "该提示要显式路径:{note}");
        // :save 带路径就能写
        let path = 临时路径("stdin-save", "gtui");
        app.cmd_apply(&format!("save {path}"));
        assert!(Path::new(&path).exists());
        assert_eq!(app.source.as_deref(), Some(path.as_str()));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn save_只写gtui不写dot() {
        let dot = 临时路径("refuse", "dot");
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.cmd_apply(&format!("save {dot}"));
        assert!(!Path::new(&dot).exists(), "绝不写回 .dot");
        assert!(app.view.note.as_deref().unwrap_or("").contains(".gtui"));
    }

    #[test]
    fn 导出dot不动图且把警告放回显位() {
        let dot = 临时路径("export", "dot");
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        let before = app.doc.boxes.clone();
        app.cmd_apply(&format!("export {dot}"));
        assert_eq!(app.doc.boxes, before, "导出不该改图");
        let body = std::fs::read_to_string(&dot).expect("该导出");
        assert!(
            body.contains("digraph") || body.contains("graph"),
            "该是 dot:{body}"
        );
        assert!(app.view.note.is_some(), "导出要有回显");
        let _ = std::fs::remove_file(&dot);
    }

    #[test]
    fn r_换布局后坐标仍是整数() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.view.zoom = 2.5;
        let want = app.view.layout.next();
        app.handle(键(KeyCode::Char('r')));
        assert_eq!(app.view.layout, want);
        for (i, b) in app.doc.boxes.iter().enumerate() {
            let t = app.doc.text(i).expect("每个框都要有文字");
            if b.style.is_boxed() {
                assert_eq!(b.size.1, 3);
                assert_eq!(t.at, (b.at.0 + 1, b.at.1 + 1));
            } else {
                assert_eq!(b.size, (1, 1));
            }
        }
    }

    #[test]
    fn 视图状态往返一致并且开局映射进view() {
        let mut doc = 样例文档();
        doc.view = ViewState {
            center: (12.5, -3.0),
            zoom: 2.0,
            layout: "force".into(),
            cursor: Some(crate::document::Cursor::Box("b1".into())),
            focus: vec!["g1".into()],
            labels: true,
            lang: "en".into(),
            layer: Some(String::new()),
            side: 3.5,
            cycle: Some(1.25),
            orientation: "90".to_string(),
            notes: false,
        };
        let body = doc.save();
        let back = Document::load(&body).expect("读回");
        // 光标写出去会换成格子,比较解析后的格子
        assert_eq!(back.view.cursor_cell(&back), doc.view.cursor_cell(&doc));
        assert_eq!(
            ViewState {
                cursor: None,
                ..back.view.clone()
            },
            ViewState {
                cursor: None,
                ..doc.view.clone()
            }
        );
        // 默认选项不压文档里的语言:文档写 en 就用 en
        let app = App::new(back, Options::default()).expect("开局");
        assert_eq!(
            app.view.cursor,
            app.doc.text(1).unwrap().at,
            "cursor 按 id 映射成文字起点格"
        );
        assert_eq!(app.view.focus, vec![1], "focus 按 id 映射成下标");
        assert!(app.view.labels);
        assert_eq!(app.view.lang, Lang::En);
        // 打开即"按一次 0":文件里存的 zoom/center 不再恢复
        assert_eq!(app.view.zoom, 1.0, "打开该回到 1:1");
        assert!(app.view.center.is_none(), "打开该居中到整幅");
        assert_eq!(app.view.side, 3.5);
        assert_eq!(app.view.cycle, Some(1.25));
        assert_eq!(app.view.layout, Layout::Force);
        assert_eq!(app.view.layer.as_deref(), Some(""));
    }

    /// 打开即按 `0`:1:1、居中到整幅(`:e` 换文件也一样)
    #[test]
    fn 打开即按0() {
        let mut doc = 样例文档();
        doc.view.zoom = 3.0;
        doc.view.center = (40.0, 12.0);
        let mut app = App::new(doc.clone(), zh_opts()).expect("开局");
        assert_eq!(app.view.zoom, 1.0);
        assert!(app.view.center.is_none());
        assert!(app.view.pan.is_none() && app.view.target.is_none());

        // :e 打开另一个文件:同样归位
        let path = 临时路径("open-zero", "gtui");
        let mut other = 样例文档();
        other.view.zoom = 2.5;
        other.view.center = (5.0, 5.0);
        std::fs::write(&path, other.save()).expect("写");
        app.view.zoom = 2.0;
        app.view.center = Some((7.0, 7.0));
        app.cmd_apply(&format!("e {path}"));
        assert_eq!(app.view.zoom, 1.0, ":e 之后也回到 1:1");
        assert!(app.view.center.is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn 冒号e打开gtui并重置视图() {
        let src = 临时路径("open-src", "gtui");
        let mut doc = 样例文档();
        doc.view.cursor = Some(crate::document::Cursor::Box("b2".into()));
        doc.view.zoom = 1.0;
        std::fs::write(&src, doc.save()).expect("写源文件");
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.view.sel = Some(RectSel {
            anchor: (0, 0),
            end: (0, 0),
            confirmed: false,
        });
        app.cmd_apply(&format!("e {src}"));
        let b2 = app.doc.box_idx("b2").unwrap();
        assert_eq!(app.view.cursor, app.doc.text(b2).unwrap().at);
        assert!(app.view.sel.is_none(), "换文档要清选区");
        assert_eq!(app.source.as_deref(), Some(src.as_str()));
        // 打开的是 en 文档:界面跟着变成英文
        assert!(app.view.note.as_deref().unwrap_or("").contains("opened"));
        let _ = std::fs::remove_file(&src);
    }

    #[test]
    fn 冒号w写到指定路径并切换来源() {
        let path = 临时路径("colon-w", "gtui");
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.cmd_apply(&format!("w {path}"));
        assert!(Path::new(&path).exists());
        assert_eq!(app.source.as_deref(), Some(path.as_str()));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn 数字前缀与命令解析保持原样() {
        let mut p: Option<u32> = None;
        assert!(count_digit(&mut p, '5'));
        assert_eq!(p, Some(5));
        assert!(count_digit(&mut p, '0'));
        assert_eq!(p, Some(50));
        let mut q: Option<u32> = None;
        assert!(!count_digit(&mut q, '0'));
        assert_eq!(cmd_rest("text  两个空格", "text"), " 两个空格");
        assert_eq!(parse_via("1,2 3,4"), Some(vec![(1, 2), (3, 4)]));
        assert_eq!(parse_via("1,2,z"), None);
        assert_eq!(parse_color("#4c8ef8"), Some((0x4c, 0x8e, 0xf8)));
        assert_eq!(parse_color("#12"), None);
    }

    #[test]
    fn 搜索命中刷新() {
        let doc = 样例文档();
        // 数字:聚焦那一组
        let mut view = View::default();
        view.query = Some("1".into());
        refresh(&doc, &mut view);
        assert_eq!(view.focus, vec![0]);
        assert_eq!(view.hits, vec![0, 1]);
        // 文字:只挑命中的框,不碰聚焦(同组的其它框不该被一起点亮)
        let mut view = View::default();
        view.query = Some("丙".into());
        refresh(&doc, &mut view);
        assert_eq!(view.cursor, doc.text(2).unwrap().at);
        assert_eq!(view.hits, vec![2]);
        assert!(view.focus.is_empty(), "文字搜索不改 focus");
        view.query = Some("zzzz".into());
        refresh(&doc, &mut view);
        assert!(view.hits.is_empty());
        assert!(view.focus.is_empty());
    }

    #[test]
    fn 选边键循环并按光标清掉() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.jump_to(1);
        for want in [0usize, 1, 0] {
            app.handle(ctrl(KeyCode::Char('e')));
            assert_eq!(app.view.pick.edge(), Some(want));
        }
        app.view.pick = render::Pick::Edge(0);
        app.jump_to(2);
        assert!(app.view.pick.edge().is_none(), "换框后无关的选边要清掉");
    }

    /// 鼠标点空白处的边 = 选中那条边,不起拖动
    #[test]
    fn 鼠标点边选中() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        let area = app.area;
        let mut buf = ratatui::buffer::Buffer::empty(area);
        app.draw(&mut buf, area);
        // 找一格属于边、且换算回画布后不在任何框里的屏幕格
        let p = render::panels(&app.doc, area, &app.view);
        let (x, y) = app
            .hit
            .edge_cells
            .values()
            .flat_map(|v| v.iter())
            .find(|c| app.doc.box_at_cell(p.proj.canvas(c.0, c.1)).is_none())
            .copied()
            .expect("样例该有边笔画");
        app.handle(crossterm::event::Event::Mouse(
            crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column: x as u16,
                row: y as u16,
                modifiers: crossterm::event::KeyModifiers::NONE,
            },
        ));
        assert!(app.view.pick.edge().is_some(), "点边该选中它");
        assert!(
            matches!(app.drag, Some(Drag::Select { .. })),
            "按下去就该准备好拉选区(拖起来才落)"
        );
    }

    /// 鼠标在空白处拖:拉出区块选择(单击不算);Ctrl+左键/中键拖还是平移
    #[test]
    fn 鼠标拖出区块选择() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        let p = render::panels(&app.doc, app.area, &app.view);
        let at = |x: u16, y: u16, kind: MouseEventKind| MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        };
        // 找一个空白行:地图右下角附近
        let (sx, sy) = (p.map.x + 2, p.map.y + p.map.height - 3);
        app.handle(Event::Mouse(at(
            sx,
            sy,
            MouseEventKind::Down(MouseButton::Left),
        )));
        assert!(app.view.sel.is_none(), "只按一下不该成选区");
        app.handle(Event::Mouse(at(
            sx + 6,
            sy - 3,
            MouseEventKind::Drag(MouseButton::Left),
        )));
        let anchor = app.view.sel.expect("拖起来该有选区").anchor;
        assert_eq!(
            anchor,
            p.proj.canvas(sx as i32, sy as i32),
            "锚点是按下那一格"
        );
        assert_eq!(
            app.view.cursor,
            p.proj.canvas((sx + 6) as i32, (sy - 3) as i32)
        );
        app.handle(Event::Mouse(at(
            sx + 6,
            sy - 3,
            MouseEventKind::Up(MouseButton::Left),
        )));
        // 这一块圈出来是空的:松手就不留选区(免得挡着后面的编辑)
        assert!(app.view.sel.is_none(), "空选区该被丢掉");

        // 拉一块盖住整幅图的选区:相交的框都算选中
        app.view.sel = None;
        let (x0, y0, x1, y1) = app.doc.bbox();
        let cell_to_screen = |c: (i32, i32)| p.proj.cell((c.0 as f64, c.1 as f64));
        let (ax, ay) = cell_to_screen((x0 - 1, y0 - 1));
        let (bx, by) = cell_to_screen((x1 + 1, y1 + 1));
        app.handle(Event::Mouse(at(
            ax as u16,
            ay as u16,
            MouseEventKind::Down(MouseButton::Left),
        )));
        app.handle(Event::Mouse(at(
            bx as u16,
            by as u16,
            MouseEventKind::Drag(MouseButton::Left),
        )));
        app.handle(Event::Mouse(at(
            bx as u16,
            by as u16,
            MouseEventKind::Up(MouseButton::Left),
        )));
        let p2 = render::panels(&app.doc, app.area, &app.view);
        let got = render::selected(&app.doc, &p2, &app.view);
        assert_eq!(got.len(), app.doc.boxes.len(), "整幅图都该被选中");
        // 松开左键就是"选完了":直接确认,不用再按 Enter
        assert!(
            app.view.sel.expect("选区还在").confirmed,
            "拖完松手该已确认"
        );
        // Ctrl + 左键拖是平移,不起选区
        app.view.sel = None;
        let center = app.view.center;
        app.handle(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: sx,
            row: sy,
            modifiers: KeyModifiers::CONTROL,
        }));
        app.handle(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: sx + 4,
            row: sy,
            modifiers: KeyModifiers::CONTROL,
        }));
        assert!(app.view.sel.is_none(), "Ctrl+左键是平移,不该拉选区");
        assert_ne!(app.view.center, center, "画面该跟着挪");
    }

    /// 跳转历史:搜索也记一笔,Ctrl-o 退、Tab 进
    #[test]
    fn 跳转历史前进后退() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        let start = app.view.cursor;
        app.jump_to(2);
        let a = app.view.cursor;
        app.jump_to(1);
        let b = app.view.cursor;
        app.handle(键(KeyCode::Char('u'))); // 撤销不影响历史,这里只是热一下按键
        app.cmd_apply("undo");
        app.handle(ctrl(KeyCode::Char('o')));
        assert_eq!(app.view.cursor, a, "Ctrl-o 退回上一个跳点");
        app.handle(键(KeyCode::Tab));
        assert_eq!(app.view.cursor, b, "Tab(即 Ctrl-i)前进");
        app.handle(ctrl(KeyCode::Char('o')));
        app.handle(ctrl(KeyCode::Char('o')));
        assert_eq!(app.view.cursor, start, "连续退回到起点");
    }

    /// `a,bG` 跳到那一格,`gg` 回原点;序列没走完时挂在 showcmd 上
    #[test]
    fn 坐标跳转与gg() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        for c in "100,100".chars() {
            app.handle(键(KeyCode::Char(c)));
        }
        assert_eq!(app.view.showcmd, "100,100", "没走完的序列该显示出来");
        app.handle(键(KeyCode::Char('G')));
        assert_eq!(app.view.cursor, (100, 100), "跳到那一格");
        assert!(app.view.showcmd.is_empty(), "按完就清掉");
        // gg 回原点
        app.handle(键(KeyCode::Char('g')));
        assert_eq!(app.view.showcmd, "g");
        app.handle(键(KeyCode::Char('g')));
        assert_eq!(app.view.cursor, (0, 0));
        assert!(app.view.showcmd.is_empty());
        // 单个数按 vim 的"第 n 行"算,列不动
        app.view.cursor = (7, 0);
        app.refresh_pick();
        for c in "40G".chars() {
            app.handle(键(KeyCode::Char(c)));
        }
        assert_eq!(app.view.cursor, (7, 40));
        // 纯数字前缀照旧管方向键
        app.view.cursor = (0, 0);
        app.refresh_pick();
        app.handle(键(KeyCode::Char('5')));
        assert_eq!(app.view.showcmd, "5");
        app.handle(键(KeyCode::Char('l')));
        assert_eq!(app.view.cursor, (5, 0));
        assert!(app.view.showcmd.is_empty(), "动作做完序列就没了");
        // Esc 把没走完的序列丢掉
        app.handle(键(KeyCode::Char('1')));
        app.handle(键(KeyCode::Esc));
        assert!(app.view.showcmd.is_empty());
    }

    /// `0` 只把缩放拨回 1:1,不挪画面(回原点用 gg)
    #[test]
    fn 零只回缩放不动画面() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        app.view.zoom = 2.5;
        app.view.center = Some((40.0, 12.0));
        app.view.target = None;
        app.view.pan = None;
        app.handle(键(KeyCode::Char('0')));
        assert_eq!(app.view.zoom, 1.0, "回到 1:1");
        assert_eq!(app.view.center, Some((40.0, 12.0)), "画面不该挪");
        assert_eq!(app.view.target, None);
    }

    /// 跳到很远的地方:视图必须真的跟过去,而且落在正中    /// 跳到很远的地方:视图必须真的跟过去,而且落在正中
    #[test]
    fn 远距离跳转把光标放正中() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        for c in "1000,1000G".chars() {
            app.handle(键(KeyCode::Char(c)));
        }
        assert_eq!(app.view.cursor, (1000, 1000));
        assert_eq!(app.view.target, Some((1000.0, 1000.0)), "第一次跳转就居中");
        // 模拟缓动走完
        app.view.center = Some((1000.0, 1000.0));
        app.view.target = None;
        // 再跳回来:目标必须是 (10,10) 正中,不是只走 85%
        for c in "10,10G".chars() {
            app.handle(键(KeyCode::Char(c)));
        }
        assert_eq!(app.view.cursor, (10, 10));
        assert_eq!(app.view.target, Some((10.0, 10.0)), "跳回来该直接居中");
        app.view.center = app.view.target;
        let p = render::panels(&app.doc, app.area, &app.view);
        let (cx, cy) = p.proj.cell((10.0, 10.0));
        assert_eq!(
            (cx, cy),
            (
                p.map.x as i32 + p.map.width as i32 / 2,
                p.map.y as i32 + p.map.height as i32 / 2
            ),
            "光标落在画面正中"
        );
    }

    /// 带链接文字的样例:框 a 的文字是 `[site](https://example.com) x`
    fn 链接文档() -> Document {
        链接文档目标("https://example.com")
    }

    fn 链接文档目标(target: &str) -> Document {
        let groups = vec![Group {
            id: "g0".into(),
            label: "线".into(),
            color: (0x4c, 0x8e, 0xf8),
        }];
        let layers = vec![Layer {
            name: String::new(),
            order: 0,
        }];
        let boxes = vec![
            Box {
                id: "a".into(),
                at: (0, 0),
                size: (24, 3),
                style: BoxStyle::Single,
                color: None,
                fill: None,
                layer: String::new(),
                terms: Vec::new(),
                edge: None,
            },
            Box {
                id: "b".into(),
                at: (0, 8),
                size: (1, 1),
                style: BoxStyle::Point,
                color: None,
                fill: None,
                layer: String::new(),
                terms: Vec::new(),
                edge: None,
            },
        ];
        let texts = vec![
            Text {
                id: "ta".into(),
                text: format!("[site]({target}) x"),
                at: (1, 1),
                box_id: "a".into(),
                color: None,
                bg: None,
                align: Align::Left,
                layer: String::new(),
            },
            Text {
                id: "tb".into(),
                text: "b".into(),
                at: (0, 9),
                box_id: "b".into(),
                color: None,
                bg: None,
                align: Align::Left,
                layer: String::new(),
            },
        ];
        let edges = vec![DocEdge {
            id: "e1".into(),
            from: "a".into(),
            to: "b".into(),
            dir: Dir::Forward,
            group: "g0".into(),
            style: EdgeStyle::Solid,
            color: None,
            bg: None,
            via: Vec::new(),
            corner: Corner::Rounded,
            layer: String::new(),
            from_side: None,
            to_side: None,
            head: None,
        }];
        Document::from_parts(
            "test",
            groups,
            layers,
            boxes,
            texts,
            edges,
            ViewState::default(),
        )
        .expect("样例")
    }

    fn 目标图(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("graph-tui-target-app");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(format!("{tag}.gtui"));
        std::fs::write(&p, 链接文档().save()).unwrap();
        p
    }

    /// 把光标放到链接上(测试里每次都重画一次命中表)
    fn 点在链接上(app: &mut App) {
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        app.view.cursor = (1, 1);
        app.refresh_pick();
        app.draw(&mut buf, app.area);
        assert!(app.link_at_cursor().is_some(), "光标该在链接上");
    }

    /// `self:x,y`:当前 tab 跳过去,不新开,跳转表记一条
    #[test]
    fn self链接在图内跳不新开tab() {
        let mut app = 连边应用(链接文档目标("self:10,10"));
        点在链接上(&mut app);
        let tabs = app.tabs.len();
        let jumps = app.jumps.len();
        app.handle(键(KeyCode::Enter));
        assert_eq!(app.tabs.len(), tabs, "self 不该新开 tab");
        assert_eq!(app.view.cursor, (10, 10));
        assert!(
            app.take_open_request().is_none(),
            "跳转由 App 做完,不甩给 main"
        );
        assert_eq!(app.jumps.len(), jumps + 1, "该记一个跳点");
        // 画得出来(越界坐标也不 panic)
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
    }

    /// 当前文档自己的路径 + `:x,y`:同样在本地跳
    #[test]
    fn 当前文档路径链接也在本地跳() {
        let path = 目标图("selfdoc");
        let mut app = 连边应用(链接文档目标(&format!("{}:4,5", path.display())));
        app.source = Some(path.display().to_string());
        点在链接上(&mut app);
        let tabs = app.tabs.len();
        app.handle(键(KeyCode::Enter));
        assert_eq!(app.tabs.len(), tabs, "同一份文档不该新开 tab");
        assert_eq!(app.view.cursor, (4, 5));
    }

    /// 目标已开在另一个 tab:切过去再跳
    #[test]
    fn 别的已开图档切tab后跳() {
        let other = 目标图("open-other");
        let mut app = 连边应用(链接文档目标(&format!("{}:7,8", other.display())));
        app.new_tab(Some(&other.display().to_string()));
        assert_eq!(app.tab_at, 1, "先开成第二个 tab");
        app.switch_tab(0);
        点在链接上(&mut app);
        app.handle(键(KeyCode::Enter));
        assert_eq!(app.tab_at, 1, "该切回那个 tab");
        assert_eq!(app.view.cursor, (7, 8));
        assert_eq!(app.tabs.len(), 2, "不该再多开一个");
    }

    /// 目标没开:新开 tab 再跳
    #[test]
    fn 没开过的图档新tab后跳() {
        let other = 目标图("fresh-other");
        let mut app = 连边应用(链接文档目标(&format!("{}:3,4", other.display())));
        点在链接上(&mut app);
        app.handle(键(KeyCode::Enter));
        assert_eq!(app.tabs.len(), 2, "该新开一个 tab");
        assert_eq!(app.tab_at, 1);
        assert_eq!(app.view.cursor, (3, 4));
        // 跳过去以后 Ctrl-o 要回到原来那个 tab 和格子
        app.handle(ctrl(KeyCode::Char('o')));
        assert_eq!(app.tab_at, 0, "Ctrl-o 该回到原来的 tab");
        assert_eq!(app.view.cursor, (1, 1), "回到点链接之前的位置");
    }

    /// 带位置的本地文本交给 main 的编辑器计划,不是就地跳
    #[test]
    fn 文本链接带位置给编辑器计划() {
        let dir = std::env::temp_dir().join("graph-tui-target-app");
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("note.md");
        std::fs::write(&f, "hello\n").unwrap();
        let mut app = 连边应用(链接文档目标(&format!("{}:12,4", f.display())));
        点在链接上(&mut app);
        app.handle(键(KeyCode::Enter));
        let plan = app.take_open_request().expect("该有打开计划");
        assert_eq!(
            plan,
            crate::target::OpenPlan::Editor {
                path: f,
                position: Some(crate::target::Position { a: 12, b: 4 })
            }
        );
    }

    /// Enter / gf:打开光标下的链接;有未确认选区时 Enter 仍然只确认选区
    #[test]
    fn 打开光标下的链接() {
        let mut app = 连边应用(链接文档());
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        // 光标放到链接那几个字上
        app.view.cursor = (1, 1);
        app.refresh_pick();
        app.draw(&mut buf, app.area);
        assert_eq!(app.link_at_cursor().as_deref(), Some("https://example.com"));
        app.handle(键(KeyCode::Enter));
        let req = app.take_open_request().expect("Enter 该请求打开链接");
        assert_eq!(
            req,
            crate::target::OpenPlan::Spawn {
                program: app.opener_command(),
                args: vec!["https://example.com".to_string()]
            }
        );
        // gf:同一件事
        app.handle(键(KeyCode::Char('g')));
        app.handle(键(KeyCode::Char('f')));
        assert!(app.take_open_request().is_some(), "gf 也该请求打开");
        // 没有链接:给提示,不退回"铺满画面"
        app.view.cursor = (20, 12);
        app.refresh_pick();
        app.draw(&mut buf, app.area);
        let zoom = app.view.zoom;
        app.handle(键(KeyCode::Char('g')));
        app.handle(键(KeyCode::Char('f')));
        assert!(app.take_open_request().is_none());
        assert_eq!(app.view.cursor, (20, 12), "不能跳走");
        assert_eq!(app.view.zoom, zoom, "不能变成铺满画面");
        assert!(app.view.note.as_deref().unwrap_or("").contains("没有链接"));
        // 未确认的选区优先:Enter 只确认选区
        app.view.cursor = (1, 1);
        app.refresh_pick();
        app.draw(&mut buf, app.area);
        app.view.sel = Some(RectSel {
            anchor: (0, 0),
            end: (5, 2),
            confirmed: false,
        });
        app.handle(键(KeyCode::Enter));
        assert!(app.take_open_request().is_none(), "选区优先,不打开链接");
        assert!(app.view.sel.expect("选区还在").confirmed);
    }

    /// 把 Enter 改回 confirm_selection 之后,Enter 不再打开链接
    #[test]
    fn enter改绑后不打开链接() {
        let mut opts = zh_opts();
        opts.keys.bind(
            crate::keys::Key::parse("enter").expect("解析"),
            Some(crate::keys::Action::ConfirmSelection),
        );
        let mut app = App::new(链接文档(), opts).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        app.view.cursor = (1, 1);
        app.refresh_pick();
        app.draw(&mut buf, app.area);
        app.handle(键(KeyCode::Enter));
        assert!(app.take_open_request().is_none(), "改绑后 Enter 只确认选区");
    }

    /// 菜单记得上次选了什么:再开一次直接停在那一项(换过的颜色也跟着亮着)    /// 菜单记得上次选了什么:再开一次直接停在那一项(换过的颜色也跟着亮着)
    #[test]
    fn 菜单记住上次选择() {
        let mut app = 连边应用(横排文档(2));
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        app.jump_to(0);
        let i = app.cursor_box().expect("在框上");
        // 第一次:进"框型"子菜单,选 double
        app.open_style_menu();
        let menu = app.view.menu.as_ref().expect("菜单开了");
        let top = menu.levels.len();
        assert_eq!(top, 1, "第一次开是顶层");
        // 找到"框型"那一项并进去
        let box_style = menu
            .level()
            .iter()
            .position(|it| it.title.contains("框型") || it.title.contains("box style"))
            .expect("有框型");
        app.view.menu.as_mut().unwrap().sel = box_style;
        app.menu_choose();
        assert_eq!(
            app.view.menu.as_ref().unwrap().levels.len(),
            2,
            "进了子菜单"
        );
        let dbl = app
            .view
            .menu
            .as_ref()
            .unwrap()
            .level()
            .iter()
            .position(|it| it.title == "double")
            .expect("有 double");
        app.view.menu.as_mut().unwrap().sel = dbl;
        app.menu_choose();
        assert!(app.view.menu.is_none(), "选完就关");
        assert_eq!(app.doc.boxes[i].style, crate::model::BoxStyle::Double);
        // 第二次:直接停在"框型"子菜单里的 double 上
        app.open_style_menu();
        let menu = app.view.menu.as_ref().expect("菜单开了");
        assert_eq!(menu.levels.len(), 2, "记得上次进过子菜单");
        assert_eq!(menu.sel, dbl, "停在同一个选项上");
        assert!(menu.level()[menu.sel].checked, "就是当前值");
        app.menu_cancel();
    }

    /// 菜单记忆是个"栈":多层子菜单整条路径都记,退回上一层也回到原来那一项
    #[test]
    fn 菜单记忆整条路径() {
        let mut app = 连边应用(横排文档(2));
        app.doc
            .add_edge("b0", "b1", "g0", Dir::Forward)
            .expect("连边");
        app.refresh_pick();
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        app.jump_to(0);
        app.view.pick = render::Pick::Edge(0);
        // 边菜单:端口 -> 起点 -> right
        app.open_style_menu();
        let find = |app: &App, t: &str| -> usize {
            app.view
                .menu
                .as_ref()
                .unwrap()
                .level()
                .iter()
                .position(|i| i.title == t)
                .unwrap_or_else(|| panic!("没有 {t}"))
        };
        let ports = find(&app, "端口");
        app.view.menu.as_mut().unwrap().sel = ports;
        app.menu_choose();
        let from = find(&app, "起点");
        app.view.menu.as_mut().unwrap().sel = from;
        app.menu_choose();
        let right = find(&app, "right");
        app.view.menu.as_mut().unwrap().sel = right;
        app.menu_choose();
        assert!(app.view.menu.is_none(), "选完就关");
        assert_eq!(app.doc.edges[0].from_side, Some(crate::model::Side::Right));
        // 再开:三层都还原
        app.open_style_menu();
        {
            let m = app.view.menu.as_ref().unwrap();
            assert_eq!(m.levels.len(), 3, "端口 -> 起点 -> right 三层都记着");
            assert_eq!(m.sel, right, "停在 right");
            assert_eq!(m.parents, vec!["端口".to_string(), "起点".to_string()]);
        }
        // 退回上一层:停在"起点",不是第一项
        app.handle(键(KeyCode::Char('h')));
        {
            let m = app.view.menu.as_ref().unwrap();
            assert_eq!(m.levels.len(), 2);
            assert_eq!(m.sel, from, "回到进来时那一项");
        }
        app.handle(键(KeyCode::Char('h')));
        {
            let m = app.view.menu.as_ref().unwrap();
            assert_eq!(m.levels.len(), 1);
            assert_eq!(m.sel, ports, "再退回到端口那一项");
        }
        app.menu_cancel();
    }

    /// 改了框的颜色之后,下次 s 直接停在颜色子菜单的那一格上    /// 改了框的颜色之后,下次 s 直接停在颜色子菜单的那一格上
    #[test]
    fn 菜单记住上次选的颜色() {
        let mut app = 连边应用(横排文档(2));
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        app.jump_to(0);
        let i = app.cursor_box().expect("在框上");
        // 第一次:边框色 -> 第 4 个颜色
        app.open_style_menu();
        let colour = app
            .view
            .menu
            .as_ref()
            .unwrap()
            .level()
            .iter()
            .position(|it| it.title.contains("边框色") || it.title.contains("border colour"))
            .expect("有边框色");
        app.view.menu.as_mut().unwrap().sel = colour;
        app.menu_choose();
        let want = 4usize;
        app.view.menu.as_mut().unwrap().sel = want;
        let hex = app.view.menu.as_ref().unwrap().level()[want].title.clone();
        app.menu_choose();
        let c = crate::model::PALETTE[want - 1];
        assert_eq!(app.doc.boxes[i].color, Some(c), "颜色改成了 {hex}");
        // 第二次:s 直接进颜色子菜单,停在同一格,而且它是当前值
        app.open_style_menu();
        let menu = app.view.menu.as_ref().expect("菜单开了");
        assert_eq!(menu.levels.len(), 2, "记得上次进过颜色子菜单");
        assert_eq!(menu.sel, want, "停在同一个颜色上");
        assert!(menu.level()[menu.sel].checked, "就是当前颜色");
        app.menu_cancel();
    }

    /// 框的样式菜单里不再有"改文字…",但"删除"还在
    #[test]
    fn 框样式菜单没有改文字但有删除() {
        let mut app = 连边应用(横排文档(2));
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        app.jump_to(0);
        app.open_style_menu();
        let menu = app.view.menu.as_ref().expect("菜单开了");
        for it in menu.level() {
            assert!(
                !it.title.contains("改文字") && !it.title.contains("edit text"),
                "还有改文字:{}",
                it.title
            );
        }
        let del = menu
            .level()
            .iter()
            .position(|it| it.title == "删除")
            .expect("该有删除");
        assert_eq!(del, menu.level().len() - 1, "删除在最后一项");
        app.menu_cancel();
    }

    /// 跳转历史跨标签页:在另一个 buffer 里也能 Ctrl-o 退回来
    #[test]
    fn 跳转跨标签页() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        app.jump_to(1); // 记下第一个 tab 里的位置
        let a = app.view.cursor;
        assert_eq!(app.tab_at, 0);
        // 开第二个 tab(和 gf 打开另一个文件等价)
        app.new_tab(None);
        assert_eq!(app.tab_at, 1);
        app.draw(&mut buf, app.area);
        // Ctrl-o:回到第一个 tab 的那个格子
        app.handle(ctrl(KeyCode::Char('o')));
        assert_eq!(app.tab_at, 0, "该切回原来的 tab");
        assert_eq!(app.view.cursor, a, "格子也要回去");
        // Ctrl-i:再回到第二个 tab
        app.handle(键(KeyCode::Tab));
        assert_eq!(app.tab_at, 1, "前进回到另一个 buffer");
    }

    /// 动手的地方(新建 / 改文字 / 改样式)也进跳转历史:Ctrl-o 能回到那儿    /// 动手的地方(新建 / 改文字 / 改样式)也进跳转历史:Ctrl-o 能回到那儿    /// 动手的地方(新建 / 改文字 / 改样式)也进跳转历史:Ctrl-o 能回到那儿    /// 动手的地方(新建 / 改文字 / 改样式)也进跳转历史:Ctrl-o 能回到那儿
    #[test]
    fn 编辑位置也算跳点() {
        let mut app = App::new(Document::scratch(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        // 在空处新建一个框
        let at = (-10, -10);
        app.view.cursor = at;
        app.refresh_pick();
        app.cmd_new("aa");
        assert_eq!(app.view.cursor, at, "建框不动光标");
        // 走开(普通移动不记跳点)
        let away = (30, 30);
        app.view.cursor = away;
        app.refresh_pick();
        // Ctrl-o 回到刚建框的那一格;Ctrl-i 再回到走开的位置
        app.handle(ctrl(KeyCode::Char('o')));
        assert_eq!(app.view.cursor, at, "Ctrl-o 回到建框的地方");
        app.handle(键(KeyCode::Tab));
        assert_eq!(app.view.cursor, away, "Tab(Ctrl-i)前进回去");
        // 改样式同样算一次跳点
        app.view.cursor = at;
        app.refresh_pick();
        app.cmd_apply("box double");
        app.view.cursor = away;
        app.refresh_pick();
        app.handle(ctrl(KeyCode::Char('o')));
        assert_eq!(app.view.cursor, at, "Ctrl-o 回到改样式的地方");
    }

    /// 新建框落在光标那一格;压着别的框就往右让    /// 新建框落在光标那一格;压着别的框就往右让
    #[test]
    fn 新建框落在光标处() {
        let mut app = App::new(Document::scratch(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        // 光标放在空处
        app.view.cursor = (-10, -10);
        app.refresh_pick();
        app.cmd_new("q");
        let last = app.doc.boxes.len() - 1;
        assert_eq!(app.doc.boxes[last].at, (-10, -10), "该建在光标格");
        // 光标压在一个框上:往右让,不重叠
        app.view.cursor = app.doc.boxes[0].at;
        app.refresh_pick();
        app.cmd_new("w");
        let last = app.doc.boxes.len() - 1;
        let at = app.doc.boxes[last].at;
        for b in &app.doc.boxes[..last] {
            let overlap = at.0 < b.at.0 + b.size.0 as i32
                && b.at.0 < at.0 + app.doc.boxes[last].size.0 as i32
                && at.1 < b.at.1 + b.size.1 as i32
                && b.at.1 < at.1 + 3;
            assert!(!overlap, "新框 {at:?} 压住了 {}", b.id);
        }
    }

    /// Ctrl-q 从任意格开始区块选择
    #[test]
    fn 区块选择只留v() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        let (x0, y0, x1, y1) = app.doc.bbox();
        app.view.cursor = (x0 - 2, y0 - 2);
        app.refresh_pick();
        // Ctrl-q / V 已经删掉(键位去重)
        app.handle(ctrl(KeyCode::Char('q')));
        assert!(app.view.sel.is_none(), "Ctrl-q 不该再开始选区");
        app.handle(键(KeyCode::Char('V')));
        assert!(app.view.sel.is_none(), "V 不该再开始选区");
        // v 还是那条路
        app.handle(键(KeyCode::Char('v')));
        let sel = app.view.sel.expect("v 该有选区");
        assert_eq!(sel.anchor, (x0 - 2, y0 - 2), "锚点落在光标格");
        assert!(!sel.confirmed);
        // 扩大到盖住整幅图
        app.view.cursor = (x1 + 2, y1 + 2);
        let sel = render::selected(
            &app.doc,
            &render::panels(&app.doc, app.area, &app.view),
            &app.view,
        );
        assert!(!sel.is_empty(), "大区块里该有框");
        app.handle(键(KeyCode::Esc));
        assert!(app.view.sel.is_none(), "esc 取消");
    }

    /// i / t / c 三个键走底行提示,回车执行
    #[test]
    fn 提示行建框改文字连边() {
        let mut app = App::new(Document::scratch(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        // 光标在框上时 i 是就地改字,挪到空白处才是新建
        app.handle(键(KeyCode::Char('i')));
        assert!(app.view.edit.is_some(), "框上按 i 该就地改字");
        app.view.edit = None;
        app.view.cursor = (-10, -10);
        app.refresh_pick();
        app.handle(键(KeyCode::Char('i')));
        assert!(app.view.edit.is_some(), "空白处直接建框并进就地编辑");
        for c in "Z".chars() {
            app.handle(键(KeyCode::Char(c)));
        }
        app.handle(键(KeyCode::Esc));
        assert!(app.view.edit.is_none());
        assert_eq!(app.doc.boxes.len(), 2);
        assert_eq!(app.doc.text(1).unwrap().text, "Z");
        assert_eq!(app.undo.counts().0, 2, "建框一步、写文字一步");

        // esc 取消不产生步(取消掉这次新建的框)
        app.handle(键(KeyCode::Char('i')));
        app.handle(键(KeyCode::Esc));
        assert_eq!(app.doc.boxes.len(), 2, "取消不留空框");

        // :text -> 改光标框文字(输入框那条路)
        app.view.cursor = app.doc.text(0).unwrap().at;
        app.refresh_pick();
        let i = app.cursor_box().unwrap();
        app.cmd_apply("text hello");
        assert_eq!(app.doc.text(i).unwrap().text, "hello");

        // :link -> 连到另一个框(c 现在开颜色菜单);文字已经改过,按 id 找
        let other = app.doc.boxes[1].id.clone();
        app.cmd_apply(&format!("link {other}"));
        assert_eq!(app.doc.edges.len(), 1);
    }

    /// d 和 x 一样:删掉光标处的框或选中的边
    #[test]
    fn d_删除光标处的元素() {
        let mut app = App::new(Document::scratch(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.jump_to(0);
        app.cmd_apply("new b");
        let n = app.doc.boxes.len();
        assert_eq!(n, 2);
        // 光标落在新框里:按 d 删掉它
        app.handle(键(KeyCode::Char('d')));
        assert_eq!(app.doc.boxes.len(), n - 1, "d 该删掉光标处的框");
        assert!(app.view.note.clone().unwrap_or_default().contains("剪切"));
        // 光标留在原地(空的),要删最后一个框得先挪过去
        app.jump_to(0);
        app.cmd_apply("del");
        let note = app.view.note.clone().unwrap_or_default();
        assert!(note.contains("最后一个框"), "{note}");
    }

    /// 选区:未确认时改文档被拦,确认后组删除、组样式各一步撤销
    #[test]
    fn 选区确认与组删除粘贴() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        // 框住前两个框(留一个,免得撞上"不能删光"的规则)
        let b0 = app.doc.boxes[0].at;
        let (bx, by, bw, bh) = {
            let b1 = &app.doc.boxes[1];
            (b1.at.0, b1.at.1, b1.size.0 as i32, b1.size.1 as i32)
        };
        let (rx, ry) = (bx + bw + 1, by + bh + 1);
        app.view.cursor = (b0.0 - 1, b0.1 - 1);
        app.refresh_pick();
        app.handle(键(KeyCode::Char('v')));
        app.view.cursor = (rx, ry);
        app.refresh_pick();
        let sel = app.view.sel.expect("该有选区");
        assert!(!sel.confirmed, "刚拉出来是未确认");

        // 未确认:改文档被拒绝,不压撤销
        let undo0 = app.undo.counts().0;
        let n0 = app.doc.boxes.len();
        app.handle(键(KeyCode::Char('d')));
        assert_eq!(app.doc.boxes.len(), n0, "未确认不该删");
        assert!(app.view.note.clone().unwrap_or_default().contains("Enter"));
        assert_eq!(app.undo.counts().0, undo0, "被拒绝不该压撤销");

        // 确认之后:整批删除,一步撤销,内容进剪贴板
        app.handle(键(KeyCode::Enter));
        assert!(app.view.sel.unwrap().confirmed);
        let targets = app.targets();
        assert!(targets.boxes.len() >= 2, "选区该盖住至少两个框");
        let nb = targets.boxes.len();
        app.handle(键(KeyCode::Char('d')));
        assert_eq!(app.doc.boxes.len(), n0 - nb, "该删掉选区里的框");
        assert_eq!(app.undo.counts().0, undo0 + 1, "组删除只算一步");
        let clip = app.clipboard.clone().expect("删除该写剪贴板");
        assert_eq!(clip.boxes.len(), nb);
        assert!(!clip.texts.is_empty());
        // 撤销恢复全部,剪贴板还在
        app.cmd_apply("undo");
        assert_eq!(app.doc.boxes.len(), n0, "撤销恢复全部");
        assert!(app.clipboard.is_some(), "撤销不清剪贴板");
        // 粘贴一份副本
        app.cmd_apply("paste");
        assert_eq!(app.doc.boxes.len(), n0 + nb, "粘贴出一份副本");
        // 再粘一次 id 不冲突
        let before: std::collections::HashSet<String> =
            app.doc.boxes.iter().map(|b| b.id.clone()).collect();
        app.cmd_apply("paste");
        let after: std::collections::HashSet<String> =
            app.doc.boxes.iter().map(|b| b.id.clone()).collect();
        assert!(after.len() > before.len(), "id 不该撞");
        // 再删一次同样的选区会删光?整体拒绝的分支
        app.view.sel = None;
        app.refresh_pick();
        app.view.cursor = (b0.0 - 1, b0.1 - 1);
        app.handle(键(KeyCode::Char('v')));
        app.view.cursor = (rx, ry);
        app.handle(键(KeyCode::Enter));
    }

    /// `:` 补全:动词前缀、取值枚举、框名/图层名/文件名
    #[test]
    fn 命令行补全() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        // 动词前缀
        app.view.cmd = Some("fi".to_string());
        app.complete_cmd(false);
        assert_eq!(app.view.cmd.as_deref(), Some("fill"));
        assert!(app.view.cmd_menu.is_some(), "该弹补全窗");
        // 取值
        app.view.cmd = Some("box ".to_string());
        app.view.cmd_menu = None;
        app.complete_cmd(false);
        assert_eq!(app.view.cmd.as_deref(), Some("box point"), "第一个候选");
        app.complete_cmd(false);
        assert_eq!(app.view.cmd.as_deref(), Some("box none"), "Tab 轮换");
        app.complete_cmd(true);
        assert_eq!(app.view.cmd.as_deref(), Some("box point"), "Shift-Tab 往回");
        // dir 的四个值
        app.view.cmd = Some("dir b".to_string());
        app.view.cmd_menu = None;
        app.complete_cmd(false);
        assert_eq!(app.view.cmd.as_deref(), Some("dir back"), "按前缀过滤");
        // 框名:link 补 id/文字
        app.view.cmd = Some("link b".to_string());
        app.view.cmd_menu = None;
        app.complete_cmd(false);
        let line = app.view.cmd.clone().unwrap();
        assert!(line.starts_with("link b"), "{line}");
        // 输入会把弹窗收掉
        app.handle(键(KeyCode::Enter));
        assert!(app.view.cmd_menu.is_none());
    }

    /// 命令行右侧的路径提示:存盘会写到哪、`:e` 会从哪读
    #[test]
    fn 命令行路径提示() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        // 没有来源:明说"还没文件名"
        app.view.cmd = Some("w".to_string());
        app.refresh_cmd_hint();
        let hint = app.view.cmd_hint.clone().unwrap();
        assert!(hint.contains("还没有文件名"), "{hint}");

        // .dot 来源:默认写到同目录的 <stem>.gtui,并说明源文件不动
        app.source = Some("/tmp/demo/chart.dot".to_string());
        app.view.cmd = Some("w".to_string());
        app.refresh_cmd_hint();
        let hint = app.view.cmd_hint.clone().unwrap();
        assert!(hint.contains("/tmp/demo/chart.gtui"), "{hint}");
        assert!(hint.contains("不动"), "{hint}");

        // .gtui 来源:覆盖原文件
        app.source = Some("/tmp/demo/a.gtui".to_string());
        app.refresh_cmd_hint();
        let hint = app.view.cmd_hint.clone().unwrap();
        assert!(
            hint.contains("/tmp/demo/a.gtui") && hint.contains("覆盖"),
            "{hint}"
        );

        // 手写路径:提示解析后的绝对路径,输入本身不变
        app.view.cmd = Some("w out/x.gtui".to_string());
        app.refresh_cmd_hint();
        let hint = app.view.cmd_hint.clone().unwrap();
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(hint, cwd.join("out/x.gtui").to_string_lossy().to_string());
        assert_eq!(app.view.cmd.as_deref(), Some("w out/x.gtui"));

        // :e 空参数说明回车弹列表
        app.view.cmd = Some("e".to_string());
        app.refresh_cmd_hint();
        let hint = app.view.cmd_hint.clone().unwrap();
        assert!(hint.contains("文件列表"), "{hint}");
        // 无关动词:不提示
        app.view.cmd = Some("notes on".to_string());
        app.refresh_cmd_hint();
        assert!(app.view.cmd_hint.is_none());
        // 回车后清掉
        app.view.cmd = Some("w".to_string());
        app.refresh_cmd_hint();
        app.handle(键(KeyCode::Enter));
        assert!(app.view.cmd_hint.is_none());
    }

    /// `:w ` 空参数按 Tab 填默认路径;参数带空格时整段替换
    #[test]
    fn 制表键填默认路径与空格参数() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        // 没有来源:不填,说一声
        app.view.cmd = Some("w ".to_string());
        app.complete_cmd(false);
        assert_eq!(app.view.cmd.as_deref(), Some("w "), "空白缓冲不瞎填");
        app.view.cmd = Some("w".to_string());
        app.complete_cmd(false);
        assert_eq!(app.view.cmd.as_deref(), Some("w"), "没空格也不瞎填");
        assert!(app.view.note.is_some());

        // .dot 来源:填绝对路径,还能接着编辑
        app.source = Some("/tmp/demo/chart.dot".to_string());
        app.view.cmd = Some("w ".to_string());
        app.complete_cmd(false);
        assert_eq!(app.view.cmd.as_deref(), Some("w /tmp/demo/chart.gtui"));
        assert!(app.view.cmd_hint.as_deref().unwrap().contains("chart.gtui"));

        // 参数带空格:补全整段替换,不能按最后一个空格切
        let dir = std::env::temp_dir().join(format!("gtui-tab-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a b.gtui"), "{}").unwrap();
        let path = format!("{}/a", dir.to_str().unwrap());
        app.view.cmd = Some(format!("w {path}"));
        app.view.cmd_menu = None;
        app.complete_cmd(false);
        let line = app.view.cmd.clone().unwrap();
        assert_eq!(line, format!("w {}/a b.gtui", dir.to_str().unwrap()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 磁盘上的源文件被外部改过:没本地改动就自动重新载入,
    /// 有改动就标 stale,`R` 两下 / `:e!` 才是强制重读
    #[test]
    fn 发现磁盘改动并重新载入() {
        let dir = std::env::temp_dir().join(format!("gtui-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("w.gtui").to_str().unwrap().to_string();
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.cmd_apply(&format!("w {path}"));
        assert!(!app.check_disk(), "刚存过:没变化");
        let mine = label_of(&app.doc, 0);
        assert!(!mine.is_empty());

        // 外部改:另一个 App 往同一个路径存一版文字不同的
        let mut other = App::new(样例文档(), zh_opts()).expect("开局");
        other.doc.set_text(0, "外部改的".to_string());
        other.save_document(&path).expect("外部写入");
        // 本地没动过 -> 自动换成磁盘上的版本
        assert!(app.check_disk(), "该发现改动");
        assert_eq!(label_of(&app.doc, 0), "外部改的", "没本地改动:直接重新载入");
        assert!(!app.view.stale);
        assert!(app.view.note.as_deref().unwrap().contains("重新载入"));

        // 本地改一行,再让磁盘变:只标 stale,不动文档
        app.view.pick = render::Pick::Box(0);
        app.view.cursor = app.doc.text(0).map(|t| t.at).unwrap_or((2, 2));
        app.cmd_apply("text 本地改过");
        other.doc.set_text(0, "又改了一次".to_string());
        other.save_document(&path).expect("外部再写");
        assert!(app.check_disk());
        assert!(app.view.stale, "有本地改动:只提醒");
        assert_eq!(label_of(&app.doc, 0), "本地改过", "文档不该被换掉");
        assert!(app.view.note.as_deref().unwrap().contains("R"));

        // 第一次 R 只提醒,第二次才丢改动重读
        app.reload_step();
        assert_eq!(label_of(&app.doc, 0), "本地改过");
        app.reload_step();
        assert_eq!(label_of(&app.doc, 0), "又改了一次", "第二下 R 真的重读");
        assert!(!app.view.stale);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `:e!` 不论有没有本地改动都强制重读
    #[test]
    fn 强制重新载入() {
        let dir = std::env::temp_dir().join(format!("gtui-force-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("f.gtui").to_str().unwrap().to_string();
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.cmd_apply(&format!("w {path}"));
        app.view.pick = render::Pick::Box(0);
        app.view.cursor = app.doc.text(0).map(|t| t.at).unwrap_or((2, 2));
        app.cmd_apply("text 改了");
        let mut other = App::new(样例文档(), zh_opts()).expect("开局");
        other.doc.set_text(0, "磁盘上的".to_string());
        other.save_document(&path).expect("外部写入");
        app.cmd_apply("e!");
        assert_eq!(label_of(&app.doc, 0), "磁盘上的", ":e! 不理会本地改动");
        assert!(!app.view.stale);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 缩放锚在指针/光标:缩放前后同一屏幕格下面的画布坐标不变
    #[test]
    fn 缩放锚在光标() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.view.center = None;
        app.view.zoom = 1.0;
        let p = render::panels(&app.doc, app.area, &app.view);
        // 光标所在的屏幕格就是锚;记下这一格下面的画布坐标,缩放后它还得在这一格
        let target = app.doc.center(0);
        app.view.cursor = (target.0.round() as i32, target.1.round() as i32);
        app.view.pick = render::Pick::Box(0);
        let at = app.cursor_screen_cell();
        let point = p.proj.canvas_f(at.0, at.1);

        // 放大三档:目标点始终还在那一格(允许 1 格取整误差)
        for _ in 0..3 {
            app.zoom_at(app.view.zoom * ZOOM_STEP, at);
            let p = render::panels(&app.doc, app.area, &app.view);
            let now = p.proj.cell(point);
            assert!(
                (now.0 - at.0).abs() <= 1 && (now.1 - at.1).abs() <= 1,
                "锚点漂了: {at:?} -> {now:?} (zoom {})",
                app.view.zoom
            );
        }
        assert!(app.view.zoom > 1.5, "该放大了:{}", app.view.zoom);
        // 缩小回 1:1 附近会吸附到正好 1.0
        app.zoom_at(1.02, at);
        assert_eq!(app.view.zoom, 1.0, "1:1 附近要吸附");
    }

    /// 从缩放状态进编辑:切回 1:1 但不跳,光标还在原来那一格附近
    #[test]
    fn 编辑切回一比一不跳() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.view.center = None;
        let target = app.doc.center(0);
        app.view.cursor = (target.0.round() as i32, target.1.round() as i32);
        app.view.pick = render::Pick::Box(0);
        app.view.zoom = 2.0;
        let before = app.cursor_screen_cell();
        app.require_edit_scale();
        assert_eq!(app.view.zoom, 1.0);
        let after = app.cursor_screen_cell();
        assert!(
            (after.0 - before.0).abs() <= 1 && (after.1 - before.1).abs() <= 1,
            "切回 1:1 时跳了: {before:?} -> {after:?}"
        );
        assert!(app.view.note.as_deref().unwrap().contains("1:1"));
    }

    /// 滚轮在指针处缩放,不是绕视图中心
    #[test]
    fn 滚轮锚在指针() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.view.center = None;
        app.view.zoom = 1.0;
        let p = render::panels(&app.doc, app.area, &app.view);
        // 指针停在某个框上
        let target = app.doc.center(0);
        let at = p.proj.cell(target);
        let me = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollUp,
            column: at.0.clamp(0, 79) as u16,
            row: at.1.clamp(0, 23) as u16,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        // 用钳过之后的格子当锚,比对同样钳过的目标
        let anchor = (me.column as i32, me.row as i32);
        let point = p.proj.canvas_f(anchor.0, anchor.1);
        app.handle_mouse(me);
        let p = render::panels(&app.doc, app.area, &app.view);
        let now = p.proj.cell(point);
        assert!(
            (now.0 - anchor.0).abs() <= 1 && (now.1 - anchor.1).abs() <= 1,
            "指针下的点跑了: {anchor:?} -> {now:?}"
        );
        assert!(
            (app.view.zoom - WHEEL_STEP).abs() < 1e-9,
            "{}",
            app.view.zoom
        );
        let _ = target;
    }

    /// 鼠标手势:从边框拖到另一个框 = 连边;拖到空白取消;拖端点 = 改接
    fn mev(kind: crossterm::event::MouseEventKind, x: u16, y: u16) -> crossterm::event::MouseEvent {
        crossterm::event::MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: crossterm::event::KeyModifiers::NONE,
        }
    }

    #[test]
    fn 拖拽连边与重连() {
        use crossterm::event::{MouseButton, MouseEventKind};
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 120, 40);
        app.draw(&mut ratatui::buffer::Buffer::empty(app.area), app.area);
        // 两个框都在 1:1,屏幕上直接取它们的矩形
        let p = render::panels(&app.doc, app.area, &app.view);
        let ra = render::rect_of(&app.doc, 0, &p.proj);
        // 丢到第三个框:前两个本来就有边,重复边会被去重
        let rb = render::rect_of(&app.doc, 2, &p.proj);
        let (bx, by) = ((rb.0 + rb.2 / 2) as u16, (rb.1 + rb.3 / 2) as u16);
        // 按在 A 的上边框,拖到 B 内部,松开
        let top = (ra.0 + 1) as u16;
        app.handle_mouse(mev(
            MouseEventKind::Down(MouseButton::Left),
            top,
            ra.1 as u16,
        ));
        assert!(
            matches!(app.drag, Some(Drag::Link { .. })),
            "边框起拖该进连边"
        );
        app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Left), bx, by));
        app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Left), bx, by));
        assert!(
            app.doc
                .edges
                .iter()
                .any(|e| e.from == app.doc.boxes[0].id && e.to == app.doc.boxes[2].id),
            "该建出边:{:?}",
            app.doc
                .edges
                .iter()
                .map(|e| (&e.from, &e.to))
                .collect::<Vec<_>>()
        );
        // 拖到空白:取消,不加边
        let before = app.doc.edges.len();
        app.draw(&mut ratatui::buffer::Buffer::empty(app.area), app.area);
        let p = render::panels(&app.doc, app.area, &app.view);
        let ra = render::rect_of(&app.doc, 0, &p.proj);
        app.handle_mouse(mev(
            MouseEventKind::Down(MouseButton::Left),
            top,
            ra.1 as u16,
        ));
        app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Left), 3, 2));
        app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Left), 3, 2));
        assert_eq!(app.doc.edges.len(), before, "空白处松开不该建边");
        assert!(app.view.link.is_none(), "预览要收掉");

        // 端点重连:选中一条边,按它的端点格拖到另一个框
        app.draw(&mut ratatui::buffer::Buffer::empty(app.area), app.area);
        let p = render::panels(&app.doc, app.area, &app.view);
        let ei = 0;
        let ends = app.usable_hit().edge_ends.get(&ei).copied().expect("端点");
        let Some(other) = (0..app.doc.boxes.len()).find(|&i| {
            let r = render::rect_of(&app.doc, i, &p.proj);
            let c = (r.0 + r.2 / 2, r.1 + r.3 / 2);
            c != ends.0 && c != ends.1
        }) else {
            return;
        };
        let to_id = app.doc.boxes[other].id.clone();
        app.view.pick = render::Pick::Edge(ei);
        let (ex, ey) = (ends.1 .0 as u16, ends.1 .1 as u16);
        app.handle_mouse(mev(MouseEventKind::Down(MouseButton::Left), ex, ey));
        assert!(matches!(app.drag, Some(Drag::Reconnect { .. })), "端点起拖");
        let r = render::rect_of(&app.doc, other, &p.proj);
        let (cx, cy) = ((r.0 + r.2 / 2) as u16, (r.1 + r.3 / 2) as u16);
        app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Left), cx, cy));
        app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Left), cx, cy));
        assert_eq!(app.doc.edges[ei].to, to_id, "终点该改接到另一个框");
    }

    /// 右键拖过阈值 = 平移;没拖过(含 1 格微动)= 在按下格弹菜单
    #[test]
    fn 右键拖动平移单击弹菜单() {
        use crossterm::event::{MouseButton, MouseEventKind};
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 120, 40);
        app.draw(&mut ratatui::buffer::Buffer::empty(app.area), app.area);
        let before = app.view.center;

        // 1) 拖 3 格:平移,不弹菜单
        app.handle_mouse(mev(MouseEventKind::Down(MouseButton::Right), 40, 12));
        assert!(app.view.menu.is_none(), "按下时不该弹菜单");
        app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Right), 43, 12));
        assert_ne!(app.view.center, before, "拖过阈值该平移");
        app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Right), 43, 12));
        assert!(app.view.menu.is_none(), "拖过就不弹菜单");
        assert!(app.drag.is_none(), "松手清掉拖动状态");

        // 2) 微动 1 格:算单击,菜单在按下格打开
        app.view.center = before;
        let p = render::panels(&app.doc, app.area, &app.view);
        let (px, py) = (p.map.x + 4, p.map.y + 4);
        app.handle_mouse(mev(MouseEventKind::Down(MouseButton::Right), px, py));
        app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Right), px + 1, py));
        assert_eq!(app.view.center, before, "1 格不该平移");
        app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Right), px + 1, py));
        let menu = app.view.menu.as_ref().expect("该弹菜单");
        assert_eq!(menu.at, (px, py), "菜单开在按下格");
        app.menu_cancel();

        // 3) 拖出去再拖回原点:已经越过阈值,松手仍不开菜单
        app.handle_mouse(mev(MouseEventKind::Down(MouseButton::Right), px, py));
        app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Right), px + 5, py));
        app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Right), px, py));
        app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Right), px, py));
        assert!(app.view.menu.is_none(), "拖过再回来也不弹菜单");

        // 4) 菜单开着时右键只取消,不开始平移
        app.open_menu(px, py);
        assert!(app.view.menu.is_some());
        let center = app.view.center;
        app.handle_mouse(mev(MouseEventKind::Down(MouseButton::Right), px, py));
        assert!(app.view.menu.is_none(), "右键该先取消菜单");
        app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Right), px + 6, py));
        app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Right), px + 6, py));
        assert_eq!(app.view.center, center, "取消菜单之后这一下不该平移");
    }

    /// 从框上按右键拖:只平移,不动框、不改选区、不压撤销
    /// 右键连边途中:指针不在目标框上时预览跟光标,进入目标框时接到框
    #[test]
    fn 右键连边途中跟随指针() {
        use crossterm::event::{MouseButton, MouseEventKind};
        let mut app = 连边应用(横排文档(2));
        let p = render::panels(&app.doc, app.area, &app.view);
        let r0 = render::rect_of(&app.doc, 0, &p.proj);
        let r1 = render::rect_of(&app.doc, 1, &p.proj);
        let (sx, sy) = ((r0.0 + 1) as u16, (r0.1 + 1) as u16);
        app.handle_mouse(mev(MouseEventKind::Down(MouseButton::Right), sx, sy));
        // 空白处:预览跟光标,目标为空
        // 框下面几行是空的:指针挪到那儿就没有目标
        let (ex, ey) = ((r0.0 + 2) as u16, (r0.1 + r0.3 + 3) as u16);
        app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Right), ex, ey));
        let link = app.view.link.clone().expect("该有连边预览");
        assert_eq!(link.sources, vec![0]);
        assert!(link.targets.is_empty(), "空白处没有目标框");
        assert_eq!(
            app.view.cursor,
            p.proj.canvas(ex as i32, ey as i32),
            "光标跟着指针走"
        );
        // 进入目标框:目标接上
        let (tx, ty) = ((r1.0 + 1) as u16, (r1.1 + 1) as u16);
        app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Right), tx, ty));
        let link = app.view.link.clone().expect("该还在连边");
        assert_eq!(link.targets, vec![1], "指针下的框成为目标");
        // 回到空白处:目标清掉
        app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Right), ex, ey));
        assert!(app
            .view
            .link
            .as_ref()
            .map(|l| l.targets.is_empty())
            .unwrap_or(false));
        app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Right), ex, ey));
        assert!(app.view.link.is_none(), "松开后清掉");
    }

    /// 右键在 point / none 节点上也能起拖连边;在边上则是平移
    #[test]
    fn 右键在点节点和边上() {
        use crossterm::event::{MouseButton, MouseEventKind};
        // point 节点
        let mut doc = 横排文档(2);
        doc.boxes[1].style = crate::model::BoxStyle::Point;
        doc.rebuild();
        let mut app = 连边应用(doc);
        let p = render::panels(&app.doc, app.area, &app.view);
        let point = 1usize;
        let r = render::rect_of(&app.doc, point, &p.proj);
        app.handle_mouse(mev(
            MouseEventKind::Down(MouseButton::Right),
            r.0 as u16,
            r.1 as u16,
        ));
        assert!(
            matches!(app.drag, Some(Drag::RightLink { .. })),
            "点节点上该起连边"
        );
        app.handle_mouse(mev(
            MouseEventKind::Up(MouseButton::Right),
            r.0 as u16,
            r.1 as u16,
        ));

        // 边上:按空白处理 -> 平移
        let mut app = 连边应用(样例文档());
        app.draw(&mut ratatui::buffer::Buffer::empty(app.area), app.area);
        let (ex, ey) = app
            .hit
            .edge_cells
            .values()
            .flatten()
            .find(|c| app.doc.box_at_cell(**c).is_none())
            .copied()
            .expect("样例该有边笔画");
        let (sx, sy) = (ex as u16, ey as u16);
        app.handle_mouse(mev(MouseEventKind::Down(MouseButton::Right), sx, sy));
        assert!(
            matches!(app.drag, Some(Drag::RightPan { .. })),
            "边上该按空白处理"
        );
    }

    #[test]
    fn 右键在框上拖动是连边() {
        use crossterm::event::{MouseButton, MouseEventKind};
        let mut app = 连边应用(横排文档(2));
        let p = render::panels(&app.doc, app.area, &app.view);
        let r0 = render::rect_of(&app.doc, 0, &p.proj);
        let r1 = render::rect_of(&app.doc, 1, &p.proj);
        let (sx, sy) = ((r0.0 + 1) as u16, (r0.1 + 1) as u16);
        let (tx, ty) = ((r1.0 + 1) as u16, (r1.1 + 1) as u16);
        let center = app.view.center;
        let edges0 = app.doc.edges.len();
        let at0: Vec<(i32, i32)> = app.doc.boxes.iter().map(|b| b.at).collect();
        app.handle_mouse(mev(MouseEventKind::Down(MouseButton::Right), sx, sy));
        app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Right), tx, ty));
        assert!(app.view.link.is_some(), "拖动中途该有连边预览");
        app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Right), tx, ty));
        assert_eq!(app.view.center, center, "框上拖不再平移");
        assert_eq!(
            app.doc.boxes.iter().map(|b| b.at).collect::<Vec<_>>(),
            at0,
            "框不该动"
        );
        assert_eq!(app.doc.edges.len(), edges0 + 1, "该建一条边");
        assert!(app.view.link.is_none(), "松开后连边状态清掉");

        // 松开在空白处:不建边
        let mut app = 连边应用(横排文档(2));
        let p = render::panels(&app.doc, app.area, &app.view);
        let r0 = render::rect_of(&app.doc, 0, &p.proj);
        let (sx, sy) = ((r0.0 + 1) as u16, (r0.1 + 1) as u16);
        let (ex, ey) = ((p.map.x + 2), (p.map.y + 2));
        let edges0 = app.doc.edges.len();
        app.handle_mouse(mev(MouseEventKind::Down(MouseButton::Right), sx, sy));
        app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Right), ex, ey));
        app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Right), ex, ey));
        assert_eq!(app.doc.edges.len(), edges0, "空白处松开不建边");
        assert!(app.view.link.is_none());
        assert!(app.view.note.clone().unwrap_or_default().contains("没连边"));

        // 没拖过阈值:弹菜单
        let mut app = 连边应用(横排文档(2));
        let p = render::panels(&app.doc, app.area, &app.view);
        let r0 = render::rect_of(&app.doc, 0, &p.proj);
        let (sx, sy) = ((r0.0 + 1) as u16, (r0.1 + 1) as u16);
        app.handle_mouse(mev(MouseEventKind::Down(MouseButton::Right), sx, sy));
        app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Right), sx + 1, sy));
        app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Right), sx + 1, sy));
        assert!(app.view.menu.is_some(), "没拖过 2 格就是单击");
        assert_eq!(app.doc.edges.len(), 0);
    }

    /// 拖动靠近视口边缘时自动平移    /// 拖动靠近视口边缘时自动平移
    #[test]
    fn 拖拽到边缘自动平移() {
        use crossterm::event::{MouseButton, MouseEventKind};
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 120, 40);
        app.draw(&mut ratatui::buffer::Buffer::empty(app.area), app.area);
        app.view.center = Some((0.0, 0.0));
        let p = render::panels(&app.doc, app.area, &app.view);
        let ra = render::rect_of(&app.doc, 0, &p.proj);
        app.handle_mouse(mev(
            MouseEventKind::Down(MouseButton::Left),
            (ra.0 + 1) as u16,
            ra.1 as u16,
        ));
        let before = app.view.center;
        // 拖到地图右边最边上
        let x = p.map.x + p.map.width - 1;
        app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Left), x, 5));
        assert_ne!(app.view.center, before, "靠边该自动平移");
        app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Left), x, 5));
    }

    /// 多文档:两个 tab 各自保留文档/光标/缩放/撤销,剪贴板跨 tab
    #[test]
    fn 多tab互不干扰() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        // tab 1:改文字、缩放
        app.view.pick = render::Pick::Box(0);
        app.view.cursor = app.doc.text(0).unwrap().at;
        app.cmd_apply("text tab-one");
        app.view.zoom = 2.0;
        let undo1 = app.undo.counts();
        // 新开 tab:内容/光标/缩放归新文档
        app.new_tab(None);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.tab_at, 1);
        assert_ne!(label_of(&app.doc, 0), "tab-one", "新 tab 是空文档");
        assert_eq!(app.view.zoom, 1.0);
        app.view.pick = render::Pick::Box(0);
        app.view.cursor = app.doc.text(0).unwrap().at;
        app.cmd_apply("text tab-two");
        // 切回 tab 1:内容、缩放、撤销历史都还在
        app.switch_tab(0);
        assert_eq!(label_of(&app.doc, 0), "tab-one");
        assert_eq!(app.view.zoom, 2.0);
        assert_eq!(app.undo.counts(), undo1);
        // 在 tab 1 撤销不影响 tab 2
        app.undo_step();
        assert_ne!(label_of(&app.doc, 0), "tab-one");
        app.switch_tab(1);
        assert_eq!(label_of(&app.doc, 0), "tab-two");
    }

    /// 脏 tab 关不掉,`!` 才能丢;关掉后落到相邻 tab
    #[test]
    fn 关闭tab要看脏标记() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.new_tab(None);
        app.view.pick = render::Pick::Box(0);
        app.view.cursor = app.doc.text(0).unwrap().at;
        app.cmd_apply("text dirty");
        app.cmd_apply("tabclose");
        assert_eq!(app.tabs.len(), 2, "脏 tab 不该被关掉");
        app.cmd_apply("tabclose!");
        assert_eq!(app.tabs.len(), 1);
        assert_eq!(app.tab_at, 0);
        // 只剩一个 tab 时关闭 = 退出
        app.cmd_apply("tabclose!");
        assert!(app.quit);
    }

    /// 多 tab 的 :tabs 列表与切换命令
    #[test]
    fn tab命令与列表() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.new_tab(None);
        app.cmd_apply("tab 1");
        assert_eq!(app.tab_at, 0);
        app.cmd_apply("tabnext");
        assert_eq!(app.tab_at, 1);
        app.cmd_apply("tabnext");
        assert_eq!(app.tab_at, 0, "循环");
        app.cmd_apply("tabprev");
        assert_eq!(app.tab_at, 1);
        app.cmd_apply("tabs");
        let list = app.view.note.clone().unwrap();
        assert!(list.contains("1") && list.contains("2"), "{list}");
    }

    /// 任意缩放下拖框:不切 1:1,框/光标按画布位移走(直接用 Drag 状态,绕开边框手势)
    #[test]
    fn 缩放下拖框() {
        use crossterm::event::{MouseButton, MouseEventKind};
        for (zoom, mouse_steps, want_cells) in [(2.0, 2, 1), (0.5, 1, 2), (1.0, 3, 3)] {
            let mut app = App::new(样例文档(), zh_opts()).expect("开局");
            app.area = Rect::new(0, 0, 120, 40);
            app.view.zoom = zoom;
            app.draw(&mut ratatui::buffer::Buffer::empty(app.area), app.area);
            let p = render::panels(&app.doc, app.area, &app.view);
            let before = app.doc.boxes[0].at;
            app.view.cursor = before;
            // 每次都从同一屏幕格起拖,位移只取决于鼠标走了几格
            let from = (10u16, 10u16);
            app.drag = Some(Drag::Boxes {
                from,
                at: vec![(0, before)],
                proj: p.proj,
                cursor0: app.view.cursor,
                sel0: None,
                began: false,
            });
            let undo_before = app.undo.counts();
            app.handle_mouse(mev(
                MouseEventKind::Drag(MouseButton::Left),
                from.0 + mouse_steps,
                from.1,
            ));
            assert_eq!(app.view.zoom, zoom, "拖框不该改缩放");
            assert!(app.view.drag_hide_sel, "拖动期间该藏起选区矩形");
            assert_eq!(
                app.doc.boxes[0].at.0,
                before.0 + want_cells,
                "zoom {zoom}: 鼠标走 {mouse_steps} 格该挪 {want_cells} 格"
            );
            assert_eq!(app.view.cursor.0, before.0 + want_cells, "光标跟着走");
            let undo_after = app.undo.counts();
            assert_eq!(undo_after.0, undo_before.0 + 1, "开始拖就压一步");
            app.handle_mouse(mev(
                MouseEventKind::Up(MouseButton::Left),
                from.0 + mouse_steps,
                from.1,
            ));
            assert!(!app.view.drag_hide_sel, "松手该恢复选区矩形");
            assert_eq!(
                app.undo.counts().0,
                undo_before.0 + 1,
                "一次拖动只有一步撤销"
            );
            assert_eq!(app.view.pick.box_at(), Some(0), "松手后光标仍在该框");
        }
    }

    /// 多选拖动:相对位置不变,光标与选区按同一位移走;拖十次不漂移
    #[test]
    fn 多选拖框与漂移() {
        use crossterm::event::{MouseButton, MouseEventKind};
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 120, 40);
        app.view.zoom = 1.0;
        // 选两个框
        app.view.sel = Some(RectSel {
            anchor: (0, 0),
            end: (20, 20),
            confirmed: true,
        });
        app.draw(&mut ratatui::buffer::Buffer::empty(app.area), app.area);
        let p = render::panels(&app.doc, app.area, &app.view);
        let r = render::rect_of(&app.doc, 0, &p.proj);
        let (sx, sy) = ((r.0 + r.2 / 2) as u16, (r.1 + r.3 / 2) as u16);
        let at0: Vec<(i32, i32)> = app.doc.boxes.iter().map(|b| b.at).collect();
        let sel0 = app.view.sel.unwrap();
        for _ in 0..10 {
            app.handle_mouse(mev(MouseEventKind::Down(MouseButton::Left), sx, sy));
            app.handle_mouse(mev(MouseEventKind::Drag(MouseButton::Left), sx + 1, sy));
            app.handle_mouse(mev(MouseEventKind::Up(MouseButton::Left), sx + 1, sy));
        }
        for (i, &(x, _)) in at0.iter().enumerate() {
            assert_eq!(
                app.doc.boxes[i].at.0,
                x + 10,
                "框 {i} 该整体挪 10 格,不漂移"
            );
        }
        let sel1 = app.view.sel.expect("选区还在");
        assert_eq!(sel1.anchor.0, sel0.anchor.0 + 10, "选区矩形跟着走");
        assert_eq!(sel1.end.0, sel0.end.0 + 10);
    }

    /// 图例已移除:`:legend` 既不在补全里,也不再是命令(补全表和帮助都不该提到它)
    #[test]
    fn 图例命令已经移除() {
        assert!(!VERBS.contains(&"legend"), ":legend 不该再是命令:{VERBS:?}");
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.cmd_apply("legend on");
        let note = app.view.note.clone().unwrap_or_default();
        assert!(note.contains("legend"), "该报未知命令:{note}");
        assert!(
            note.contains("未知命令") || note.contains("unknown command"),
            "{note}"
        );
    }

    /// 不存在的 .gtui 当成新文件(和 nvim 一样);缺 .dot 仍然是错
    #[test]
    fn 不存在的gtui是新文件() {
        let dir = std::env::temp_dir().join(format!(
            "graph-tui-newfile-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let gtui = dir.join("fresh.gtui");
        let dot = dir.join("fresh.dot");
        let (doc, fresh) = App::read_doc(gtui.to_str().unwrap()).expect("新文件该能开");
        assert!(fresh, "标记成新文件");
        assert_eq!(doc.boxes.len(), 1, "空白文档带一个起始框");
        assert!(
            App::read_doc(dot.to_str().unwrap()).is_err(),
            "缺 .dot 仍然报错"
        );
        // 真的文件:不标记成新文件
        std::fs::write(&gtui, Document::scratch().save()).expect("写盘");
        let (_, fresh) = App::read_doc(gtui.to_str().unwrap()).expect("读得回来");
        assert!(!fresh);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 菜单:选中即预览(不改撤销),点一下才真正应用;取消回滚
    #[test]
    fn 菜单预览与回滚() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        app.jump_to(0);
        let n0 = app.doc.boxes.len();
        let undo0 = app.undo.counts().0;
        let p = render::panels(&app.doc, app.area, &app.view);
        let (cx, cy) = p
            .proj
            .cell((app.view.cursor.0 as f64, app.view.cursor.1 as f64));
        app.open_menu(cx as u16, cy as u16);
        // 第一项是"框型"(子菜单):不动文档
        assert_eq!(app.doc.boxes.len(), n0);

        // 进"边框色"子菜单,挑一个颜色预览:颜色变了,但不留撤销步
        let colour = app
            .view
            .menu
            .as_ref()
            .unwrap()
            .level()
            .iter()
            .position(|i| i.title == "边框色")
            .expect("框菜单该有边框色");
        app.view.menu.as_mut().unwrap().sel = colour;
        app.menu_choose();
        assert_eq!(
            app.view.menu.as_ref().unwrap().levels.len(),
            2,
            "进了子菜单"
        );
        let pick = 3usize;
        app.view.menu.as_mut().unwrap().sel = pick;
        app.menu_preview();
        assert_eq!(
            app.doc.boxes[0].color,
            Some(crate::model::PALETTE[pick - 1]),
            "预览该立刻生效"
        );
        assert_eq!(app.undo.counts().0, undo0, "预览不压撤销");

        // 取消:回到原样
        app.menu_cancel();
        assert_eq!(app.doc.boxes[0].color, None, "取消该回滚");
        assert!(app.view.menu.is_none());

        // 再开一次:取消过就不该记(菜单回到顶层),走完一遍并应用
        let victim = app.view.pick.box_at().expect("光标该在框上");
        app.open_menu(cx as u16, cy as u16);
        assert_eq!(
            app.view.menu.as_ref().unwrap().levels.len(),
            1,
            "取消不算选过"
        );
        app.view.menu.as_mut().unwrap().sel = colour;
        app.menu_choose();
        app.view.menu.as_mut().unwrap().sel = pick;
        app.menu_choose();
        assert_eq!(
            app.doc.boxes[victim].color,
            Some(crate::model::PALETTE[pick - 1]),
            "应用该改掉"
        );
        assert_eq!(app.undo.counts().0, undo0 + 1, "应用才算一步");
        // 这一次真的记下了:再开直接停在颜色子菜单那一格
        app.open_menu(cx as u16, cy as u16);
        {
            let m = app.view.menu.as_ref().unwrap();
            assert_eq!(m.levels.len(), 2, "该记得上次进过颜色子菜单");
            assert_eq!(m.sel, pick, "该停在上次选的颜色上");
            assert!(m.level()[m.sel].checked, "就是当前颜色");
        }
        app.menu_cancel();
    }

    /// 右键菜单:按光标处的东西给一层,子菜单能进能退
    #[test]
    fn 右键菜单按元素给项并能执行() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        // 放到第一个框上再开菜单
        app.jump_to(0);
        let p = render::panels(&app.doc, app.area, &app.view);
        let (cx, cy) = p
            .proj
            .cell((app.view.cursor.0 as f64, app.view.cursor.1 as f64));
        app.open_menu(cx as u16, cy as u16);
        let menu = app.view.menu.clone().expect("该弹菜单");
        let titles: Vec<String> = menu.level().iter().map(|i| i.title.clone()).collect();
        assert!(titles.iter().any(|t| t == "框型"), "{titles:?}");
        assert!(titles.iter().any(|t| t == "边框色"), "{titles:?}");
        assert!(titles.iter().any(|t| t == "删除"), "{titles:?}");

        // 进"框型"子菜单,选 heavy,执行
        let idx = titles.iter().position(|t| t == "框型").unwrap();
        app.view.menu.as_mut().unwrap().sel = idx;
        app.menu_choose();
        assert_eq!(
            app.view.menu.as_ref().unwrap().levels.len(),
            2,
            "该进子菜单"
        );
        let heavy = app
            .view
            .menu
            .as_ref()
            .unwrap()
            .level()
            .iter()
            .position(|i| i.title == "heavy")
            .expect("有 heavy");
        app.view.menu.as_mut().unwrap().sel = heavy;
        app.menu_choose();
        assert!(app.view.menu.is_none(), "执行完该关掉");
        assert_eq!(app.doc.boxes[0].style.name(), "heavy");
        assert_eq!(app.undo.counts().0, 1, "菜单执行也只算一步");

        // 空处右键给"新建框"
        app.view.cursor = (-50, -50);
        app.refresh_pick();
        app.open_menu(1, 1);
        let m = app.view.menu.clone().unwrap();
        assert_eq!(m.level()[0].title, "新建框…");
    }

    /// 撤销/重做:同值设置不占步,新建/换布局都能撤
    #[test]
    fn 撤销与重做() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.jump_to(0);
        let before = app.doc.boxes[0].fill;
        app.cmd_apply("fill #123456");
        assert_eq!(app.doc.boxes[0].fill, Some((0x12, 0x34, 0x56)));
        // 同一个值再来一次不算一步
        app.cmd_apply("fill #123456");
        assert_eq!(app.undo.counts(), (1, 0), "同值设置不该多一步");

        app.cmd_apply("fill #654321");
        assert_eq!(app.undo.counts(), (2, 0));
        app.handle(键(KeyCode::Char('u')));
        assert_eq!(app.doc.boxes[0].fill, Some((0x12, 0x34, 0x56)), "撤销一步");
        assert_eq!(app.undo.counts(), (1, 1));
        app.handle(ctrl(KeyCode::Char('r')));
        assert_eq!(app.doc.boxes[0].fill, Some((0x65, 0x43, 0x21)), "重做回来");

        // 新建框 -> 撤销
        let n0 = app.doc.boxes.len();
        app.cmd_apply("new z");
        assert_eq!(app.doc.boxes.len(), n0 + 1);
        app.cmd_apply("undo");
        assert_eq!(app.doc.boxes.len(), n0, "撤销建框");
        assert!(app.view.note.clone().unwrap_or_default().contains("撤销"));

        // 换布局 -> 撤销后坐标回原样
        let at0: Vec<_> = app.doc.boxes.iter().map(|b| b.at).collect();
        app.cmd_apply("layout force");
        assert_ne!(app.doc.boxes.iter().map(|b| b.at).collect::<Vec<_>>(), at0);
        app.cmd_apply("undo");
        assert_eq!(app.doc.boxes.iter().map(|b| b.at).collect::<Vec<_>>(), at0);

        // 撤销后再改东西,重做栈清空
        app.cmd_apply("undo");
        assert!(app.undo.counts().1 > 0, "撤销之后该有可重做的");
        app.cmd_apply("fill #abcdef");
        assert_eq!(app.undo.counts().1, 0, "新动作清掉重做栈");
        let _ = before;
    }

    /// 命令行/搜索的光标按原样文本算:路径里的 `/` 不是斜体标记
    #[test]
    fn 命令行的光标不被标记语法影响() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.view.cmd = Some("w /examples/".to_string());
        app.view.cmd_pos = 12;
        let (x, _) = app.cursor_cell().expect("该有光标");
        // `:` + 原文的宽度,而不是解析掉 /examples/ 之后的宽度
        assert_eq!(x, render::raw_width(":w /examples/") as u16);
        app.view.cmd = Some("text *a*b*".to_string());
        app.view.cmd_pos = 11;
        let (x2, _) = app.cursor_cell().expect("该有光标");
        assert_eq!(x2, render::raw_width(":text *a*b*") as u16);
    }

    #[test]
    fn 自定义键位生效而默认键不变() {
        use crate::keys::{Action as K, Key, KeyMap};
        let mut opts = zh_opts();
        let mut km = KeyMap::defaults();
        km.bind(Key::parse("h").unwrap(), Some(K::MoveDown));
        opts.keys = km;
        let mut app = App::new(样例文档(), opts).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        let before = app.view.cursor;
        app.handle(键(KeyCode::Char('h')));
        assert_eq!(app.view.cursor.1, before.1 + 1, "h 该改成往下走");
        // 没改的键照旧
        let c = app.view.cursor;
        app.handle(键(KeyCode::Char('l')));
        assert_eq!(app.view.cursor.0, c.0 + 1, "l 还是右移");
    }

    #[test]
    fn 关掉鼠标就不理鼠标事件() {
        let mut opts = zh_opts();
        opts.mouse = false;
        let mut app = App::new(样例文档(), opts).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        let before = app.view.cursor;
        app.handle(鼠标(
            crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            10,
            5,
        ));
        assert_eq!(app.view.cursor, before, "鼠标关掉后左键不该动光标");
    }

    /// 退出只认 :q:q 与 esc 都不退
    #[test]
    fn 只有_q_能退出() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        assert_eq!(app.handle(键(KeyCode::Char('q'))), Flow::Continue);
        let note = app.view.note.clone().unwrap_or_default();
        assert!(note.contains(":q"), "q 该提示用 :q 退出:{note}");
        assert_eq!(app.handle(键(KeyCode::Esc)), Flow::Continue);
        assert_eq!(
            app.handle(ctrl(KeyCode::Char('c'))),
            Flow::Continue,
            "ctrl-c 也不再退出"
        );
        // :q 立刻退
        app.view.cmd = Some(String::new());
        for c in ":q".chars().skip(1) {
            app.handle(键(KeyCode::Char(c)));
        }
        assert_eq!(app.handle(键(KeyCode::Enter)), Flow::Quit);
        assert!(app.quit);
    }

    #[test]
    /// 光标一格一步,数字前缀是格数;pick 跟着格子走
    fn 光标按格子移动() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        app.jump_to(0);
        let start = app.view.cursor;
        app.handle(键(KeyCode::Char('l')));
        assert_eq!(app.view.cursor, (start.0 + 1, start.1), "向右一格");
        app.handle(键(KeyCode::Char('j')));
        assert_eq!(app.view.cursor, (start.0 + 1, start.1 + 1), "向下一格");
        app.view.cursor = start;
        app.refresh_pick();
        for c in ['5', 'l'] {
            app.handle(键(KeyCode::Char(c)));
        }
        assert_eq!(app.view.cursor, (start.0 + 5, start.1), "5l 走五格");
        // 10j:0 属于计数前缀, 不能触发"回到 1:1"
        app.view.cursor = start;
        app.view.zoom = 2.0;
        app.refresh_pick();
        for c in ['1', '0', 'j'] {
            app.handle(键(KeyCode::Char(c)));
        }
        assert_eq!(app.view.cursor, (start.0, start.1 + 10), "10j 走十格");
        assert_eq!(app.view.zoom, 2.0, "计数里的 0 不能改缩放");
        // 没有前缀时 0 仍然是回到 1:1
        app.view.cursor = start;
        app.handle(键(KeyCode::Char('0')));
        assert_eq!(app.view.zoom, 1.0, "单独按 0 回到 1:1");
        assert_eq!(app.view.cursor, start, "单独按 0 不移动光标");
        app.view.cursor = (-50, -50);
        app.refresh_pick();
        assert_eq!(app.view.pick, render::Pick::None);
    }

    /// 鼠标点哪儿,光标就落到哪一格
    #[test]
    fn 鼠标点击落格() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        app.handle(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 40,
            row: 10,
            modifiers: KeyModifiers::NONE,
        }));
        let p = render::panels(&app.doc, app.area, &app.view);
        assert_eq!(app.view.cursor, p.proj.canvas(40, 10));
    }

    #[test]
    fn 旋转动词改坐标并记住方向() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        let before: Vec<_> = app.doc.boxes.iter().map(|b| b.at).collect();
        app.cmd_apply("rotate 90");
        assert_eq!(app.view.angle, 90);
        let after: Vec<_> = app.doc.boxes.iter().map(|b| b.at).collect();
        assert_ne!(before, after, "坐标该转过去");
        app.cmd_apply("rotate 0");
        assert_eq!(app.view.angle, 0);
        assert_eq!(
            app.doc.boxes.iter().map(|b| b.at).collect::<Vec<_>>(),
            before,
            "转回来该回到原坐标"
        );
        // auto 只改方向,不动坐标
        let now: Vec<_> = app.doc.boxes.iter().map(|b| b.at).collect();
        app.cmd_apply("rotate auto");
        assert_eq!(app.view.orientation.name(), "auto");
        assert_eq!(app.doc.boxes.iter().map(|b| b.at).collect::<Vec<_>>(), now);
        // 不带参数就是再转 90 度
        app.cmd_apply("rotate");
        assert_eq!(app.view.angle, 90);
    }

    /// 选中一条边时 `:text` 写的是这条边的注释框(空串连框删掉);进撤销栈
    /// 新建的节点用默认样式:只有文字,宽随文字
    #[test]
    fn 新建节点默认是只有文字() {
        let mut app = App::new(Document::scratch(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.cmd_apply("new 你好");
        let i = app.doc.boxes.len() - 1;
        assert_eq!(app.doc.boxes[i].style, BoxStyle::None);
        assert_eq!(app.doc.boxes[i].size.1, 1, "一行高");
        assert_eq!(
            app.doc.boxes[i].size.0,
            crate::text::plain_width("你好") as u16,
            "宽随文字"
        );
        assert_eq!(app.doc.texts[i].at, app.doc.boxes[i].at);
    }

    /// Ctrl+hjkl:框挪一格,光标跟着走;光标挪出画面边距时视图追过去
    #[test]
    fn ctrl_方向键光标跟框走并推动视图() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 60, 20);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);

        let before_box = app.doc.boxes[0].at;
        let before_cursor = app.view.cursor;
        let press = |app: &mut App, code| {
            app.handle(Event::Key(KeyEvent::new(
                KeyCode::Char(code),
                KeyModifiers::CONTROL,
            )));
        };
        // 光标先落到框上,这样 pick 指向它
        let target = app.doc.text(0).map(|t| t.at).unwrap_or(before_box);
        app.view.cursor = target;
        app.refresh_pick();

        press(&mut app, 'l');
        assert_eq!(
            app.doc.boxes[0].at,
            (before_box.0 + 1, before_box.1),
            "框挪一格"
        );
        assert_eq!(app.view.cursor, (target.0 + 1, target.1), "光标跟着走");
        assert_eq!(app.view.pick.box_at(), Some(0), "挪完光标还压在同一个框上");
        assert_ne!(app.view.cursor, before_cursor);

        // 把视图挪到"框就在右边距上",再往右挪:视图该开始跟过去
        app.view.cursor = app.doc.text(0).map(|t| t.at).unwrap_or(before_box);
        app.refresh_pick();
        let p = render::panels(&app.doc, app.area, &app.view);
        let x1 = (p.map.x + p.map.width) as i32 - 1 - 2; // follow_cursor 的两格边距
        let scale = p.proj.scale;
        let mid = p.map.x as i32 + p.map.width as i32 / 2;
        app.view.center = Some((
            app.view.cursor.0 as f64 - (x1 - mid) as f64 * scale,
            app.view.center.map(|c| c.1).unwrap_or(0.0),
        ));
        app.view.target = None;
        app.view.pan = None;
        let center = app.view.center;
        press(&mut app, 'l');
        assert!(
            app.view.target.is_some() || app.view.pan.is_some() || app.view.center != center,
            "光标挪到边上时视图要跟过去"
        );
    }

    /// 键盘挪光标:贴边时视图只挪一小段,挪完还能再走几格才需要再挪
    #[test]
    fn 光标贴边只挪一小段视图() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        app.view.zoom = 1.0;
        let p = render::panels(&app.doc, app.area, &app.view);
        let y0 = p.map.y as i32 + 2; // follow_cursor 的两格边距
        let anchor = p.map.y as f64 + p.map.height as f64 / 2.0;
        // 光标先停在上边距那一行
        let c = (10, 0);
        app.view.cursor = c;
        app.view.center = Some((c.0 as f64, c.1 as f64 - y0 as f64 + anchor));
        app.view.target = None;
        app.view.pan = None;
        app.refresh_pick();
        // 贴到边距之后像 nvim:每按一下画面当场滚一格(不缓动),
        // 光标在屏幕上钉在边距那一行 —— 不然会先动再弹回来
        let mut prev = app.view.center.expect("有中心");
        for step in 0..4 {
            app.handle(键(KeyCode::Char('k')));
            let now = app.view.center.expect("有中心");
            let dy = now.1 - prev.1;
            assert!((dy + 1.0).abs() < 1e-6, "第 {step} 下该只滚一格,实际 {dy}");
            assert_eq!(app.view.target, None, "贴边时不做缓动");
            let p = render::panels(&app.doc, app.area, &app.view);
            let (_, cy) = p
                .proj
                .cell((app.view.cursor.0 as f64, app.view.cursor.1 as f64));
            assert_eq!(cy, y0, "光标该钉在边距那一行(第 {step} 下)");
            prev = now;
        }
        // 逻辑位置一直在走
        assert_eq!(app.view.cursor.1, c.1 - 4);
    }

    /// 计数前缀也管 Ctrl-hjkl:10 + Ctrl-l = 挪 10 格,一次撤销
    #[test]
    fn ctrl_方向键吃计数前缀() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);

        let target = app.doc.text(0).map(|t| t.at).unwrap_or(app.doc.boxes[0].at);
        app.view.cursor = target;
        app.refresh_pick();
        let before = app.doc.boxes[0].at;

        for c in ['1', '0'] {
            app.handle(Event::Key(KeyEvent::new(
                KeyCode::Char(c),
                KeyModifiers::NONE,
            )));
        }
        app.handle(Event::Key(KeyEvent::new(
            KeyCode::Char('l'),
            KeyModifiers::CONTROL,
        )));
        assert_eq!(
            app.doc.boxes[0].at,
            (before.0 + 10, before.1),
            "10 应该挪十格"
        );
        assert_eq!(app.view.cursor, (target.0 + 10, target.1), "光标跟着走十格");

        // 一次 Ctrl-l 带计数只算一步撤销
        app.undo_step();
        assert_eq!(app.doc.boxes[0].at, before, "一步就撤回来");

        // 计数用过就清掉:再按一次 Ctrl-l 只挪一格
        app.view.cursor = target;
        app.refresh_pick();
        app.handle(Event::Key(KeyEvent::new(
            KeyCode::Char('l'),
            KeyModifiers::CONTROL,
        )));
        assert_eq!(
            app.doc.boxes[0].at,
            (before.0 + 1, before.1),
            "计数不该留着"
        );
    }

    /// E 只在光标那个节点与邻居之间的边里轮(按文档顺序绕回)
    #[test]
    fn e_只在当前节点的相连边里轮() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = 连边应用(样例文档());
        // 样例里 b1 连着两条边(b0 -> b1、b2 -- b1)
        let c = app.doc.box_idx("b1").expect("样例有 b1");
        app.view.cursor = app.doc.text(c).map(|t| t.at).unwrap_or((0, 0));
        app.view.pick = render::Pick::None;
        let incident: Vec<usize> = app.doc.adj(c).to_vec();
        assert_eq!(incident.len(), 2, "样例该给 b1 连两条边");

        let key = || KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL);
        app.handle(Event::Key(key()));
        assert_eq!(app.view.pick.edge(), Some(incident[0]), "第一条");
        app.handle(Event::Key(key()));
        assert_eq!(app.view.pick.edge(), Some(incident[1]), "下一条");
        app.handle(Event::Key(key()));
        assert_eq!(app.view.pick.edge(), Some(incident[0]), "到头绕回去");
    }

    /// E:只亮光标节点的邻居与相连的边;再按 E 或 Esc 退出
    #[test]
    fn e_开关邻居高亮() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);

        let c = app.doc.box_idx("b1").expect("样例有 b1");
        app.view.cursor = app.doc.text(c).map(|t| t.at).unwrap_or((0, 0));
        app.refresh_pick();

        app.handle(键(KeyCode::Char('E')));
        assert_eq!(app.view.ego, Some(c), "开着的是光标那个节点");
        let note = app.view.note.clone().unwrap_or_default();
        assert!(note.contains("2 个邻居"), "回显要报邻居数:{note}");

        // 光标记到另一个节点上:高亮跟着换过去(不用再按 E)
        let other = app.doc.box_idx("b0").expect("样例有 b0");
        app.view.cursor = app.doc.text(other).map(|t| t.at).unwrap_or((0, 0));
        app.refresh_pick();
        assert_eq!(app.view.ego, Some(other), "点到/挪到别的节点就换过去亮它");

        // 再按一次 E:关掉整个邻居高亮
        app.handle(键(KeyCode::Char('E')));
        assert_eq!(app.view.ego, None, "E 再按一次该关掉");

        // 开起来之后 Esc 也能退
        app.handle(键(KeyCode::Char('E')));
        assert!(app.view.ego.is_some());
        app.handle(键(KeyCode::Esc));
        assert_eq!(app.view.ego, None, "Esc 该退掉");

        // 光标不在节点上:只提示,不开
        app.view.cursor = (-99, -99);
        app.refresh_pick();
        app.handle(键(KeyCode::Char('E')));
        assert_eq!(app.view.ego, None);
        assert!(app
            .view
            .note
            .clone()
            .unwrap_or_default()
            .contains("没有节点"));
    }

    #[test]
    fn 输入行交给外部编辑器() {
        use crate::external::Outcome;
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.open_prompt(Prompt::NewBox, "旧".to_string());
        app.handle(ctrl(KeyCode::Char('g')));
        assert!(app.edit_pending(), "Ctrl+G 该请求外部编辑器");
        assert_eq!(app.edit_initial().as_deref(), Some("旧"));
        let boxes = app.doc.boxes.len();
        app.apply_edit_result(Outcome::Changed("第一行\n第二行".to_string()));
        assert!(!app.edit_pending(), "应用之后没有待办");
        assert!(app.view.prompt.is_none(), "直接提交就不要再留输入框");
        assert_eq!(app.doc.boxes.len(), boxes + 1, "编辑器内容直接建了框");

        // 连边目标字段是单行:多行拒绝,输入框留着
        app.open_prompt(Prompt::Link(0), "网关".to_string());
        app.handle(ctrl(KeyCode::Char('g')));
        assert_eq!(app.edit_initial().as_deref(), Some("网关"));
        app.apply_edit_result(Outcome::Changed("甲\n乙".to_string()));
        assert_eq!(
            app.view.prompt.as_ref().map(|(_, t)| t.as_str()),
            Some("网关"),
            "单行字段不收换行"
        );
        assert!(app.view.note.clone().unwrap_or_default().contains("一行"));

        // 失败:原样 + 提示
        app.handle(ctrl(KeyCode::Char('g')));
        assert_eq!(app.edit_initial().as_deref(), Some("网关"));
        app.apply_edit_result(Outcome::Failed(3));
        assert_eq!(
            app.view.prompt.as_ref().map(|(_, t)| t.as_str()),
            Some("网关")
        );
    }

    /// Ctrl+G(普通模式)直接用编辑器改框里的字,退出即生效,一步撤销
    #[test]
    fn 外部编辑器直接改框文字() {
        use crate::external::Outcome;
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        app.jump_to(0);
        app.handle(ctrl(KeyCode::Char('g')));
        assert!(app.edit_pending(), "该请求外部编辑器");
        assert_eq!(
            app.edit_initial().as_deref(),
            Some("甲"),
            "初始内容是光标框的原文"
        );
        let undo = app.undo.counts().0;
        app.apply_edit_result(Outcome::Changed("新文字".to_string()));
        assert!(app.view.prompt.is_none(), "不经过底行输入框");
        let id = app.doc.boxes[0].id.clone();
        assert_eq!(
            app.doc.text(0).map(|t| t.text.as_str()),
            Some("新文字"),
            "文字直接写进框"
        );
        assert_eq!(app.undo.counts().0, undo + 1, "整次编辑一步撤销");
        app.undo_step();
        assert_eq!(app.doc.text(0).map(|t| t.text.as_str()), Some("甲"));
        assert!(app.doc.boxes.iter().any(|b| b.id == id), "框还在");

        // 没改动:不动
        app.handle(ctrl(KeyCode::Char('g')));
        app.apply_edit_result(Outcome::Unchanged);
        assert_eq!(app.doc.text(0).map(|t| t.text.as_str()), Some("甲"));
        assert!(app
            .view
            .note
            .clone()
            .unwrap_or_default()
            .contains("没有改动"));

        // 没有框:提示,不请求编辑器
        app.view.cursor = (200, 200);
        app.refresh_pick();
        app.handle(ctrl(KeyCode::Char('g')));
        assert!(!app.edit_pending());
        assert!(app
            .view
            .note
            .clone()
            .unwrap_or_default()
            .contains("没有可编辑的框"));
    }

    /// 一条边的注释文字(没有注释框就是 None)
    fn 边的注释文字(app: &App, e: usize) -> Option<String> {
        let id = app.doc.edges.get(e)?.id.clone();
        let b = app.doc.note_of_edge(&id)?;
        app.doc.text(b).map(|t| t.text.clone())
    }

    #[test]
    fn 选中边时_text写注释() {
        let mut app = 连边应用(样例文档());
        assert!(!app.doc.edges.is_empty(), "样例该有边");
        app.view.pick = render::Pick::Edge(0);
        app.cmd_apply("text 收到 FIN");
        assert_eq!(边的注释文字(&app, 0).as_deref(), Some("收到 FIN"));
        let b = app.doc.note_of_edge(&app.doc.edges[0].id).unwrap();
        assert_eq!(app.doc.boxes[b].style, crate::model::BoxStyle::None);
        app.undo_step();
        assert!(边的注释文字(&app, 0).is_none(), "撤销该把注释框撤掉");

        app.view.pick = render::Pick::Edge(0);
        app.cmd_apply("text 收到 FIN");
        app.view.pick = render::Pick::Edge(0);
        app.cmd_apply("text");
        assert!(边的注释文字(&app, 0).is_none(), "空串 = 连注释框一起删");
    }

    #[test]
    fn 选中边按i建注释框() {
        let mut app = 连边应用(样例文档());
        // 光标先挪到真正的边笔画上(不在框上),i 才是"给这条边建注释"
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        let e = 0usize;
        let cell = app
            .hit
            .edge_cells
            .get(&e)
            .and_then(|c| {
                c.iter()
                    .copied()
                    .find(|c| app.doc.box_at_cell(*c).is_none())
            })
            .expect("样例该有边笔画");
        app.view.cursor = cell;
        app.refresh_pick();
        app.view.pick = render::Pick::Edge(0);
        app.handle(键(KeyCode::Char('i')));
        assert!(
            matches!(app.view.prompt, Some((Prompt::Text(_), _))),
            "该进输入"
        );
        app.handle(键(KeyCode::Esc));
        assert!(边的注释文字(&app, 0).is_none(), "Esc 该把空注释框撤掉");

        // 再来一次,写了字就留下
        app.view.pick = render::Pick::Edge(0);
        app.handle(键(KeyCode::Char('i')));
        for c in ['收', '到', 'A', 'C', 'K'] {
            app.handle(键(KeyCode::Char(c)));
        }
        app.handle(键(KeyCode::Enter));
        assert_eq!(边的注释文字(&app, 0).as_deref(), Some("收到ACK"));
    }

    #[test]
    fn 删边连注释框一起走() {
        let mut app = 连边应用(样例文档());
        app.view.pick = render::Pick::Edge(0);
        app.cmd_apply("text 收到 FIN");
        let n0 = app.doc.boxes.len();
        let id = app.doc.edges[0].id.clone();
        app.doc.remove_edge(&id).unwrap();
        assert_eq!(app.doc.boxes.len(), n0 - 1, "注释框该一起删");
        assert!(app.doc.note_of_edge(&id).is_none());
    }

    #[test]
    fn 真实光标位置在其他面板出现时也对() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        // 真实光标:在光标框的文字起点
        let p = render::panels(&app.doc, app.area, &app.view);
        let want = p
            .proj
            .cell((app.view.cursor.0 as f64, app.view.cursor.1 as f64));
        assert_eq!(app.cursor_cell(), Some((want.0 as u16, want.1 as u16)));
        // 命令行打开时落到输入行
        app.view.cmd = Some("notes on".to_string());
        app.view.cmd_pos = 8;
        let (x, y) = app.cursor_cell().expect("输入行该有光标");
        assert_eq!(y, 23);
        assert_eq!(x, render::width(":notes on") as u16);
        app.view.cmd = None;
        // 帮助浮层打开时藏起来
        app.view.help = true;
        assert_eq!(app.cursor_cell(), None);
    }

    #[test]
    fn 空白文档里建框连边再删掉() {
        let mut app = App::new(Document::scratch(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        assert_eq!(app.doc.boxes.len(), 1, "空白文档先给一个框");
        assert_eq!(
            app.doc.text(0).map(|t| t.text.clone()).as_deref(),
            Some("a")
        );

        app.cmd_apply("new b");
        assert_eq!(app.doc.boxes.len(), 2);
        assert_eq!(app.view.cursor, app.doc.text(1).unwrap().at, "光标跟到新框");
        assert_eq!(
            app.doc.text(1).map(|t| t.text.clone()).as_deref(),
            Some("b")
        );

        app.cmd_apply("link a");
        assert_eq!(app.doc.edges.len(), 1);
        assert_eq!(app.doc.edges[0].from, "b2");
        assert_eq!(app.doc.edges[0].to, "b1");
        assert_eq!(app.doc.edges[0].dir, Dir::Forward);
        // 同一条边不能连两次
        app.cmd_apply("link a");
        assert_eq!(app.doc.edges.len(), 1);
        let note = app.view.note.clone().unwrap_or_default();
        assert!(note.contains("连不上"), "{note}");

        // 删框:它的文字与边一起走
        app.cmd_apply("del");
        assert_eq!(app.doc.boxes.len(), 1);
        assert!(app.doc.edges.is_empty());
        assert_eq!(app.doc.texts.len(), 1);
        // 光标留在被删掉的位置,先挪回最后一个框
        app.jump_to(0);
        app.cmd_apply("del");
        assert_eq!(app.doc.boxes.len(), 1);
        let note = app.view.note.clone().unwrap_or_default();
        assert!(note.contains("最后一个框"), "{note}");
    }

    #[test]
    fn 新建框默认文字用框id() {
        let mut app = App::new(Document::scratch(), zh_opts()).expect("开局");
        app.cmd_apply("new");
        let last = app.doc.boxes.len() - 1;
        let id = app.doc.boxes[last].id.clone();
        assert_eq!(app.doc.text(last).map(|t| t.text.clone()), Some(id));
    }

    #[test]
    fn 图层循环与tolayer建层() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        assert_eq!(app.view.layer, None);
        app.handle(键(KeyCode::Char('L')));
        assert_eq!(app.view.layer.as_deref(), Some(""));
        app.handle(键(KeyCode::Char('L')));
        assert_eq!(app.view.layer, None, "循环回全部");
        app.cmd_apply("layer all");
        app.jump_to(0);
        app.view.sel = None;
        app.cmd_apply("tolayer 新层");
        assert_eq!(app.doc.boxes[0].layer, "新层");
        assert_eq!(app.doc.text(0).unwrap().layer, "新层", "文字跟着框走层");
        assert!(app.doc.layer_names().iter().any(|l| l == "新层"));
    }

    #[test]
    fn 编辑动词改属性() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.view.zoom = 1.0;
        app.jump_to(0);
        app.cmd_apply("box double");
        assert_eq!(app.doc.boxes[0].style, BoxStyle::Double);
        app.cmd_apply("nodecolor #ff0000");
        assert_eq!(app.doc.boxes[0].color, Some((0xff, 0, 0)));
        app.cmd_apply("nodecolor auto");
        assert_eq!(app.doc.boxes[0].color, None);
        app.cmd_apply("text **粗** 体 /斜/  空格");
        assert_eq!(app.doc.text(0).unwrap().text, "**粗** 体 /斜/  空格");
        // 改文字后框宽重算
        assert!(app.doc.boxes[0].size.0 >= 4);
        app.view.pick = render::Pick::Edge(1);
        app.cmd_apply("style dashed");
        assert_eq!(app.doc.edges[1].style, EdgeStyle::Dashed);
        app.cmd_apply("edgecolor #00ff00");
        assert_eq!(app.doc.edges[1].color, Some((0, 0xff, 0)));
        app.cmd_apply("via 1,2 3,4");
        assert_eq!(app.doc.edges[1].via, vec![(1, 2), (3, 4)]);
        app.cmd_apply("via off");
        assert!(app.doc.edges[1].via.is_empty());
    }

    /// fill 改选中框的背景;none 清成 null;point 框也允许设(渲染自己忽略)
    #[test]
    fn 填充动词改背景并支持清空() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.view.zoom = 1.0;
        app.jump_to(0);
        app.cmd_apply("fill #102030");
        assert_eq!(app.doc.boxes[0].fill, Some((0x10, 0x20, 0x30)));
        assert!(app.view.note.as_deref().unwrap_or("").contains("填充"));
        app.cmd_apply("fill none");
        assert_eq!(app.doc.boxes[0].fill, None, "none 要清成 null");
        // 点框也允许设
        app.doc.boxes[0].style = BoxStyle::Point;
        app.cmd_apply("fill #abcdef");
        assert_eq!(app.doc.boxes[0].fill, Some((0xab, 0xcd, 0xef)));
    }

    #[test]
    fn 填充参数坏了有回显且不改图() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.view.zoom = 1.0;
        app.cmd_apply("fill blue");
        assert_eq!(app.doc.boxes[0].fill, None, "坏参数不该改图");
        assert!(app.view.note.as_deref().unwrap_or("").contains("fill"));
        app.cmd_apply("fill");
        let note = app.view.note.clone().unwrap_or_default();
        assert!(note.contains("fill"), "缺参数要有回显:{note}");
    }

    /// textcolor 改的是框内文字颜色;auto 清成 null(跟着框)
    #[test]
    fn 文字颜色动词改颜色并支持auto() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.view.zoom = 1.0;
        app.jump_to(0);
        app.cmd_apply("textcolor #112233");
        assert_eq!(app.doc.texts[0].color, Some((0x11, 0x22, 0x33)));
        assert!(app.view.note.as_deref().unwrap_or("").contains("文字颜色"));
        app.cmd_apply("textcolor auto");
        assert_eq!(app.doc.texts[0].color, None);
    }

    #[test]
    fn 文字颜色参数坏了有回显() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.view.zoom = 1.0;
        app.cmd_apply("textcolor nope");
        assert_eq!(app.doc.texts[0].color, None);
        assert!(app.view.note.as_deref().unwrap_or("").contains("textcolor"));
        app.cmd_apply("textcolor");
        let note = app.view.note.clone().unwrap_or_default();
        assert!(note.contains("textcolor"), "缺参数要有回显:{note}");
    }

    /// textalign 改框内文字对齐
    #[test]
    fn 文字对齐动词改对齐并拒绝坏参数() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.view.zoom = 1.0;
        app.jump_to(0);
        app.cmd_apply("textalign center");
        assert_eq!(app.doc.texts[0].align, Align::Center);
        assert!(app.view.note.as_deref().unwrap_or("").contains("文字对齐"));
        app.cmd_apply("textalign right");
        assert_eq!(app.doc.texts[0].align, Align::Right);
        app.cmd_apply("textalign middle");
        assert_eq!(app.doc.texts[0].align, Align::Right, "坏参数不该改");
        assert!(app.view.note.as_deref().unwrap_or("").contains("textalign"));
        app.cmd_apply("textalign");
        assert!(app.view.note.as_deref().unwrap_or("").contains("textalign"));
    }

    /// dir 改选中边的四态方向
    #[test]
    fn 边方向动词改方向并拒绝坏参数() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.view.zoom = 1.0;
        app.view.pick = render::Pick::Edge(1);
        app.cmd_apply("dir both");
        assert_eq!(app.doc.edges[1].dir, Dir::Both);
        assert!(app.view.note.as_deref().unwrap_or("").contains("边方向"));
        app.cmd_apply("dir backward");
        assert_eq!(app.doc.edges[1].dir, Dir::Both, "坏参数不该改");
        assert!(app.view.note.as_deref().unwrap_or("").contains("dir"));
        app.cmd_apply("dir");
        let note = app.view.note.clone().unwrap_or_default();
        assert!(
            note.contains("dir") && note.contains("缺参数"),
            "缺参数要有回显:{note}"
        );
    }

    /// head 改选中边的箭头字形;undo 能撤回
    #[test]
    fn 边箭头动词改字形并拒绝坏参数() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.view.zoom = 1.0;
        app.view.pick = render::Pick::Edge(1);
        app.cmd_apply("head triangles");
        assert_eq!(
            app.doc.edges[1].head,
            Some(crate::model::ArrowStyle::Triangles)
        );
        assert!(app.view.note.as_deref().unwrap_or("").contains("箭头"));
        app.cmd_apply("head auto");
        assert_eq!(app.doc.edges[1].head, None, "auto 就是跟全局走");
        app.cmd_apply("head square");
        assert_eq!(app.doc.edges[1].head, None, "坏参数不该改");
        assert!(app.view.note.as_deref().unwrap_or("").contains("head"));
        app.cmd_apply("head");
        let note = app.view.note.clone().unwrap_or_default();
        assert!(
            note.contains("head") && note.contains("缺参数"),
            "缺参数要有回显:{note}"
        );
        app.cmd_apply("head triangles");
        let text = app.doc.save();
        assert!(
            text.contains("\"head\": \"triangles\""),
            "存出去该带着:{text}"
        );
        // 一次改动一步 undo
        app.cmd_apply("head arrows");
        assert_eq!(
            app.doc.edges[1].head,
            Some(crate::model::ArrowStyle::Arrows)
        );
        app.undo_step();
        assert_eq!(
            app.doc.edges[1].head,
            Some(crate::model::ArrowStyle::Triangles),
            "undo 只退一步"
        );
        app.undo_step();
        assert_eq!(app.doc.edges[1].head, None, "再退一步回到 auto");
    }

    /// 边的右键菜单里有箭头字形,比当前值打勾
    #[test]
    fn 边菜单有箭头字形() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.view.zoom = 1.0;
        app.doc.edges[1].head = Some(crate::model::ArrowStyle::Triangles);
        app.view.pick = render::Pick::Edge(1);
        let items = app.menu_for();
        let i = items
            .iter()
            .position(|it| it.title == "箭头")
            .expect("该有箭头一项");
        let MenuAction::Sub(sub) = &items[i].action else {
            panic!("箭头该是子菜单");
        };
        let titles: Vec<&str> = sub.iter().map(|it| it.title.as_str()).collect();
        assert_eq!(titles, ["auto", "ascii", "arrows", "triangles"]);
        assert!(
            sub.iter()
                .find(|it| it.title == "triangles")
                .map(|it| it.checked)
                .unwrap_or(false),
            "当前值该打勾"
        );
        assert!(!sub
            .iter()
            .find(|it| it.title == "auto")
            .map(|it| it.checked)
            .unwrap_or(true));
    }

    /// 视图挪开之后 E 仍然能选中光标下的边(命中表按屏幕格,光标是画布格)
    #[test]
    fn 视图挪过也能选中边() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.view.zoom = 1.0;
        let (cx, cy) = app.view.center.unwrap_or((0.0, 0.0));
        app.view.center = Some((cx + 13.0, cy + 7.0));
        let p = render::panels(&app.doc, app.area, &app.view);
        let mut buf = ratatui::buffer::Buffer::empty(app.area);
        app.draw(&mut buf, app.area);
        // 找一条边占着的一格(命中表里存的是屏幕格),把光标放到它对应的画布格
        let Some(screen) = app.hit.edge_cells.values().find_map(|c| c.first().copied()) else {
            panic!("样例文档该有边");
        };
        let canvas = p.proj.canvas(screen.0, screen.1);
        assert_ne!(screen, canvas, "视图挪开后屏幕格和画布格不该重合");
        app.view.cursor = canvas;
        app.cycle_edge();
        assert!(
            app.view.pick.edge().is_some(),
            "该选中一条边:{:?}",
            app.view.note
        );
    }

    /// corner 改选中边的拐角
    #[test]
    fn 边拐角动词改拐角并拒绝坏参数() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.view.zoom = 1.0;
        app.view.pick = render::Pick::Edge(1);
        app.cmd_apply("corner sharp");
        assert_eq!(app.doc.edges[1].corner, Corner::Sharp);
        assert!(app.view.note.as_deref().unwrap_or("").contains("边拐角"));
        app.cmd_apply("corner round");
        assert_eq!(app.doc.edges[1].corner, Corner::Sharp, "坏参数不该改");
        assert!(app.view.note.as_deref().unwrap_or("").contains("corner"));
        app.cmd_apply("corner");
        let note = app.view.note.clone().unwrap_or_default();
        assert!(
            note.contains("corner") && note.contains("缺参数"),
            "缺参数要有回显:{note}"
        );
    }

    /// dir / corner 没选边时要说一句
    #[test]
    fn 需要选边的动词没选边时报一句() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.view.zoom = 1.0;
        app.view.pick = render::Pick::None;
        for line in ["dir both", "dir", "corner sharp", "corner"] {
            app.view.note = None;
            app.cmd_apply(line);
            let note = app.view.note.clone().unwrap_or_default();
            assert!(
                note.contains("没有选中边") || note.contains("no edge selected"),
                "{line} 该提示没选边:{note}"
            );
        }
    }

    /// 新动词同样受 1:1 门槛约束:缩放不在一比一时先切回再执行
    #[test]
    fn 新编辑动词缩放不是一比一时先切回再执行() {
        let mut app = App::new(样例文档(), zh_opts()).expect("开局");
        app.area = Rect::new(0, 0, 80, 24);
        app.view.zoom = 2.0;
        app.jump_to(0);
        app.cmd_apply("fill #102030");
        assert_eq!(app.view.zoom, 1.0, "修改前该切回 1:1");
        assert_eq!(app.doc.boxes[0].fill, Some((0x10, 0x20, 0x30)));
        app.view.zoom = 2.0;
        app.view.pick = render::Pick::Edge(1);
        app.cmd_apply("corner sharp");
        assert_eq!(app.view.zoom, 1.0, "修改前该切回 1:1");
        assert_eq!(app.doc.edges[1].corner, Corner::Sharp);
    }
}
