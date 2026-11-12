//! 会话内的一条随机提示:帮人发现键位和功能.
//!
//! 每个会话挑一条(`App::new` 用时间 + 进程号做种子),换行时不再随机;
//! `?` 打开时推进到下一条.内容都是可执行的短句,一行放得下.

/// 一条提示(中英各一份)
pub struct Tip {
    pub zh: &'static str,
    pub en: &'static str,
}

impl Tip {
    /// 按语言取这一条
    pub fn text(&self, lang: crate::lang::Lang) -> &'static str {
        lang.pick(self.zh, self.en)
    }
}

const TIPS: &[Tip] = &[
    Tip {
        zh: "5l 向右走五格:移动、复制、粘贴都吃数字前缀",
        en: "5l moves five cells; counts work for moves, paste and more",
    },
    Tip {
        zh: "Ctrl-e 在光标所在节点的边之间轮换",
        en: "Ctrl-e cycles the edges at the cursor",
    },
    Tip {
        zh: "w / b 沿这一行往右 / 往左跳到下一个框或边",
        en: "w / b jump right / left to the next box or edge",
    },
    Tip {
        zh: "E 隔离:只留这个节点和它的邻居,E 或 Esc 回来",
        en: "E isolates the node and its neighbours; E or Esc restores",
    },
    Tip {
        zh: "空处按 i 新建一个框,直接开始打字",
        en: "i on empty space creates a box and starts typing",
    },
    Tip {
        zh: "v 拖出选区,Enter 确认,然后 y 复制 / d 剪切 / p 粘贴",
        en: "v drags a block, Enter confirms, then y / d / p",
    },
    Tip {
        zh: ": 打开命令行,Tab 补全路径和动词",
        en: ": opens the command line; Tab completes verbs and paths",
    },
    Tip {
        zh: "/ 搜索节点名、别名、分组和区域标题",
        en: "/ searches node labels, aliases, groups and region titles",
    },
    Tip {
        zh: "f 把光标所在的分组铺满,F 铺满整幅图",
        en: "f fits the cursor's group, F fits the whole picture",
    },
    Tip {
        zh: "+ / - 缩放,0 回到 1:1",
        en: "+ / - zoom, 0 goes back to 1:1",
    },
    Tip {
        zh: "空格把光标那个框拉到画面正中",
        en: "space brings the cursor box to the middle",
    },
    Tip {
        zh: "Ctrl-d / Ctrl-u 半屏往下 / 往上跳",
        en: "Ctrl-d / Ctrl-u jump half a screen down / up",
    },
    Tip {
        zh: "Ctrl-o / Ctrl-i 在落脚点之间前后翻",
        en: "Ctrl-o / Ctrl-i walk back and forth through your stops",
    },
    Tip {
        zh: "i 就地改字:Enter 换行,Esc 提交,一步撤销",
        en: "i edits in place: Enter breaks the line, Esc commits",
    },
    Tip {
        zh: "u 撤销,Ctrl-r 重做",
        en: "u undoes, Ctrl-r redoes",
    },
    Tip {
        zh: "s 打开样式菜单,改框型 / 颜色 / 填充 / 对齐",
        en: "s opens the style menu: style, colour, fill, align",
    },
    Tip {
        zh: "c 只改颜色:框是边框色,边是线色",
        en: "c changes colour only: border for a box, ink for an edge",
    },
    Tip {
        zh: "右键拖框可以连边;配置里 mouse_edges = false 可关掉",
        en: "right-drag from a box links it; mouse_edges = false turns that off",
    },
    Tip {
        zh: "Ctrl-t 新 tab,Ctrl-n / Ctrl-p 切换,Ctrl-w 关掉",
        en: "Ctrl-t new tab, Ctrl-n / Ctrl-p switch, Ctrl-w closes",
    },
    Tip {
        zh: "r 在几种布局之间轮换,教程和示例都还能读",
        en: "r cycles the layouts; the picture stays readable",
    },
    Tip {
        zh: ":notes 开关边注释;注释框也能像框一样拖动和改字",
        en: ":notes toggles edge notes; a note is a box you can move and edit",
    },
    Tip {
        zh: "M 把选中的几个框合成一个",
        en: "M merges the selected boxes into one",
    },
    Tip {
        zh: "S 在光标那一行把框切开,后半段另起一个框",
        en: "S splits the box below the cursor line into two",
    },
    Tip {
        zh: "Ctrl-G 把这段文字交给 $EDITOR 去改",
        en: "Ctrl-G hands the text to $EDITOR",
    },
    Tip {
        zh: "? 打开键位速查;g 之后按别的键是组合键",
        en: "? shows the key reference; `g` starts a key sequence",
    },
    Tip {
        zh: "鼠标滚轮缩放,Ctrl+滚轮平移",
        en: "wheel zooms, Ctrl+wheel pans",
    },
    Tip {
        zh: "graph-tui tutor 打开一份可随手改的教程副本",
        en: "graph-tui tutor opens an editable walkthrough",
    },
    Tip {
        zh: "状态行上行是文件与模式,下行是提示和回显",
        en: "the upper status row is the file and mode, the lower is hints",
    },
];

/// 全部提示
pub fn all() -> &'static [Tip] {
    TIPS
}

/// 用种子挑一条(种子由调用方提供,渲染层不取随机数)
pub fn pick(seed: u64) -> usize {
    if TIPS.is_empty() {
        return 0;
    }
    (seed % TIPS.len() as u64) as usize
}

/// 会话种子:纳秒时间 ^ 进程号.不引额外依赖,也不需要可复现
pub fn seed() -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    now ^ (std::process::id() as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    /// 至少 24 条,中英都短到一行放得下(按显示宽度,别按字符数)
    #[test]
    fn 提示够多而且一行放得下() {
        assert!(all().len() >= 24, "只有 {} 条", all().len());
        for tip in all() {
            for text in [tip.zh, tip.en] {
                assert!(!text.is_empty());
                assert!(
                    UnicodeWidthStr::width(text) <= 78,
                    "太长了({} 列):{text}",
                    UnicodeWidthStr::width(text)
                );
            }
        }
    }

    /// 同一个种子总是同一条;种子不同能落到池子里
    #[test]
    fn 同一个种子同一条() {
        for seed in [0u64, 7, 12345, u64::MAX] {
            assert_eq!(pick(seed), pick(seed));
            assert!(pick(seed) < all().len());
        }
        // 不同种子应该能挑到不同的条目(不要求覆盖全部)
        let seen: std::collections::HashSet<usize> = (0..64).map(pick).collect();
        assert!(seen.len() > 1, "种子没散开");
    }
}
