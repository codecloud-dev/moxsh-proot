//! TUI 控制台（路线图 0.4.0）。
//!
//! ratatui 实现的第三二进制 `moxsh-proot-tui`，以及 `moxsh-proot tui` 子命令
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
use std::time::Duration;

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
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Tabs, Wrap},
    Terminal,
};

use crate::proot::distro::{builtin_specs, DistroManager, DistroSpec, InstalledDistro};
use crate::proot::links;
use crate::proot::recipe::RecipeIndex;
use crate::proot::settings::{self, Settings};

const TAB_TITLES: [&str; 6] = ["发行版", "短链", "配方", "日志", "向导", "账号"];
const TAB_DISTROS: usize = 0;
const TAB_LINKS: usize = 1;
const TAB_RECIPES: usize = 2;
const TAB_LOGS: usize = 3;
const TAB_WIZARD: usize = 4;
const TAB_ACCOUNT: usize = 5;

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
    /// 设备绑定状态（运行时从落盘的 device_token 推断；二维码绑定走 `moxsh-proot login`）。
    account_bound: bool,
    /// 绑定后用于展示的 device_token 掩码（前 8 位 + …）。
    account_mask: String,
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
        match self.mgr.login_cmd(id, Some("/bin/sh")) {
            Ok(cmd) => {
                // 退出 TUI 进入容器；容器退出后自动恢复 TUI。
                throw_to_shell(&cmd);
            }
            Err(e) => self.status = format!("生成登录命令失败: {}", e),
        }
    }

    fn install(&mut self, id: &str) {
        let spec = match DistroManager::find_spec(id) {
            Ok(s) => s,
            Err(e) => {
                self.status = format!("未找到发行版 `{}`: {}", id, e);
                return;
            }
        };
        self.status = format!("正在安装 {} …（下载 + 校验 + 解压）", spec.name);
        let cb: &mut dyn FnMut(&str, u64, u64) = &mut |stage: &str, _done, _total| {
            // 安装为同步阻塞，进度文本随下一帧重绘显示（实时进度条留待后台线程优化）。
            let _ = stage;
        };
        match self.mgr.install(&spec, None, cb) {
            Ok(()) => {
                self.status = format!("✅ {} 安装完成", spec.name);
                self.refresh();
            }
            Err(e) => self.status = format!("❌ 安装失败: {}", e),
        }
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
        let wizard = "moxsh-proot-distro wizard".to_string();
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
            eprintln!("moxsh-proot-tui: 终端初始化失败: {}", e);
            return 2;
        }
    };

    if enter_altscreen(&mut terminal).is_err() {
        let _ = restore_altscreen(&mut terminal);
        eprintln!("moxsh-proot-tui: 无法进入备用屏幕（非交互终端？）");
        return 2;
    }

    let app = match App::new(&root) {
        Ok(a) => a,
        Err(e) => {
            let _ = restore_altscreen(&mut terminal);
            eprintln!("moxsh-proot-tui: 初始化失败: {}", e);
            return 2;
        }
    };

    let res = run_app(&mut terminal, app);
    let _ = restore_altscreen(&mut terminal);
    match res {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("moxsh-proot-tui: 运行错误: {}", e);
            1
        }
    }
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    mut app: App,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        terminal.draw(|f| ui(f, &app))?;
        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
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
                    KeyCode::Char('r') => {
                        if app.tab == TAB_DISTROS {
                            app.remove();
                        }
                    }
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
            "moxsh-proot ",
            Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
        ),
        Span::styled("TUI 控制台", Style::default().fg(Color::Cyan)),
    ]);
    let tabs = Tabs::new(TAB_TITLES.iter().map(|t| Line::from(*t)).collect::<Vec<_>>())
        .block(Block::default().borders(Borders::ALL).title(title))
        .select(app.tab)
        .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
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

    let help = "↑↓ 选择 · Enter 登录/安装 · r 删除 · b 备份 · Tab 切换 · q 退出";
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
}

fn render_distros(f: &mut ratatui::Frame, area: Rect, app: &App) {
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
        "安装向导：一次性初始化数据根、安装首个发行版、落启动器与控制台。\n\n按 Enter 运行 `moxsh-proot-distro wizard`。",
    )
    .block(Block::default().borders(Borders::ALL).title("安装向导"))
    .alignment(Alignment::Left)
    .wrap(Wrap { trim: true });
    f.render_widget(p, area);
}

/// 账号页（0.4.0 路线图「账号」tab）：展示设备绑定状态，按 Enter 触发扫码绑定。
///
/// 安全说明：本页只展示绑定*状态*与 token 掩码，**不在此渲染二维码**（扫码由终端
/// 原生 `run_login_flow` 处理，能获得完整半块字符图、不被 TUI 盒子裁切）。设备码
/// 授权走 RFC 8628，后端 `/activate` 复用既有 GitHub / Gitee 双 OAuth。
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
            "按 Enter 开始设备码扫码绑定：",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from("  1. 终端临时退出 TUI，打印半块字符二维码 + 验证码"));
        lines.push(Line::from("  2. 手机扫码（或访问提示链接），用 GitHub / Gitee 登录并确认"));
        lines.push(Line::from("  3. 确认后终端自动轮询拿到 device_token 并落盘，回到本页即显示「已绑定」"));
    }

    let p = Paragraph::new(Text::from(lines))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("账号（设备绑定 · 扫码登录）")
                .border_style(Style::default().fg(state_color)),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(p, area);
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

    // 执行目标逻辑（如 moxsh-proot 进容器 / 设备码绑定）；退出后回到这里。
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
        let tmp = std::env::temp_dir().join(format!("moxsh_tui_test_{}", std::process::id()));
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
}
