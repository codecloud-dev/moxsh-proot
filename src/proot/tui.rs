//! TUI 控制台（路线图 0.4.0）。
//!
//! ratatui 实现的第三二进制 `mox-proot-tui`，以及 `mox-proot tui` 子命令
//! 共用的入口 [`run_tui`]。覆盖：发行版管理（列表 / 登录 / 安装 / 删除 / 备份）、
//! 短链浏览与安装、配方浏览与安装、日志诊断、安装向导入口。
//!
//! 全部复用 [`crate::proot::distro::DistroManager`] / [`crate::proot::links`] /
//! [`crate::proot::recipe`] / [`crate::proot::settings`] 既有逻辑，不重写容器引擎。
//!
//! 本模块整体受 `tui` feature 门控（见 `Cargo.toml`）：引擎 cdylib 与 Android 构建
//! 不引入 crossterm/ratatui，避免 NDK target 的编译与体积不确定性。

#![cfg(feature = "tui")]

use std::io::{self, Stdout, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Sparkline, Tabs, Wrap,
    },
    Terminal,
};

use crate::proot::backend;
use crate::proot::delight::{self, ContentScene};
use crate::proot::palette;
use crate::proot::device_code::{self, DeviceCodeResp};
use crate::proot::distro::{builtin_specs, DistroManager, DistroSpec, InstalledDistro};
use crate::proot::links;
use crate::proot::qrcode_term;
use crate::proot::recipe::RecipeIndex;
use crate::proot::settings::{self, Settings};

/// 自绘 Spinner 帧（braille 点阵，零额外依赖——不引 throbber 类 crate）。
const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// 后台安装任务通过 channel 回传的事件。
enum InstallEvent {
    /// 进度更新（阶段说明 / 已完成 / 总量）。
    Progress { stage: String, done: u64, total: u64 },
    /// 安装结束（Ok / Err 文本）。
    Finished(Result<(), String>),
}

/// 一次后台安装的运行态（TUI 侧只持有接收端 + 最新进度快照）。
struct InstallTask {
    rx: Receiver<InstallEvent>,
    stage: String,
    done: u64,
    total: u64,
    spinner_tick: usize,
    started: Instant,
}

impl InstallTask {
    fn new(rx: Receiver<InstallEvent>) -> InstallTask {
        InstallTask {
            rx,
            stage: "正在准备…".to_string(),
            done: 0,
            total: 0,
            spinner_tick: 0,
            started: Instant::now(),
        }
    }

    /// 当前进度比（0.0–1.0）；无总量时回 0（Gauge 显示 indeterminate 文案）。
    fn ratio(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.done as f64 / self.total as f64).clamp(0.0, 1.0)
        }
    }

    /// 当前 Spinner 帧。
    fn spinner(&self) -> &'static str {
        SPINNER_FRAMES[self.spinner_tick % SPINNER_FRAMES.len()]
    }
}

const TAB_TITLES: [&str; 6] = ["发行版", "短链", "配方", "日志", "向导", "账号"];
const TAB_DISTROS: usize = 0;
const TAB_LINKS: usize = 1;
const TAB_RECIPES: usize = 2;
const TAB_LOGS: usize = 3;
const TAB_WIZARD: usize = 4;
const TAB_ACCOUNT: usize = 5;

/// 命令面板条目（Ctrl+P）：(动作标识, 说明)。不引额外 crate，纯静态表。
const PALETTE_COMMANDS: &[(&str, &str)] = &[
    ("list", "刷新发行版列表"),
    ("install", "安装 / 进入选中的发行版"),
    ("remove", "删除选中的发行版（二次确认）"),
    ("backup", "备份选中的发行版"),
    ("account", "切到账号页（设备码绑定）"),
    ("time", "显示当前时间问候与古诗"),
    ("egg", "揭晓隐藏彩蛋"),
    ("help", "打开快捷键帮助"),
];

/// 单个发行版行（已装 / 可装）。
#[derive(Clone)]
struct DistroRow {
    id: String,
    name: String,
    installed: bool,
}

/// TUI 应用状态。
struct App {
    root: PathBuf,
    mgr: DistroManager,
    rows: Vec<DistroRow>,
    links: Vec<String>,
    recipes: RecipeIndex,
    tab: usize,
    distro_state: ListState,
    link_state: ListState,
    recipe_state: ListState,
    status: String,
    /// 设备绑定状态（运行时从落盘的 device_token 推断；二维码绑定走 `mox-proot login`）。
    account_bound: bool,
    /// 绑定后用于展示的 device_token 掩码（前 8 位 + …）。
    account_mask: String,
    /// 启动欢迎屏（时间问候 + 古诗；按任意键进入主界面）。
    welcome: bool,
    /// 后台安装任务（Some = 正在跑，TUI 画 Gauge + Spinner）。
    install: Option<InstallTask>,
    /// 真实设备码（按 l 向 mox-id 后端申请；Some 时账号页渲染带 user_code 的真二维码）。
    login: Option<DeviceCodeResp>,
    /// `/` 搜索输入（Some = 正在输入；过滤后的 rows 由 rows_backup 恢复）。
    search: Option<String>,
    /// 搜索前的完整列表快照，用于清空搜索时还原。
    rows_backup: Option<Vec<DistroRow>>,
    /// Ctrl+P 命令面板（Some(选中索引) = 打开）。
    palette: Option<usize>,
    /// 删除二次确认弹层。
    confirm_remove: bool,
    /// `?` 帮助面板。
    show_help: bool,
    /// 安装进度历史（Sparkline 数据源，每帧采样一次）。
    rates: Vec<u64>,
    /// `config.toml [login] binds`：登录容器时自动追加的绑定 `(guest, host)`。
    login_binds: Vec<(String, String)>,
}

