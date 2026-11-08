//! 内置教程:随程序打包的一份可编辑 `.gtui` 文档,像 `vimtutor` 那样边看边做.
//!
//! 教程本身没有任何特权:它是一份普通文档,改坏了重跑命令就恢复(每次都从内嵌文本解析).

use crate::document::Document;
use crate::lang::Lang;

const ZH: &str = include_str!("../assets/tutor.zh.gtui");
const EN: &str = include_str!("../assets/tutor.en.gtui");

/// 教程文档的原始文本(按语言)
pub fn source(lang: Lang) -> &'static str {
    match lang {
        Lang::Zh => ZH,
        Lang::En => EN,
    }
}

/// 解析成文档:每次调用都是新的一份,调用方随便改
pub fn document(lang: Lang) -> Result<Document, String> {
    Document::load(source(lang)).map_err(|e| format!("tutor: {e}"))
}

/// 进来时状态行那一句:说清这是副本、怎么退出、怎么重置
pub fn hint(lang: Lang) -> &'static str {
    lang.pick(
        "教程副本;:q 退出;重跑 graph-tui tutor 恢复",
        "tutorial copy; :q to leave; run graph-tui tutor again to reset",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 两份教程都能解析,箱子和边数量一致(结构同一套,只有文字不同)
    #[test]
    fn 两份教程都能解析且结构一致() {
        let zh = document(Lang::Zh).expect("中文教程要能解析");
        let en = document(Lang::En).expect("英文教程要能解析");
        assert!(zh.boxes.len() >= 10, "教程要够长:{ } 个框", zh.boxes.len());
        assert_eq!(zh.boxes.len(), en.boxes.len(), "两份的框数要一致");
        assert_eq!(zh.edges.len(), en.edges.len(), "两份的边数要一致");
        assert_eq!(zh.texts.len(), en.texts.len(), "两份的文字数要一致");
        // 几何也必须一致,换语言画面不该跳
        for (a, b) in zh.boxes.iter().zip(&en.boxes) {
            assert_eq!(a.at, b.at, "{} 的位置", a.id);
            assert_eq!(a.size, b.size, "{} 的尺寸", a.id);
            assert_eq!(a.style, b.style, "{} 的样式", a.id);
        }
        for (a, b) in zh.edges.iter().zip(&en.edges) {
            assert_eq!(a.from, b.from);
            assert_eq!(a.to, b.to);
            assert_eq!(a.dir, b.dir);
        }
    }

    /// 教程第一屏就要立得住:光标落在欢迎框里,欢迎框和第一章都在它下面
    #[test]
    fn 教程从欢迎框开始() {
        let d = document(Lang::Zh).expect("解析");
        let cur = d.view.cursor_cell(&d);
        let welcome = d.box_idx("c0_welcome").expect("要有欢迎框");
        let (x, y) = (d.boxes[welcome].at.0, d.boxes[welcome].at.1);
        let (w, h) = (
            d.boxes[welcome].size.0 as i32,
            d.boxes[welcome].size.1 as i32,
        );
        assert!(
            cur.0 >= x && cur.0 < x + w && cur.1 >= y && cur.1 < y + h,
            "光标 {cur:?} 该在欢迎框里"
        );
        let first = d.box_idx("c1_move").expect("要有第一章");
        assert!(
            d.boxes[first].at.1 - (y + h) <= 6,
            "第一章别离欢迎框太远,第一屏要看得见"
        );
    }

    /// 练习区该有的东西都在:可改的文本框、空框、两个可连边的框、一条边、一个点节点
    #[test]
    fn 练习区有可练的东西() {
        let d = document(Lang::Zh).expect("解析");
        for id in ["s_edit", "s_empty", "s_a", "s_b", "s_pt"] {
            assert!(d.box_idx(id).is_some(), "练习区少了 {id}");
        }
        assert!(
            d.box_idx("s_empty")
                .is_some_and(|i| d.text(i).is_some_and(|t| t.text.is_empty())),
            "空框要能直接 i 打字"
        );
        let has_edge = d.edges.iter().any(|e| e.from == "s_a" && e.to == "s_b");
        assert!(has_edge, "练习区要有一条边");
        assert!(
            d.box_idx("s_pt")
                .is_some_and(|i| d.boxes[i].style.is_point()),
            "要有一个点节点"
        );
    }
}
