//! graph-tui:通用图渲染引擎.
//!
//! 输入是一张图(节点 + 边 + 边分组),输出是终端里的字符格画面:
//! 边走正交折线、拐角用圆角制表符、`- | / \` 四种笔画的连接处自动合成字形、
//! 两条边交叉时一条从另一条下面穿过(断一格)、一格只留一种颜色。
//!
//! 分层:
//!   document native 文档模型(框/文字/边分开,坐标是单元格)
//!   model    .dot / .canvas 的中间结构(导入导出用)
//!   dot      .dot 子集解析;dot_write 把它写回去(编辑后保存)
//!   scene    画布坐标(同一张图可以有几套坐标)
//!   layout   布局算法:geo / relax / force / layered
//!   canvas   光栅化:把坐标画成方向位,再合成字形
//!   render   投影、缩放平移、站点与标签、图例
//!   text      标签里的内联样式(**粗** / /斜/)
//!   svg       一帧画面 -> 自包含 SVG;json/canvas_io  JSON Canvas 导入导出
//!   app      交互状态与事件
//!   search   搜索
pub mod app;
pub mod canvas;
pub mod canvas_io;
pub mod convert;
pub mod document;
pub mod document_io;
pub mod dot;
pub mod dot_write;
pub mod dump;
pub mod engine;
pub mod external;
pub mod headless;
pub mod json;
pub mod lang;
pub mod layout;
pub mod model;
pub mod paths;
pub mod render;
pub mod scene;
pub mod search;
pub mod svg;
pub mod text;

pub use lang::Lang;
pub use layout::Layout;
pub use model::{Builder, Edge, Graph, Group, Node};
