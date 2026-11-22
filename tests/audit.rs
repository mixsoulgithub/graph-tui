//! Source audit: the core must not mention any vendor-specific product name.
//!
//! This is a product constraint, not an implementation detail: custom URI
//! schemes come from the `[links]` config, so the core only knows "some scheme",
//! never a particular vendor's name.

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
fn core_has_no_vendor_specific_scheme() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    walk(&root, &mut files);
    assert!(!files.is_empty(), "该找到源码:{}", root.display());
    for f in files {
        let text = std::fs::read_to_string(&f).expect("读源码");
        for (i, line) in text.lines().enumerate() {
            // Only product names; ignore "omp" inside English words
            // (`complete`, `prompt` and the like).
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
