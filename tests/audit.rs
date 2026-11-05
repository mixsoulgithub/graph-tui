//! 源码审计:核心不许出现某一家产品的专用名词.
//!
//! 这是产品约束,不是实现细节:自定义 URI scheme 一律走 `[links]` 配置,
//! 核心只认识 "某个 scheme",不认识具体那一家的名字.

use std::path::Path;

fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().and_then(|x| x.to_str()) == Some("rs") {
            out.push(p);
        }
    }
}

#[test]
fn 核心没有厂商专用scheme() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    walk(&root, &mut files);
    assert!(!files.is_empty(), "该找到源码:{}", root.display());
    for f in files {
        let text = std::fs::read_to_string(&f).expect("读源码");
        for (i, line) in text.lines().enumerate() {
            // 只看产品名,不看 English 单词里的 omp(`complete` / `prompt` 之类)
            let bad = line.contains("Omp") || line.contains("omp://") || line.contains("OMP://");
            assert!(
                !bad,
                "{}:{}: 核心不该出现厂商专用 scheme:{line}",
                f.display(),
                i + 1
            );
        }
    }
}