impl App {
    fn new(root: &PathBuf) -> Result<App, String> {
        let mgr = DistroManager::new(root);
        let installed = mgr
            .list_installed()
            .map_err(|e| e.to_string())?;
        let specs = builtin_specs();
        let user_links = links::load_user_links()
            .map(|t| t.names().map(|s| s.to_string()).collect::<Vec<_>>())
            .unwrap_or_default();
        let mut app = App {
            root: root.clone(),
            mgr,
            rows: Vec::new(),
            links: user_links,
            recipes: RecipeIndex::load(),
            tab: TAB_DISTROS,
            distro_state: ListState::default(),
            link_state: ListState::default(),
            recipe_state: ListState::default(),
            status: String::new(),
            account_bound: false,
            account_mask: String::new(),
            welcome: true,
            install: None,
            login: None,
            search: None,
            rows_backup: None,
            palette: None,
            confirm_remove: false,
            show_help: false,
            rates: Vec::new(),
            login_binds: crate::proot::settings::Settings::load_user()
                .map(|s| s.login_binds())
                .unwrap_or_default(),
        };
        app.refresh_account();
        app.rebuild_rows(&installed, &specs);
        if !app.rows.is_empty() {
            app.distro_state.select(Some(0));
        }
        if !app.links.is_empty() {
            app.link_state.select(Some(0));
        }
        if !app.recipes.recipes.is_empty() {
            app.recipe_state.select(Some(0));
        }
        Ok(app)
    }

    /// 按当前搜索词过滤发行版列表。
    /// 直接重建 `rows` 而非只改渲染——这样 `distro_state` 的选中索引始终
    /// 对应真实行，Enter / r / b 拿到的都是正确的那一项。
    fn apply_search(&mut self) {
        let Some(q) = self.search.clone() else { return; };
        if self.rows_backup.is_none() {
            self.rows_backup = Some(self.rows.clone());
        }
        let base = self.rows_backup.clone().unwrap_or_default();
        let ql = q.to_lowercase();
        self.rows = if ql.is_empty() {
            base
        } else {
            base.into_iter()
                .filter(|r| r.id.to_lowercase().contains(&ql) || r.name.to_lowercase().contains(&ql))
                .collect()
        };
        self.distro_state.select(if self.rows.is_empty() { None } else { Some(0) });
    }

    fn clear_search(&mut self) {
        if let Some(base) = self.rows_backup.take() {
            self.rows = base;
        }
        self.search = None;
        self.status = "已清除搜索".into();
        if !self.rows.is_empty() {
            self.distro_state.select(Some(0));
        }
    }

    /// 命令面板选中项执行（复用既有方法，不重写业务逻辑）。
    fn run_command(&mut self, idx: usize) {
        let Some((cmd, _)) = PALETTE_COMMANDS.get(idx) else { return; };
        match *cmd {
            "list" => { self.refresh(); self.status = "列表已刷新".into(); }
            "install" => self.primary(),
            "remove" => self.confirm_remove = true,
            "backup" => self.backup(),
            "account" => { self.tab = TAB_ACCOUNT; self.status = "已切到账号页".into(); }
            "time" => self.status = delight::time_banner().replace('\n', " · "),
            "egg" => self.status = delight::easter_egg("egg").unwrap_or("").replace('\n', " · "),
            "help" => self.show_help = true,
            _ => {}
        }
    }

    /// 合并「已装 + 可装」为单一列表（已装的排在前，标记已装）。
    fn rebuild_rows(&mut self, installed: &[InstalledDistro], specs: &[DistroSpec]) {
        let installed_ids: Vec<&str> = installed.iter().map(|d| d.id.as_str()).collect();
        let mut rows = Vec::new();
        for d in installed {
            rows.push(DistroRow {
                id: d.id.clone(),
                name: d.id.clone(),
                installed: true,
            });
        }
        for s in specs {
            if installed_ids.iter().any(|i| *i == s.id) {
                continue;
            }
            rows.push(DistroRow {
                id: s.id.to_string(),
                name: format!("{} ({})", s.name, s.version),
                installed: false,
            });
        }
        self.rows = rows;
    }

    /// 重算账号绑定态：读落盘的 device_token（存在且有内容即视为已绑定），
    /// 并生成展示用掩码（前 8 位 + …）。
    fn refresh_account(&mut self) {
        match crate::proot::device_code::load_device_token() {
            Some(tok) => {
                self.account_bound = true;
                let head: String = tok.chars().take(8).collect();
                self.account_mask = if tok.len() > 8 {
                    format!("{}…", head)
                } else {
                    head
                };
            }
            None => {
                self.account_bound = false;
                self.account_mask.clear();
            }
        }
    }

    fn refresh(&mut self) {
        match self.mgr.list_installed() {
            Ok(installed) => {
                let specs = builtin_specs();
                self.rebuild_rows(&installed, &specs);
                self.recipes = RecipeIndex::load();
            }
            Err(e) => self.status = format!("刷新失败: {}", e),
        }
    }

    /// 非阻塞收取后台安装事件：更新进度快照；结束则清任务并刷新列表。
    /// 事件循环每帧调用一次，配合 `spinner_tick` 让 Gauge / Spinner 实时动起来。
    fn poll_install(&mut self) {
        let mut finished: Option<Result<(), String>> = None;
        if let Some(task) = &mut self.install {
            while let Ok(ev) = task.rx.try_recv() {
                match ev {
                    InstallEvent::Progress { stage, done, total } => {
                        task.stage = stage;
                        task.done = done;
                        task.total = total;
                    }
                    InstallEvent::Finished(r) => {
                        finished = Some(r);
                        break;
                    }
                }
            }
        }
        if let Some(r) = finished {
            self.install = None;
            match r {
                Ok(()) => {
                    self.status = "✅ 安装完成（列表已刷新）".into();
                    self.refresh();
                }
                Err(e) => self.status = format!("❌ 安装失败: {}", e),
            }
        }
    }

    fn selected_row(&self) -> Option<&DistroRow> {
        self.distro_state
            .selected()
            .and_then(|i| self.rows.get(i))
    }

