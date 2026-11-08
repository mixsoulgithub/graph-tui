//! `graph-tui tutor`:内嵌教程子命令的错误路径与资源打包.

use std::process::Command;

use graph_tui::lang::Lang;
use graph_tui::tutor;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_graph-tui")
}

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(bin()).args(args).output().expect("run");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// tutor 是交互入口:--dump / --export 要说清楚,不能猜
#[test]
fn tutor_拒绝导出和_导出开关() {
    let cases: [&[&str]; 2] = [&["--dump"], &["--export", "/tmp/gtui-tutor.svg"]];
    for extra in cases {
        let (code, out, err) = run(&[&["tutor"], extra].concat());
        assert_ne!(code, 0, "该报错:{out}{err}");
        assert!(
            err.contains("--dump / --export do not apply"),
            "错误要说清原因:{err}"
        );
    }
}

/// tutor 不收文件参数(文件照常用普通参数打开)
#[test]
fn tutor_不收文件参数() {
    let (code, _, err) = run(&["tutor", "examples/org.gtui"]);
    assert_ne!(code, 0);
    assert!(err.contains("takes no file"), "错误要说清:{err}");
}

/// 两份教程都是能解析的正常文档,而且自包含(资源随二进制打包)
#[test]
fn 两份教程都能直接解析() {
    for lang in [Lang::Zh, Lang::En] {
        let d = tutor::document(lang).expect("教程要能解析");
        assert!(d.boxes.len() >= 10, "{lang:?} 的教程太短");
        assert!(!d.edges.is_empty(), "{lang:?} 的教程要有一条边");
    }
    assert!(!tutor::source(Lang::Zh).is_empty());
    assert!(!tutor::source(Lang::En).is_empty());
}
