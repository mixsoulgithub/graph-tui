//! 内置教程:随程序打包的一份可编辑 `.gtui` 文档,像 `vimtutor` 那样边看边做.
//!
//! 教程本身没有任何特权:它是一份普通文档,改坏了重跑命令就恢复(每次都从内嵌文本解析).

use crate::document::Document;

const EN: &str = include_str!("../assets/tutor.en.gtui");

/// 教程文档的原始文本
pub fn source() -> &'static str {
    EN
}

/// 解析成文档:每次调用都是新的一份,调用方随便改
pub fn document() -> Result<Document, String> {
    Document::load(source()).map_err(|e| format!("tutor: {e}"))
}

/// 进来时状态行那一句:说清这是副本、怎么退出、怎么重置
pub fn hint() -> &'static str {
    "tutorial copy; :q to leave; run graph-tui tutor again to reset"
}