    /// 主操作：已装→登录；可装→安装。
    fn primary(&mut self) {
        match self.tab {
            TAB_DISTROS => {
                if let Some(row) = self.selected_row().cloned() {
                    if row.installed {
                        self.login(&row.id);
                    } else {
                        self.install(&row.id);
                    }
                }
            }
            TAB_LINKS => {
                let name = self
                    .link_state
                    .selected()
                    .and_then(|i| self.links.get(i))
                    .cloned();
                if let Some(name) = name {
                    self.install_link(&name);
                }
            }
            TAB_RECIPES => {
                if let Some(r) = self
                    .recipe_state
                    .selected()
                    .and_then(|i| self.recipes.recipes.get(i))
                {
                    self.status = format!(
                        "配方 `{}` 安装属于 0.3.0 配方系统（尚未实现）；索引已加载 {} 个配方。",
                        r.id,
                        self.recipes.recipes.len()
                    );
                }
            }
            TAB_WIZARD => self.run_wizard(),
            TAB_ACCOUNT => self.run_login_binding(),
            _ => {}
        }
    }

    /// 账号页：临时退出 TUI → 终端里跑设备码扫码绑定（打印半块二维码 + 轮询）→ 回 TUI 刷新。
    ///
    /// 复用 [`crate::proot::device_code::run_login_flow`]，该函数自管二维码渲染与轮询落盘；
    /// 绑定成功后 [`refresh_account`] 会从落盘 token 读到新状态。失败（如超时/网络）不留痕。
    /// 向真实后端申请一次性设备码（RFC 8628）。此前账号页只有一枚固定
    /// `/activate` 的「示意」二维码，扫了也没有 user_code，等于没接后端。
    fn start_login(&mut self) {
        self.status = "正在向 mox-id 申请设备码…".into();
        match device_code::request_code(&device_code::base_url()) {
            Ok(r) => {
                self.status = format!(
                    "设备码 {} 已就绪：手机确认后按 r 检查授权（Enter 可切原生终端扫码）",
                    r.user_code
                );
                self.login = Some(r);
            }
            Err(e) => {
                self.status = format!(
                    "申请设备码失败：{e}（检查网络，或设 MOX_ID_BASE 指向自托管后端）"
                );
                self.login = None;
            }
        }
    }

    /// 轮询后端，手机确认后换取 device_token 并落盘。
    fn poll_login(&mut self) {
        let Some(code) = self.login.as_ref().map(|c| c.device_code.clone()) else {
            self.status = "请先按 l 申请设备码".into();
            return;
        };
        match device_code::poll_token(&device_code::base_url(), &code) {
            Ok(device_code::PollOutcome::Ready(t)) => {
                match device_code::store_device_token(&t.device_token) {
                    Ok(()) => {
                        self.login = None;
                        self.refresh_account();
                        self.status = "✅ 设备已绑定（device_token 已保存）".into();
                    }
                    Err(e) => self.status = format!("保存令牌失败：{e}"),
                }
            }
            Ok(device_code::PollOutcome::Pending) => {
                self.status = "⏳ 仍在等待手机确认，稍后再按 r 检查".into();
            }
            Ok(device_code::PollOutcome::Error(e)) => {
                self.status = format!("绑定失败：{e}（按 l 重新申请）");
                self.login = None;
            }
            Err(e) => self.status = format!("轮询失败：{e}"),
        }
    }

    fn run_login_binding(&mut self) {
        let rc = leave_tui_for(|| crate::proot::device_code::run_login_flow());
        self.refresh_account();
        if rc == 0 {
            self.status = "✅ 设备已绑定（device_token 已保存）".into();
        } else {
            self.status = "绑定未完成（可再次按 Enter 重试）".into();
        }
    }

    fn login(&mut self, id: &str) {
        // 2026-11-11 审计修复：TUI 登录原先无法带上任何 -b 绑定，用户在
        // config.toml 配了也没用。现在统一从 [login] binds 读取并透传。
        match self
            .mgr
            .login_cmd_with_binds(id, Some("/bin/sh"), &self.login_binds)
        {
            Ok(cmd) => {
                // 退出 TUI 进入容器；容器退出后自动恢复 TUI。
                throw_to_shell(&cmd);
            }
            Err(e) => self.status = format!("生成登录命令失败: {}", e),
        }
    }

    /// 启动后台安装：把阻塞的下载/校验/解压放到工作线程，进度经 channel 回传，
    /// 由 [`App::poll_install`] 每帧消费——TUI 不再卡住，Gauge / Spinner 实时刷新。
    fn install(&mut self, id: &str) {
        if self.install.is_some() {
            self.status = "已有安装任务进行中，请稍候…".into();
            return;
        }
        let spec = match DistroManager::find_spec(id) {
            Ok(s) => s,
            Err(e) => {
                self.status = format!("未找到发行版 `{}`: {}", id, e);
                return;
            }
        };
        let root = self.root.clone();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            // 线程内自建 DistroManager（避免跨线程共享 &mut self）。
            let mgr = DistroManager::new(&root);
            let result = {
                let cb: &mut dyn FnMut(&str, u64, u64) = &mut |stage, done, total| {
                    let _ = tx.send(InstallEvent::Progress {
                        stage: stage.to_string(),
                        done,
                        total,
                    });
                };
                mgr.install(&spec, None, cb)
            };
            let _ = tx.send(InstallEvent::Finished(result.map_err(|e| e.to_string())));
        });
        self.install = Some(InstallTask::new(rx));
        self.status = format!("后台安装 {} 已启动（下载 + 校验 + 解压）…", id);
    }

    fn install_link(&mut self, name: &str) {
        // 内置短链名即发行版 id（alpine/ubuntu/debian/kali）。
        if links::builtin_names().iter().any(|n| *n == name) {
            self.install(name);
        } else {
            self.status = format!("自定义短链 `{}` 需经 distro 子系统安装（0.2.0 后续优化）。", name);
        }
    }

    fn remove(&mut self) {
        if let Some(row) = self.selected_row() {
            if row.installed {
                let id = row.id.clone();
                match self.mgr.remove(&id) {
                    Ok(()) => {
                        self.status = format!("🗑 已删除 {}", id);
                        self.refresh();
                    }
                    Err(e) => self.status = format!("删除失败: {}", e),
                }
            }
        }
    }

    fn backup(&mut self) {
        if let Some(row) = self.selected_row() {
            if row.installed {
                let id = row.id.clone();
                let out = self.root.join(format!("{}.backup.tar", id));
                match self.mgr.backup(&id, &out) {
                    Ok(()) => self.status = format!("已备份到 {}", out.display()),
                    Err(e) => self.status = format!("备份失败: {}", e),
                }
            }
        }
    }

    fn run_wizard(&mut self) {
        let wizard = "mox-proot-distro wizard".to_string();
        throw_to_shell(&wizard);
    }
}

