//! `moxsh-proot-distro wizard` 一次性初始化向导的端到端测试（0.2.0，roadmap §5）。
//!
//! 全部走**子进程跑真二进制**：env 只注入子进程，测试进程零污染，
//! 也不需要和 lib 单测抢环境变量（跨进程，无竞态）。
//! 预置 rootfs 走「已安装跳过」路径，不触网下载。

use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_moxsh-proot-distro");

/// 一次性测试沙箱：root（数据根）/ cfg（配置目录）/ home / prefix 四层全隔离，
/// Drop 时整目录销毁。
struct Sandbox {
    dir: PathBuf,
}

impl Sandbox {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("moxsh-wizard-it-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self { dir }
    }

    fn root(&self) -> PathBuf {
        self.dir.join("root")
    }

    fn cfg(&self) -> PathBuf {
        self.dir.join("cfg")
    }

    fn home(&self) -> PathBuf {
        self.dir.join("home")
    }

    fn prefix(&self) -> PathBuf {
        self.dir.join("prefix")
    }

    /// 预置一个「已安装」的 rootfs（跳过下载路径）。
    fn preinstall(&self, id: &str) {
        std::fs::create_dir_all(self.root().join("containers").join(id).join("rootfs")).unwrap();
    }

    /// 以隔离环境跑 wizard 子命令，返回完整输出。
    fn run(&self, args: &[&str]) -> Output {
        Command::new(BIN)
            .args(args)
            .env("MOXSH_ROOT", self.root())
            .env("MOXSH_CONFIG_DIR", self.cfg())
            .env("HOME", self.home())
            .env("PREFIX", self.prefix())
            .output()
            .unwrap()
    }

    fn cfg_toml(&self) -> String {
        std::fs::read_to_string(self.cfg().join("config.toml")).unwrap_or_default()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// 主路径：预置 alpine → 「已安装跳过」+ 数据根落 config.toml + 启动器/控制台落盘。
#[test]
fn wizard_skips_installed_and_persists_root() {
    let sb = Sandbox::new("skip");
    sb.preinstall("alpine");
    let out = sb.run(&["wizard"]);
    assert!(out.status.success(), "wizard 应退出 0：{:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("已安装，跳过"), "输出：{}", stdout);
    assert!(stdout.contains("① 数据根目录"), "输出：{}", stdout);
    assert!(stdout.contains("mx:alpine"), "教学提示缺短链：{}", stdout);
    assert!(stdout.contains("mox"), "教学提示缺控制台：{}", stdout);

    // config.toml 写入了数据根。
    let cfg_text = sb.cfg_toml();
    assert!(
        cfg_text.contains(&format!("root_dir = \"{}\"", sb.root().display())),
        "config.toml 缺 root_dir：{}",
        cfg_text
    );

    // 启动器 + 控制台脚本落盘且 0755。
    for name in ["alpine", "mox"] {
        let p = sb.prefix().join("bin").join(name);
        assert!(p.is_file(), "{} 未落盘", name);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&p).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o755, "{} 要 0755", name);
        }
    }
}

/// --root 覆盖 + 未知发行版跳过（不触网、不阻断向导）。
#[test]
fn wizard_root_override_and_unknown_distro() {
    let sb = Sandbox::new("override");
    let other = sb.dir.join("elsewhere");
    std::fs::create_dir_all(&other).unwrap();
    let out = sb.run(&[
        "wizard",
        "--root",
        other.to_str().unwrap(),
        "--distro",
        "nosuchdistro",
    ]);
    assert!(out.status.success(), "wizard 应退出 0：{:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("初始化向导"), "缺标题：{}", stdout);
    assert!(
        stdout.contains(&other.display().to_string()),
        "覆盖数据根未生效：{}",
        stdout
    );
    assert!(
        stdout.contains("未知发行版"),
        "未知发行版应跳过而非报错：{}",
        stdout
    );

    let cfg_text = sb.cfg_toml();
    assert!(
        cfg_text.contains(&format!("root_dir = \"{}\"", other.display())),
        "config.toml 未写覆盖值：{}",
        cfg_text
    );
}

/// 参数错误：缺值/未知参数 → 用法提示，不写配置。
#[test]
fn wizard_bad_args() {
    let sb = Sandbox::new("badargs");
    for (args, needle) in [
        (vec!["wizard", "--root"], "--root 缺参数"),
        (vec!["wizard", "--distro"], "--distro 缺参数"),
        (vec!["wizard", "--bogus"], "未知参数"),
    ] {
        let out = sb.run(&args);
        assert!(out.status.success(), "{:?} 应退出 0", args);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains(needle),
            "{:?} 输出缺「{}」：{}",
            args,
            needle,
            stdout
        );
    }
    // 参数错误路径不该留下 config.toml。
    assert!(
        !sb.cfg().join("config.toml").exists(),
        "错误路径不该写配置"
    );
}

/// 路径穿越防护：`--distro ../evil` 必须被 id 白名单拦下
/// （install_hint 会拿 id 拼 `bin/<id>` 启动器路径，不拦就写出 bin 目录）。
#[test]
fn wizard_rejects_path_traversal_id() {
    let sb = Sandbox::new("traversal");
    let out = sb.run(&["wizard", "--distro", "../../evil"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("非法发行版 id"), "未拦截穿越 id：{}", stdout);
    // 穿越目标不得被创建（bin/../../evil = 沙箱根/evil）。
    assert!(!sb.dir.join("evil").exists(), "穿越写出了文件");
    // 拦截发生在写配置之前。
    assert!(
        !sb.cfg().join("config.toml").exists(),
        "拦截前不该写配置"
    );
}
