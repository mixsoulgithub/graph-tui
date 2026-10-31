//! 别名归一化:拼音/首字母/大小写这些搜索用的字符串处理.
//! 真正的匹配在 document::search 里(它要同时看文字与分组).

/// 别名查询的归一化:只留字母数字,小写,ü 与 v 都当 u
pub fn latin(s: &str) -> String {
    s.chars()
        .filter_map(|c| {
            // ü 与 v 都当 u("nv" 与 "nü" 是一条):要放在字母分支前面,否则 v 会先被当普通字母留下
            if c == 'ü' || c == 'ǖ' || c == 'ǘ' || c == 'ǚ' || c == 'ǜ' || c == 'v' || c == 'V'
            {
                Some('u')
            } else if c.is_ascii_alphanumeric() {
                Some(c.to_ascii_lowercase())
            } else if c.is_alphanumeric() {
                Some(c.to_lowercase().next().unwrap_or(c))
            } else {
                None
            }
        })
        .collect()
}

/// 拼音的首字母缩写:"wu lin guang chang" -> "wlgc"
pub fn initials(pinyin: &str) -> String {
    pinyin
        .split_whitespace()
        .filter_map(|s| s.chars().next())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 别名归一化只留字母数字且小写() {
        assert_eq!(latin("Wu Lin 广场"), "wulin广场");
        assert_eq!(latin("a-b_c!"), "abc");
    }

    #[test]
    fn 拼音里的_v_与_ü_当_u() {
        assert_eq!(latin("lü"), "lu");
        assert_eq!(latin("nv"), "nu");
    }

    #[test]
    fn 首字母缩写按空格切音节() {
        assert_eq!(initials("wu lin guang chang"), "wlgc");
        assert_eq!(initials(""), "");
    }
}