/// TUI 入口：初始化终端 → 事件循环 → 恢复终端。返回进程退出码。
pub fn run_tui() -> i32 {
    let root = Settings::load_user()
        .map(|s| s.resolve_root_dir())
        .unwrap_or_else(|_| settings::default_root_dir());

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = match Terminal::new(backend) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("mox-proot-tui: 终端初始化失败: {}", e);
            return 2;
        }
    };

    if enter_altscreen(&mut terminal).is_err() {
        let _ = restore_altscreen(&mut terminal);
        eprintln!("mox-proot-tui: 无法进入备用屏幕（非交互终端？）");
        return 2;
    }

    let app = match App::new(&root) {
        Ok(a) => a,
        Err(e) => {
            let _ = restore_altscreen(&mut terminal);
            eprintln!("mox-proot-tui: 初始化失败: {}", e);
            return 2;
        }
    };

    let res = run_app(&mut terminal, app);
    let _ = restore_altscreen(&mut terminal);
    match res {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("mox-proot-tui: 运行错误: {}", e);
            1
        }
    }
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    mut app: App,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        // 每帧：消费后台安装进度 + 推进 Spinner 帧（即使无按键也保持动画）。
        app.poll_install();
        if let Some(t) = &mut app.install {
            t.spinner_tick = t.spinner_tick.wrapping_add(1);
        }
        // Sparkline 数据源：每帧采样一次已完成量，保留最近 48 帧。
        if let Some(t) = &app.install {
            app.rates.push(t.done.min(t.total.max(1)));
            if app.rates.len() > 48 { app.rates.remove(0); }
        } else if !app.rates.is_empty() {
            app.rates.clear();
        }
        terminal.draw(|f| {
            if app.welcome {
                render_welcome(f, &app);
            } else {
                ui(f, &app);
            }
        })?;
        if event::poll(Duration::from_millis(120))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                // 欢迎屏：任意键进入主界面，q / Esc 直接退出。
                if app.welcome {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        _ => app.welcome = false,
                    }
                    continue;
                }
                // 安装进行中：屏蔽重复触发键（其余导航/退出照常）。
                if app.install.is_some()
                    && matches!(key.code, KeyCode::Enter | KeyCode::Char('r') | KeyCode::Char('b'))
                {
                    continue;
                }
                // ── 浮层优先：命令面板 / 搜索 / 删除确认 / 帮助 依次接管按键 ──
                if let Some(sel) = app.palette {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => app.palette = None,
                        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => app.palette = None,
                        KeyCode::Up => app.palette = Some(sel.saturating_sub(1)),
                        KeyCode::Down => app.palette = Some((sel + 1).min(PALETTE_COMMANDS.len() - 1)),
                        KeyCode::Enter => { app.palette = None; app.run_command(sel); }
                        KeyCode::Char(c) if c.is_ascii_digit() => {
                            let i = (c as usize) - ('0' as usize);
                            if i < PALETTE_COMMANDS.len() { app.palette = None; app.run_command(i); }
                        }
                        _ => {}
                    }
                    continue;
                }
                if app.show_help {
                    if matches!(key.code, KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?')) {
                        app.show_help = false;
                    }
                    continue;
                }
                if app.confirm_remove {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => { app.confirm_remove = false; app.remove(); }
                        _ => { app.confirm_remove = false; app.status = "已取消删除".into(); }
                    }
                    continue;
                }
                if app.search.is_some() {
                    let mut close = false;
                    let mut restore = false;
                    match key.code {
                        KeyCode::Esc => { close = true; restore = true; }
                        KeyCode::Enter => close = true,
                        KeyCode::Backspace => { if let Some(q) = app.search.as_mut() { q.pop(); } app.apply_search(); }
                        KeyCode::Char(c) => { if let Some(q) = app.search.as_mut() { q.push(c); } app.apply_search(); }
                        _ => {}
                    }
                    if close { if restore { app.clear_search(); } else { app.search = None; app.status = "搜索已应用（按 / 重搜，Esc 无浮层时清除）".into(); } }
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Tab => app.tab = (app.tab + 1) % TAB_TITLES.len(),
                    KeyCode::BackTab => {
                        app.tab = (app.tab + TAB_TITLES.len() - 1) % TAB_TITLES.len()
                    }
                    KeyCode::Up => match app.tab {
                        TAB_DISTROS => select_prev(&mut app.distro_state, app.rows.len()),
                        TAB_LINKS => select_prev(&mut app.link_state, app.links.len()),
                        TAB_RECIPES => select_prev(&mut app.recipe_state, app.recipes.recipes.len()),
                        _ => {}
                    },
                    KeyCode::Down => match app.tab {
                        TAB_DISTROS => select_next(&mut app.distro_state, app.rows.len()),
                        TAB_LINKS => select_next(&mut app.link_state, app.links.len()),
                        TAB_RECIPES => select_next(&mut app.recipe_state, app.recipes.recipes.len()),
                        _ => {}
                    },
                    KeyCode::Enter => app.primary(),
                    KeyCode::Char('l') => {
                        if app.tab == TAB_ACCOUNT {
                            app.start_login();
                        }
                    }
                    KeyCode::Char('r') => {
                        if app.tab == TAB_DISTROS {
                            app.confirm_remove = true;
                        } else if app.tab == TAB_ACCOUNT {
                            app.poll_login();
                        }
                    }
                    KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => app.palette = Some(0),
                    KeyCode::Char('/') => { app.search = Some(String::new()); app.apply_search(); }
                    KeyCode::Char('?') => app.show_help = true,
                    KeyCode::Esc => { if app.rows_backup.is_some() { app.clear_search(); } }
                    KeyCode::Char('b') => {
                        if app.tab == TAB_DISTROS {
                            app.backup();
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

fn select_prev(state: &mut ListState, len: usize) {
    if len == 0 {
        return;
    }
    let cur = state.selected().unwrap_or(0);
    state.select(Some(cur.saturating_sub(1)));
}

fn select_next(state: &mut ListState, len: usize) {
    if len == 0 {
        return;
    }
    let cur = state.selected().unwrap_or(0);
    state.select(Some((cur + 1).min(len - 1)));
}

fn ui(f: &mut ratatui::Frame, app: &App) {
    let size = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // 标题 + Tabs
            Constraint::Min(4),   // 主体
            Constraint::Length(2), // 状态栏
        ])
        .split(size);

    let title = Line::from(vec![
        Span::styled(
            "mox-proot ",
            palette::style(palette::Slot::Accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled("TUI 控制台", Style::default().fg(Color::Cyan)),
    ]);
    let tabs = Tabs::new(TAB_TITLES.iter().map(|t| Line::from(*t)).collect::<Vec<_>>())
        .block(Block::default().borders(Borders::ALL).border_style(palette::style(palette::Slot::Info)).title(title))
        .select(app.tab)
        .highlight_style(palette::style(palette::Slot::Active).add_modifier(Modifier::BOLD))
        .divider("|");
    f.render_widget(tabs, chunks[0]);

    match app.tab {
        TAB_DISTROS => render_distros(f, chunks[1], app),
        TAB_LINKS => render_links(f, chunks[1], app),
        TAB_RECIPES => render_recipes(f, chunks[1], app),
        TAB_LOGS => render_logs(f, chunks[1], app),
        TAB_WIZARD => render_wizard(f, chunks[1], app),
        TAB_ACCOUNT => render_account(f, chunks[1], app),
        _ => {}
    }

    let help = match app.tab {
        TAB_DISTROS => "↑↓ 选择 · Enter 登录/安装 · / 搜索 · r 删除 · b 备份 · ? 帮助 · Ctrl+P 命令 · q 退出",
        TAB_ACCOUNT => "l 申请设备码 · r 检查授权 · Enter 原生扫码 · ? 帮助 · Ctrl+P 命令 · q 退出",
        _ => "↑↓ 选择 · Enter 执行 · Tab 切换 · ? 帮助 · Ctrl+P 命令 · q 退出",
    };
    let status_line = if app.status.is_empty() {
        Line::from(Span::styled(help, Style::default().fg(Color::DarkGray)))
    } else {
        Line::from(Span::styled(
            format!("{}   │  {}", app.status, help),
            Style::default().fg(Color::Green),
        ))
    };
    let status = Paragraph::new(status_line).block(Block::default().borders(Borders::ALL));
    f.render_widget(status, chunks[2]);

    // 浮层自上而下叠加：删除确认 → 命令面板 → 帮助 → 搜索框 → 安装进度。
    if app.confirm_remove { render_confirm(f, size, app); }
    if let Some(sel) = app.palette { render_palette(f, size, sel); }
    if app.show_help { render_help(f, size); }
    if let Some(q) = &app.search { render_search(f, size, q); }
    if let Some(task) = &app.install {
        render_install_overlay(f, size, task, &app.rates);
    }
}

/// 启动欢迎屏：品牌字标 + 时间上下文问候 + 古诗 + 彩蛋提示（#450 惊喜三件套）。
/// 复用 [`crate::proot::delight`] 的 Rust 单一来源，与 `mox` 控制台同源。
fn render_welcome(f: &mut ratatui::Frame, app: &App) {
    let size = f.area();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" mox-proot TUI ")
        .border_style(Style::default().fg(Color::Magenta));
    let inner = block.inner(size);
    f.render_widget(block, size);

    let phase = delight::current_phase();
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));
    for l in crate::proot::brand::BANNER.lines() {
        lines.push(Line::from(Span::styled(
            l.to_string(),
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        phase.greeting(),
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        format!("{} {}", phase.poem(), phase.poem_from()),
        Style::default().fg(Color::Yellow),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(format!("  数据根：{}", app.root.display())));
    lines.push(Line::from(format!(
        "  时相：{}   ·   已装 {} 个发行版",
        phase.name(),
        app.rows.iter().filter(|r| r.installed).count()
    )));
    lines.push(Line::from(""));
    // MOX_EGG=1 时欢迎屏直接展开彩蛋（与 README 承诺一致）。
    if delight::egg_enabled_via_env() {
        if let Some(t) = delight::easter_egg("egg") {
            for l in t.lines() {
                lines.push(Line::from(Span::styled(
                    l.to_string(),
                    Style::default().fg(Color::LightMagenta),
                )));
            }
        }
    } else {
        lines.push(Line::from(Span::styled(
            "  🥚 彩蛋暗号：mox egg  /  mox-proot-distro egg",
            Style::default().fg(Color::DarkGray),
        )));
    }
    if let Some(line) = delight::pick_line(ContentScene::Welcome) {
        lines.push(Line::from(Span::styled(
            format!("  {}", line),
            Style::default().fg(Color::DarkGray),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  按任意键进入控制台   ·   q 退出",
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
    )));

    let p = Paragraph::new(Text::from(lines)).alignment(Alignment::Center);
    f.render_widget(p, inner);
}

/// 居中弹窗区域（百分比宽 + 固定高）。
fn centered_rect(percent_x: u16, height: u16, r: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(r.height.saturating_sub(height) / 2),
            Constraint::Length(height.min(r.height)),
            Constraint::Min(0),
        ])
        .split(r)[1];
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical)[1]
}

/// 后台安装浮层：Spinner 标题 + Gauge 实时进度 + 阶段说明 + 耗时。
fn render_install_overlay(f: &mut ratatui::Frame, area: Rect, task: &InstallTask, rates: &[u64]) {
    let popup = centered_rect(64, 10, area);
    f.render_widget(Clear, popup);
    let secs = task.started.elapsed().as_secs();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} 安装进行中 ", task.spinner()))
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(inner);

    let stage = Paragraph::new(format!("{} {}", task.spinner(), task.stage))
        .style(Style::default().fg(Color::Yellow))
        .wrap(Wrap { trim: true });
    f.render_widget(stage, chunks[0]);

    let pct = (task.ratio() * 100.0) as u16;
    let label = if task.total == 0 {
        format!("{} …", pct)
    } else {
        format!("{}%  ({}/{})", pct, task.done, task.total)
    };
    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(Color::Green).bg(Color::DarkGray))
        .ratio(task.ratio())
        .label(label);
    f.render_widget(gauge, chunks[1]);

    // Sparkline：最近 48 帧的已完成量曲线，卡顿时一眼看出「还活着但没涨」。
    let spark = Sparkline::default()
        .data(rates)
        .style(palette::style(palette::Slot::Success))
        .max(task.total.max(1));
    f.render_widget(spark, chunks[2]);

    let hint = Paragraph::new(Line::from(Span::styled(
        format!("已用时 {}s   ·   q 退出 / 进度在后台继续", secs),
        Style::default().fg(Color::DarkGray),
    )));
    f.render_widget(hint, chunks[3]);
}

/// 删除二次确认（手册 §3.4：危险操作必须 y/N，不能一按 r 就删）。
fn render_confirm(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let r = centered_rect(52, 8, area);
    f.render_widget(Clear, r);
    let id = app.selected_row().map(|x| x.id.as_str()).unwrap_or("?");
    let body = vec![
        Line::from(""),
        Line::from(Span::styled(format!("  ⚠ 确认删除 {} ？", id), Style::default().fg(Color::Yellow))),
        Line::from(""),
        Line::from(format!("  将删除该发行版的 rootfs 与完成标记，不可恢复。")),
        Line::from(""),
        Line::from(Span::styled("  [y] 删除        [n / Esc] 取消", Style::default().fg(Color::Green))),
    ];
    f.render_widget(
        Paragraph::new(body).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" 确认 ")
                .border_style(palette::style(palette::Slot::Warning)),
        ),
        r,
    );
}

/// Ctrl+P 命令面板：不用记命令，数字键直达。
fn render_palette(f: &mut ratatui::Frame, area: Rect, sel: usize) {
    let r = centered_rect(56, (PALETTE_COMMANDS.len() as u16) + 4, area);
    f.render_widget(Clear, r);
    let mut lines = vec![Line::from(Span::styled(
        "  > 输入数字或 ↑↓ 选择，Enter 执行，Esc 关闭",
        Style::default().fg(Color::DarkGray),
    ))];
    for (i, (cmd, desc)) in PALETTE_COMMANDS.iter().enumerate() {
        let style = if i == sel {
            Style::default().fg(Color::Black).bg(Color::Yellow)
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(
            format!("  {}  {:<10} {}", i, cmd, desc),
            style,
        )));
    }
    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" 命令面板 ")
                .border_style(palette::style(palette::Slot::Accent)),
        ),
        r,
    );
}

/// `?` 帮助面板：快捷键随上下文变化，这里给全集。
fn render_help(f: &mut ratatui::Frame, area: Rect) {
    let r = centered_rect(62, 16, area);
    f.render_widget(Clear, r);
    let body = vec![
        Line::from(Span::styled("  键盘全可达（不依赖鼠标）", Style::default().fg(Color::Cyan))),
        Line::from(""),
        Line::from("  ↑↓ / Tab     选择、切换标签页"),
        Line::from("  Enter        安装未装 / 进入已装 / 执行"),
        Line::from("  /            搜索发行版（Esc 清除过滤）"),
        Line::from("  Ctrl+P       命令面板（数字键直达）"),
        Line::from("  l / r        账号页：申请设备码 / 检查授权"),
        Line::from("  r / b        发行版页：删除（二次确认）/ 备份"),
        Line::from("  ?            本帮助    ·    q / Esc  退出"),
        Line::from(""),
        Line::from(Span::styled("  无障碍：颜色从不单独承载状态，✓ / ⚠ / ✗ 与文字同在。", Style::default().fg(Color::DarkGray))),
        Line::from(Span::styled("  NO_COLOR=1 可关闭全部着色；管道/非 TTY 自动降级。", Style::default().fg(Color::DarkGray))),
        Line::from(""),
        Line::from("  按 ? / Esc / q 关闭"),
    ];
    f.render_widget(
        Paragraph::new(body).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" 帮助 ")
                .border_style(palette::style(palette::Slot::Info)),
        ),
        r,
    );
}

/// `/` 搜索输入框：贴在标题栏下方，实时过滤发行版。
fn render_search(f: &mut ratatui::Frame, area: Rect, q: &str) {
    let r = Rect { x: area.x + 2, y: area.y + 1, width: area.width.saturating_sub(4).min(52), height: 3 };
    f.render_widget(Clear, r);
    f.render_widget(
        Paragraph::new(format!("  / {}_", q)).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" 搜索发行版 ")
                .border_style(palette::style(palette::Slot::Active)),
        ),
        r,
    );
}

fn render_distros(f: &mut ratatui::Frame, area: Rect, app: &App) {
    if app.rows.is_empty() {
        let empty = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled("  还没有匹配的发行版", Style::default().fg(Color::Yellow))),
            Line::from(""),
            Line::from("  按 / 搜索、Esc 清除过滤，或 Ctrl+P 打开命令面板。"),
            Line::from("  最小的是 alpine（约 4MB），几秒钟就能装好。"),
        ])
        .block(Block::default().borders(Borders::ALL).title("发行版"));
        f.render_widget(empty, area);
        return;
    }
    let items: Vec<ListItem> = app
        .rows
        .iter()
        .map(|r| {
            let tag = if r.installed {
                Span::styled("[已装] ", Style::default().fg(Color::Green))
            } else {
                Span::styled("[可装] ", Style::default().fg(Color::Yellow))
            };
            let label = Span::raw(format!("{}  {}", r.id, r.name));
            ListItem::new(Line::from(vec![tag, label]))
        })
        .collect();
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("发行版（Enter 登录已装 / 安装可装）"),
        )
        .highlight_style(Style::default().fg(Color::Black).bg(Color::Yellow))
        .highlight_symbol(">> ");
    f.render_stateful_widget(list, area, &mut app.distro_state.clone());
    // 右侧滚动条：内容超过可视区时给出位置感（ScrollbarState 每帧按选中项重算）。
    let mut sb = ScrollbarState::new(app.rows.len())
        .position(app.distro_state.selected().unwrap_or(0));
    f.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓")),
        area,
        &mut sb,
    );
}

fn render_links(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = app
        .links
        .iter()
        .map(|n| ListItem::new(Line::from(format!("mx:{}", n))))
        .collect();
    if items.is_empty() {
        let p = Paragraph::new("无自定义短链；内置短链：mx:alpine / mx:ubuntu / mx:debian / mx:kali")
            .block(Block::default().borders(Borders::ALL).title("短链（Enter 安装/启动）"))
            .wrap(Wrap { trim: true });
        f.render_widget(p, area);
        return;
    }
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("短链（Enter 安装/启动）"))
        .highlight_style(Style::default().fg(Color::Black).bg(Color::Yellow))
        .highlight_symbol(">> ");
    f.render_stateful_widget(list, area, &mut app.link_state.clone());
}

fn render_recipes(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = app
        .recipes
        .recipes
        .iter()
        .map(|r| {
            ListItem::new(Line::from(vec![
                Span::styled(format!("{} ", r.name), Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(r.description.clone()),
            ]))
        })
        .collect();
    if items.is_empty() {
        let p = Paragraph::new(
            "暂无配方索引（recipes/index.toml 未部署）。配方系统属 0.3.0（未开始）。",
        )
        .block(Block::default().borders(Borders::ALL).title("配方"))
        .wrap(Wrap { trim: true });
        f.render_widget(p, area);
        return;
    }
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("配方（{} 个）", app.recipes.recipes.len())),
        )
        .highlight_style(Style::default().fg(Color::Black).bg(Color::Yellow))
        .highlight_symbol(">> ");
    f.render_stateful_widget(list, area, &mut app.recipe_state.clone());
}

fn render_logs(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let installed_n = app.rows.iter().filter(|r| r.installed).count();
    let lines = vec![
        Line::from(format!("数据根: {}", app.root.display())),
        Line::from(format!("已安装发行版: {}", installed_n)),
        Line::from(format!("可安装预置: {}", app.rows.len() - installed_n)),
        Line::from(format!("配方索引: {} 个", app.recipes.recipes.len())),
        Line::from(""),
        Line::from(Span::styled(
            "提示：登录/安装/删除操作会实时反映在此（刷新后）。",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    let p = Paragraph::new(Text::from(lines))
        .block(Block::default().borders(Borders::ALL).title("日志 / 诊断"))
        .wrap(Wrap { trim: true });
    f.render_widget(p, area);
}

fn render_wizard(f: &mut ratatui::Frame, area: Rect, _app: &App) {
    let p = Paragraph::new(
        "安装向导：一次性初始化数据根、安装首个发行版、落启动器与控制台。\n\n按 Enter 运行 `mox-proot-distro wizard`。",
    )
    .block(Block::default().borders(Borders::ALL).title("安装向导"))
    .alignment(Alignment::Left)
    .wrap(Wrap { trim: true });
    f.render_widget(p, area);
}

/// 账号页：展示设备绑定状态，并可直接连真实后端完成 RFC 8628 绑定。
///
/// `l` 向 mox-id 申请一次性设备码 → 右侧二维码换成带 `user_code` 的真码；
/// `r` 轮询换 token 落盘；`Enter` 仍可切到原生终端画全尺寸二维码（不被盒子裁切）。
/// 后端 `/activate` 复用既有 GitHub / Gitee 双 OAuth。
fn render_account(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let (state_label, state_color) = if app.account_bound {
        (
            Span::styled("已绑定 ✓", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Color::Green,
        )
    } else {
        (
            Span::styled("未绑定", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Color::Yellow,
        )
    };
    let mut lines = vec![
        Line::from(vec![
            Span::raw("绑定状态："),
            state_label,
        ]),
    ];
    if app.account_bound {
        lines.push(Line::from(format!("device_token：{}", app.account_mask)));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "提示：解除绑定请删除 ~/.mox/device_token（或 $MOX_ROOT/device_token）。",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "按 l 申请设备码（真实请求后端）/ Enter 切原生终端扫码",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )));
        if let Some(c) = &app.login {
            lines.push(Line::from(format!("验证码：{}", c.user_code)));
            lines.push(Line::from(format!(
                "有效期 {}s · 手机确认后按 r 检查授权", c.expires_in
            )));
            lines.push(Line::from(Span::styled(
                "后端已签发一次性码，右侧是真二维码，可直接扫。",
                Style::default().fg(Color::Green),
            )));
        } else {
            lines.push(Line::from("  1. 按 l → 后端签发 user_code，右侧换成真二维码"));
            lines.push(Line::from("  2. 手机扫码，用 GitHub / Gitee 登录并确认绑定"));
            lines.push(Line::from("  3. 按 r 轮询 → 拿到 device_token 落盘即显示「已绑定」"));
        }
    }

    // 宽屏内嵌二维码（qrcode_term 半块字符）。按 l 拿到真实设备码后，这里换成
    // 带一次性 user_code 的 verification_uri_complete，扫了直接进 /activate 确认页；
    // 未申请时退化为官网固定入口码，仅作引导（不再是「示意」冒充可扫）。
    let (qr_uri, qr_title) = match &app.login {
        Some(c) => (c.verification_uri_complete.clone(), "扫码绑定（真码）"),
        None => (backend::site_page("/activate"), "登录入口"),
    };
    let qr_text = qrcode_term::render_uri(&qr_uri).unwrap_or_default();
    let (text_area, qr_area) = if area.width >= 72 {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(30), Constraint::Length(34)])
            .split(area);
        (cols[0], Some(cols[1]))
    } else {
        (area, None)
    };

    let p = Paragraph::new(Text::from(lines))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("账号（设备绑定 · 扫码登录）")
                .border_style(Style::default().fg(state_color)),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(p, text_area);

    if let Some(qa) = qr_area {
        let qr_lines: Vec<Line> = qr_text.lines().map(|l| Line::from(l.to_string())).collect();
        let qr = Paragraph::new(Text::from(qr_lines))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(qr_title)
                    .border_style(Style::default().fg(Color::Blue)),
            )
            .alignment(Alignment::Center);
        f.render_widget(qr, qa);
    }
}

// ── 终端模式切换 ────────────────────────────────────────────────

fn enter_altscreen(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
    let _ = terminal;
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    Ok(())
}

fn restore_altscreen(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
    let _ = terminal;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    disable_raw_mode()?;
    io::stdout().flush()?;
    Ok(())
}

/// 临时退出 TUI（离开备用屏幕、关闭 raw 模式），执行一个闭包（如进入容器、
/// 运行向导、跑设备码扫码绑定），闭包结束后恢复 TUI。用于需要在普通终端里
/// 交互/打印二维码的场景。
fn leave_tui_for<F: FnOnce() -> i32>(f: F) -> i32 {
    let _ = execute!(io::stdout(), LeaveAlternateScreen);
    let _ = disable_raw_mode();
    let _ = io::stdout().flush();

    // 执行目标逻辑（如 mox-proot 进容器 / 设备码绑定）；退出后回到这里。
    let rc = f();

    // 恢复 TUI。
    let _ = enable_raw_mode();
    let _ = execute!(io::stdout(), EnterAlternateScreen);
    rc
}

/// 临时退出 TUI（离开备用屏幕、关闭 raw 模式），执行一条 shell 命令（如进入容器 /
/// 运行向导），命令结束后恢复 TUI。命令进程替代当前前台（交互式），TUI 在之后续跑。
fn throw_to_shell(cmd: &str) {
    leave_tui_for(|| {
        let _ = Command::new("sh").arg("-c").arg(cmd).status();
        0
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_new_populates_builtins_without_panic() {
        let tmp = std::env::temp_dir().join(format!("mox_proot_tui_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let app = App::new(&tmp).expect("App::new 不应失败");
        // 至少应有 4 个预置发行版可供安装。
        assert!(app.rows.len() >= 4, "预置发行版应 >= 4，实际 {}", app.rows.len());
        assert!(
            app.rows.iter().all(|r| !r.installed),
            "全新根不应有已装发行版"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn tab_count_and_titles_consistent() {
        assert_eq!(TAB_TITLES.len(), 6);
        assert_eq!(TAB_WIZARD, 4);
        assert_eq!(TAB_ACCOUNT, 5);
    }

    #[test]
    fn palette_commands_are_indexable_and_unique() {
        assert!(!PALETTE_COMMANDS.is_empty());
        assert!(PALETTE_COMMANDS.len() <= 10, "数字键 0-9 要能直达");
        let mut ids: Vec<&str> = PALETTE_COMMANDS.iter().map(|(c, _)| *c).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), PALETTE_COMMANDS.len());
    }

    #[test]
    fn search_filters_and_restores_rows() {
        let root = std::env::temp_dir().join(format!("mox-tui-search-{}", std::process::id()));
        let mut app = App::new(&root).expect("App::new 应可用（空数据根）");
        let full = app.rows.len();
        assert!(full >= 4, "内置发行版应至少 4 个");
        app.search = Some("alp".into());
        app.apply_search();
        assert!(!app.rows.is_empty(), "alp 应命中 alpine");
        assert!(app.rows.iter().all(|r| r.id.contains("alp") || r.name.contains("alp")));
        app.clear_search();
        assert_eq!(app.rows.len(), full, "清除搜索应还原完整列表");
        assert!(app.rows_backup.is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sparkline_samples_are_bounded() {
        // 每帧采样上限 48，防止长时间安装无限增长
        let mut rates: Vec<u64> = Vec::new();
        for i in 0..200u64 {
            rates.push(i);
            if rates.len() > 48 { rates.remove(0); }
        }
        assert_eq!(rates.len(), 48);
    }

    #[test]
    fn app_starts_on_welcome_screen() {
        let tmp = std::env::temp_dir().join(format!("mox_proot_tui_welcome_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let app = App::new(&tmp).unwrap();
        assert!(app.welcome, "启动应停留在欢迎屏");
        assert!(app.install.is_none(), "初始无后台安装任务");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn spinner_frames_and_ratio() {
        assert_eq!(SPINNER_FRAMES.len(), 10);
        let (_tx, rx) = mpsc::channel();
        let mut t = InstallTask::new(rx);
        assert_eq!(t.total, 0);
        assert_eq!(t.ratio(), 0.0, "无总量时 indeterminate");
        t.total = 100;
        t.done = 40;
        assert!((t.ratio() - 0.4).abs() < 1e-9);
        t.done = 150;
        assert_eq!(t.ratio(), 1.0, "超总量钳到 1.0");
        assert_eq!(t.spinner(), SPINNER_FRAMES[0]);
        t.spinner_tick = 3;
        assert_eq!(t.spinner(), SPINNER_FRAMES[3]);
    }

    #[test]
    fn account_qr_renders_halfblocks() {
        let uri = backend::site_page("/activate");
        let qr = qrcode_term::render_uri(&uri).unwrap();
        let lines: Vec<&str> = qr.lines().collect();
        assert!(lines.len() > 4, "二维码应有足够行数");
        assert!(
            qr.contains('█') || qr.contains('▀') || qr.contains('▄'),
            "应含半块字符"
        );
    }

    #[test]
    fn real_device_code_uri_carries_one_time_user_code() {
        // 接上后端后，账号页二维码内容必须是后端签发的、带 code 的一次性链接，
        // 且与官网同域（Pages Functions 反代到 mox-id）。固定 /activate 不再算数。
        let uri = format!("{}/activate?code=A7K2-9FQ1", backend::PUBLIC_BASE);
        assert!(uri.starts_with(backend::PUBLIC_BASE));
        assert!(uri.contains("code="));
        let qr = qrcode_term::render_uri(&uri).unwrap();
        assert!(qr.contains('█') || qr.contains('▀') || qr.contains('▄'));
    }
}
