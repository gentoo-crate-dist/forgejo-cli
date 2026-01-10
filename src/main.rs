use std::fmt::Display;
use std::io::IsTerminal;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use eyre::{eyre, Context};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

mod keys;
use keys::*;

mod actions;
mod auth;
mod completion;
mod config;
mod issues;
mod org;
mod projects;
mod prs;
mod release;
mod repo;
mod tag;
mod user;
mod version;
mod whoami;
mod wiki;

pub const USER_AGENT: &str = concat!(
    env!("CARGO_PKG_NAME"),
    "/",
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("CARGO_PKG_REPOSITORY"),
    ")"
);

#[derive(Parser, Debug)]
pub struct App {
    #[clap(long, short = 'H')]
    host: Option<String>,
    #[clap(long)]
    style: Option<Style>,
    #[clap(subcommand)]
    command: Command,
}

#[derive(Subcommand, Clone, Debug)]
pub enum Command {
    #[clap(subcommand)]
    Repo(repo::RepoCommand),
    Issue(issues::IssueCommand),
    Pr(prs::PrCommand),
    Project(projects::ProjectCommand),
    Config(config::ConfigCommand),
    Wiki(wiki::WikiCommand),
    Actions(actions::ActionsCommand),
    #[command(name = "whoami")]
    WhoAmI(whoami::WhoAmICommand),
    #[clap(subcommand)]
    Auth(auth::AuthCommand),
    Release(release::ReleaseCommand),
    Tag(tag::TagCommand),
    User(user::UserCommand),
    Org(org::OrgCommand),
    Version(version::VersionCommand),
    Completion(completion::CompletionCommand),
}

impl Command {
    pub async fn run(self, keys: &mut crate::KeyInfo, host_name: Option<&str>) -> eyre::Result<()> {
        match self {
            Command::Repo(subcommand) => subcommand.run(keys, host_name).await?,
            Command::Issue(subcommand) => subcommand.run(keys, host_name).await?,
            Command::Pr(subcommand) => subcommand.run(keys, host_name).await?,
            Command::Project(subcommand) => subcommand.run(keys, host_name).await?,
            Command::Config(subcommand) => subcommand.run().await?,
            Command::Wiki(subcommand) => subcommand.run(keys, host_name).await?,
            Command::Actions(subcommand) => subcommand.run(keys, host_name).await?,
            Command::WhoAmI(command) => command.run(keys, host_name).await?,
            Command::Auth(subcommand) => subcommand.run(keys, host_name).await?,
            Command::Release(subcommand) => subcommand.run(keys, host_name).await?,
            Command::Tag(subcommand) => subcommand.run(keys, host_name).await?,
            Command::User(subcommand) => subcommand.run(keys, host_name).await?,
            Command::Org(subcommand) => subcommand.run(keys, host_name).await?,
            Command::Version(command) => command.run().await?,
            Command::Completion(subcommand) => subcommand.run(),
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let args = App::parse();

    let _ = SPECIAL_RENDER.set(SpecialRender::new(args.style.unwrap_or_default()));

    let mut keys = KeyInfo::load().await?;
    let r = args.command.run(&mut keys, args.host.as_deref()).await;
    keys.save().await?;
    r?;

    Ok(())
}

async fn readline(msg: &str) -> eyre::Result<String> {
    use std::io::Write;
    print!("{msg}");
    std::io::stdout().flush()?;
    tokio::task::spawn_blocking(|| {
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        Ok(input)
    })
    .await?
}

async fn prompt_bool(msg: &str, default_answer: bool) -> eyre::Result<bool> {
    let msg = if default_answer {
        format!("{msg} [Y/n]: ")
    } else {
        format!("{msg} [y/N]: ")
    };

    loop {
        let input = readline(&msg).await?;
        let input = input.trim();

        if input.is_empty() {
            return Ok(default_answer);
        }

        if input.eq_ignore_ascii_case("y") {
            return Ok(true);
        }
        if input.eq_ignore_ascii_case("n") {
            return Ok(false);
        }
    }
}

async fn editor(contents: &mut String, ext: Option<&str>) -> eyre::Result<()> {
    let editor = std::path::PathBuf::from(
        std::env::var_os("EDITOR").ok_or_else(|| eyre!("unable to locate editor"))?,
    );

    let (mut file, path) = tempfile(ext).await?;
    file.write_all(contents.as_bytes()).await?;
    drop(file);

    // Async block acting as a try/catch block so that the temp file is deleted even
    // on errors
    let res = async {
        eprint!("waiting on editor\r");
        let flags = get_editor_flags(&editor);
        let status = tokio::process::Command::new(editor)
            .args(flags)
            .arg(&path)
            .status()
            .await?;
        if !status.success() {
            eyre::bail!("editor exited unsuccessfully");
        }

        *contents = tokio::fs::read_to_string(&path).await?;
        eprint!("                 \r");

        Ok(())
    }
    .await;

    tokio::fs::remove_file(path).await?;
    res?;
    Ok(())
}

// Read a filename, unless “-” is given, in which case, stdin is read and returned
async fn read_file_or_stdin(path: &PathBuf) -> eyre::Result<String> {
    if *path == PathBuf::from("-") {
        // Typical use should be not interactive, so it's fine to call stdin() (see docs)
        let mut stdin = tokio::io::stdin();
        let mut body = String::new();
        stdin.read_to_string(&mut body).await?;
        Ok(body)
    } else {
        tokio::fs::read_to_string(&path)
            .await
            .wrap_err_with(|| eyre::eyre!("Error reading file `{}`", path.to_string_lossy()))
    }
}

fn get_editor_flags(editor_path: &std::path::Path) -> &'static [&'static str] {
    let editor_name = match editor_path.file_stem().and_then(|s| s.to_str()) {
        Some(name) => name,
        None => return &[],
    };
    if editor_name == "code" {
        return &["--wait"];
    }
    &[]
}

async fn tempfile(ext: Option<&str>) -> tokio::io::Result<(tokio::fs::File, std::path::PathBuf)> {
    let filename = uuid::Uuid::new_v4();
    let mut path = std::env::temp_dir().join(filename.to_string());
    if let Some(ext) = ext {
        path.set_extension(ext);
    }
    let file = tokio::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&path)
        .await?;
    Ok((file, path))
}

fn ssh_url_parse(s: &str) -> Result<url::Url, url::ParseError> {
    url::Url::parse(s).or_else(|_| {
        let mut new_s = String::new();
        new_s.push_str("ssh://");

        let auth_end = s.find("@").unwrap_or(0);
        new_s.push_str(&s[..auth_end]);
        new_s.push_str(&s[auth_end..].replacen(":", "/", 1));
        url::Url::parse(&new_s)
    })
}

fn host_name(url: &url::Url) -> &str {
    let name = &url[url::Position::BeforeHost..url::Position::AfterPath];
    name.strip_suffix("/").unwrap_or(name)
}

fn repo_url_host_name(url: &url::Url) -> &str {
    let host = host_name(url);
    host.rsplitn(3, '/').last().unwrap_or(host)
}

use std::sync::OnceLock;
static SPECIAL_RENDER: OnceLock<SpecialRender> = OnceLock::new();

fn special_render() -> &'static SpecialRender {
    SPECIAL_RENDER
        .get()
        .expect("attempted to get special characters before that was initialized")
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, Default)]
enum Style {
    /// Use special characters, and colors.
    #[default]
    Fancy,
    /// No special characters and no colors. Always used in non-terminal contexts (i.e. pipes)
    Minimal,
}

struct SpecialRender {
    fancy: bool,

    dash: char,
    bullet: char,
    body_prefix: char,
    horiz_rule: char,

    // Uncomment these as needed
    // red: &'static str,
    bright_red: &'static str,
    // green: &'static str,
    bright_green: &'static str,
    // blue: &'static str,
    bright_blue: &'static str,
    // cyan: &'static str,
    bright_cyan: &'static str,
    yellow: &'static str,
    bright_yellow: &'static str,
    // magenta: &'static str,
    bright_magenta: &'static str,
    black: &'static str,
    dark_grey: &'static str,
    light_grey: &'static str,
    white: &'static str,
    no_fg: &'static str,
    reset: &'static str,

    dark_grey_bg: &'static str,
    // no_bg: &'static str,
    hide_cursor: &'static str,
    show_cursor: &'static str,
    clear_line: &'static str,

    italic: &'static str,
    bold: &'static str,
    strike: &'static str,
    no_italic_bold: &'static str,
    no_strike: &'static str,
}

impl SpecialRender {
    fn new(display: Style) -> Self {
        let is_tty = std::io::stdout().is_terminal();
        match display {
            _ if !is_tty => Self::minimal(),
            Style::Fancy => Self::fancy(),
            Style::Minimal => Self::minimal(),
        }
    }

    fn fancy() -> Self {
        Self {
            fancy: true,

            dash: '—',
            bullet: '•',
            body_prefix: '▌',
            horiz_rule: '─',

            // red: "\x1b[31m",
            bright_red: "\x1b[91m",
            // green: "\x1b[32m",
            bright_green: "\x1b[92m",
            // blue: "\x1b[34m",
            bright_blue: "\x1b[94m",
            // cyan: "\x1b[36m",
            bright_cyan: "\x1b[96m",
            yellow: "\x1b[33m",
            bright_yellow: "\x1b[93m",
            // magenta: "\x1b[35m",
            bright_magenta: "\x1b[95m",
            black: "\x1b[30m",
            dark_grey: "\x1b[90m",
            light_grey: "\x1b[37m",
            white: "\x1b[97m",
            no_fg: "\x1b[39m",
            reset: "\x1b[0m",

            dark_grey_bg: "\x1b[100m",
            // no_bg: "\x1b[49",
            hide_cursor: "\x1b[?25l",
            show_cursor: "\x1b[?25h",
            clear_line: "\x1b[2K",

            italic: "\x1b[3m",
            bold: "\x1b[1m",
            strike: "\x1b[9m",
            no_italic_bold: "\x1b[23m",
            no_strike: "\x1b[29m",
        }
    }

    fn minimal() -> Self {
        Self {
            fancy: false,

            dash: '-',
            bullet: '-',
            body_prefix: '>',
            horiz_rule: '-',

            // red: "",
            bright_red: "",
            // green: "",
            bright_green: "",
            // blue: "",
            bright_blue: "",
            // cyan: "",
            bright_cyan: "",
            yellow: "",
            bright_yellow: "",
            // magenta: "",
            bright_magenta: "",
            black: "",
            dark_grey: "",
            light_grey: "",
            white: "",
            no_fg: "",
            reset: "",

            dark_grey_bg: "",
            // no_bg: "",
            hide_cursor: "",
            show_cursor: "",
            clear_line: "",

            italic: "",
            bold: "",
            strike: "~~",
            no_italic_bold: "",
            no_strike: "~~",
        }
    }
}

fn max_line_length() -> usize {
    let (terminal_width, _) = crossterm::terminal::size().unwrap_or((80, 24));
    (terminal_width as usize - 2).min(80)
}

fn render_text(text: &str) -> String {
    let mut ansi_printer = AnsiPrinter::new(max_line_length());

    ansi_printer.pause_style();
    ansi_printer.prefix();
    ansi_printer.resume_style();
    ansi_printer.text(text);
    ansi_printer.out
}

fn markdown(text: &str) -> String {
    let SpecialRender {
        fancy,

        bullet,
        horiz_rule,
        bright_blue,
        dark_grey_bg,
        body_prefix,
        ..
    } = *special_render();

    if !fancy {
        let mut out = String::new();
        for line in text.lines() {
            use std::fmt::Write;
            let _ = writeln!(&mut out, "{body_prefix} {line}");
        }
        return out;
    }

    let arena = comrak::Arena::new();
    let mut options = comrak::Options::default();
    options.extension.strikethrough = true;
    let root = comrak::parse_document(&arena, text, &options);

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Side {
        Start,
        End,
    }

    let mut explore_stack = Vec::new();
    let mut render_queue = Vec::new();

    explore_stack.extend(root.reverse_children().map(|x| (x, Side::Start)));
    while let Some((node, side)) = explore_stack.pop() {
        if side == Side::Start {
            explore_stack.push((node, Side::End));
            explore_stack.extend(node.reverse_children().map(|x| (x, Side::Start)));
        }
        render_queue.push((node, side));
    }

    let mut list_numbers = Vec::new();

    let max_line_len = max_line_length();

    let mut links = Vec::new();

    let mut ansi_printer = AnsiPrinter::new(max_line_len);
    ansi_printer.pause_style();
    ansi_printer.prefix();
    ansi_printer.resume_style();
    let mut iter = render_queue.into_iter().peekable();
    while let Some((item, side)) = iter.next() {
        use comrak::nodes::NodeValue;
        use Side::*;
        match (&item.data.borrow().value, side) {
            (NodeValue::Paragraph, Start) => (),
            (NodeValue::Paragraph, End) => {
                if iter.peek().is_some_and(|(_, side)| *side == Start) {
                    ansi_printer.newline();
                    ansi_printer.newline();
                }
            }
            (NodeValue::Text(s), Start) => ansi_printer.text(s),
            (NodeValue::Link(_), Start) => {
                ansi_printer.start_fg(bright_blue);
            }
            (NodeValue::Link(link), End) => {
                use std::fmt::Write;
                ansi_printer.stop_fg();
                links.push(link.url.clone());
                let _ = write!(&mut ansi_printer, "({})", links.len());
            }
            (NodeValue::Image(_), Start) => {
                ansi_printer.start_fg(bright_blue);
            }
            (NodeValue::Image(link), End) => {
                use std::fmt::Write;
                ansi_printer.stop_fg();
                links.push(link.url.clone());
                let _ = write!(&mut ansi_printer, "({})", links.len());
            }
            (NodeValue::Code(code), Start) => {
                ansi_printer.pause_style();
                ansi_printer.start_bg(dark_grey_bg);
                ansi_printer.text(&code.literal);
                ansi_printer.resume_style();
            }
            (NodeValue::CodeBlock(code), Start) => {
                if ansi_printer.cur_line_len != 0 {
                    ansi_printer.newline();
                }
                ansi_printer.pause_style();
                ansi_printer.start_bg(dark_grey_bg);
                ansi_printer.text(&code.literal);
                ansi_printer.newline();
                ansi_printer.resume_style();
                ansi_printer.newline();
            }
            (NodeValue::BlockQuote, Start) => {
                ansi_printer.blockquote_depth += 1;
                ansi_printer.pause_style();
                ansi_printer.prefix();
                ansi_printer.resume_style();
            }
            (NodeValue::BlockQuote, End) => {
                ansi_printer.blockquote_depth -= 1;
                ansi_printer.newline();
            }
            (NodeValue::HtmlInline(html), Start) => {
                ansi_printer.pause_style();
                ansi_printer.text(html);
                ansi_printer.resume_style();
            }
            (NodeValue::HtmlBlock(html), Start) => {
                if ansi_printer.cur_line_len != 0 {
                    ansi_printer.newline();
                }
                ansi_printer.pause_style();
                ansi_printer.text(&html.literal);
                ansi_printer.newline();
                ansi_printer.resume_style();
            }

            (NodeValue::Heading(heading), Start) => {
                ansi_printer.reset();
                ansi_printer.start_bold();
                ansi_printer
                    .out
                    .extend(std::iter::repeat('#').take(heading.level as usize));
                ansi_printer.out.push(' ');
                ansi_printer.cur_line_len += heading.level as usize + 1;
            }
            (NodeValue::Heading(_), End) => {
                ansi_printer.reset();
                ansi_printer.newline();
                ansi_printer.newline();
            }

            (NodeValue::List(list), Start) => {
                if list.list_type == comrak::nodes::ListType::Ordered {
                    list_numbers.push(0);
                }
            }
            (NodeValue::List(list), End) => {
                if list.list_type == comrak::nodes::ListType::Ordered {
                    list_numbers.pop();
                }
                ansi_printer.newline();
            }
            (NodeValue::Item(list), Start) => {
                if list.list_type == comrak::nodes::ListType::Ordered {
                    use std::fmt::Write;
                    let number: usize = if let Some(number) = list_numbers.last_mut() {
                        *number += 1;
                        *number
                    } else {
                        0
                    };
                    let _ = write!(&mut ansi_printer, "{number}. ");
                } else {
                    ansi_printer.out.push(bullet);
                    ansi_printer.out.push(' ');
                    ansi_printer.cur_line_len += 2;
                }
            }
            (NodeValue::Item(_), End) => {
                ansi_printer.newline();
            }

            (NodeValue::LineBreak, Start) => ansi_printer.newline(),
            (NodeValue::SoftBreak, Start) => ansi_printer.newline(),
            (NodeValue::ThematicBreak, Start) => {
                if ansi_printer.cur_line_len != 0 {
                    ansi_printer.newline();
                }
                ansi_printer
                    .out
                    .extend(std::iter::repeat(horiz_rule).take(max_line_len));
                ansi_printer.newline();
                ansi_printer.newline();
            }

            (NodeValue::Emph, Start) => ansi_printer.start_italic(),
            (NodeValue::Emph, End) => ansi_printer.stop_italic(),
            (NodeValue::Strong, Start) => ansi_printer.start_bold(),
            (NodeValue::Strong, End) => ansi_printer.stop_bold(),
            (NodeValue::Strikethrough, Start) => ansi_printer.start_strike(),
            (NodeValue::Strikethrough, End) => ansi_printer.stop_strike(),

            (NodeValue::Escaped, Start) => (),
            (_, End) => (),
            (_, Start) => ansi_printer.text("?TODO?"),
        }
    }
    if !links.is_empty() {
        ansi_printer.out.push('\n');
        for (i, url) in links.into_iter().enumerate() {
            use std::fmt::Write;
            let _ = writeln!(&mut ansi_printer.out, "({}. {url} )", i + 1);
        }
    }

    ansi_printer.out
}

#[derive(Default)]
struct RenderStyling {
    bold: bool,
    italic: bool,
    strike: bool,

    fg: Option<&'static str>,
    bg: Option<&'static str>,
}

struct AnsiPrinter {
    special_render: &'static SpecialRender,

    out: String,

    cur_line_len: usize,
    max_line_len: usize,

    blockquote_depth: usize,

    style_frames: Vec<RenderStyling>,
}

impl AnsiPrinter {
    fn new(max_line_len: usize) -> Self {
        Self {
            special_render: special_render(),

            out: String::new(),

            cur_line_len: 0,
            max_line_len,

            blockquote_depth: 0,

            style_frames: vec![RenderStyling::default()],
        }
    }

    fn text(&mut self, text: &str) {
        let mut iter = text.lines().peekable();
        while let Some(mut line) = iter.next() {
            loop {
                let this_len = line.chars().count();
                if self.cur_line_len + this_len > self.max_line_len {
                    let mut split_at = self.max_line_len - self.cur_line_len;
                    loop {
                        if line.is_char_boundary(split_at) {
                            break;
                        }
                        split_at -= 1;
                    }
                    let split_at = line
                        .split_at(split_at)
                        .0
                        .char_indices()
                        .rev()
                        .find(|(_, c)| c.is_whitespace())
                        .map(|(i, _)| i)
                        .unwrap_or(split_at);
                    let (head, tail) = line.split_at(split_at);
                    self.out.push_str(head);
                    self.cur_line_len += split_at;
                    self.newline();
                    line = tail.trim_start();
                } else {
                    self.out.push_str(line);
                    self.cur_line_len += this_len;
                    break;
                }
            }
            if iter.peek().is_some() {
                self.newline();
            }
        }
    }

    // Uncomment if needed
    // fn current_fg(&self) -> Option<&'static str> {
    //     self.current_style().fg
    // }

    fn start_fg(&mut self, color: &'static str) {
        self.current_style_mut().fg = Some(color);
        self.out.push_str(color);
    }

    fn stop_fg(&mut self) {
        self.current_style_mut().fg = None;
        self.out.push_str(self.special_render.no_fg);
    }

    fn current_bg(&self) -> Option<&'static str> {
        self.current_style().bg
    }

    fn start_bg(&mut self, color: &'static str) {
        self.current_style_mut().bg = Some(color);
        self.out.push_str(color);
    }

    // Uncomment if needed
    // fn stop_bg(&mut self) {
    //     self.current_style_mut().bg = None;
    //     self.out.push_str(self.special_render.no_bg);
    // }

    fn is_bold(&self) -> bool {
        self.current_style().bold
    }

    fn start_bold(&mut self) {
        self.current_style_mut().bold = true;
        self.out.push_str(self.special_render.bold);
    }

    fn stop_bold(&mut self) {
        self.current_style_mut().bold = false;
        self.out.push_str(self.special_render.reset);
        if self.is_italic() {
            self.out.push_str(self.special_render.italic);
        }
        if self.is_strike() {
            self.out.push_str(self.special_render.strike);
        }
    }

    fn is_italic(&self) -> bool {
        self.current_style().italic
    }

    fn start_italic(&mut self) {
        self.current_style_mut().italic = true;
        self.out.push_str(self.special_render.italic);
    }

    fn stop_italic(&mut self) {
        self.current_style_mut().italic = false;
        self.out.push_str(self.special_render.no_italic_bold);
        if self.is_bold() {
            self.out.push_str(self.special_render.bold);
        }
    }

    fn is_strike(&self) -> bool {
        self.current_style().strike
    }

    fn start_strike(&mut self) {
        self.current_style_mut().strike = true;
        self.out.push_str(self.special_render.strike);
    }

    fn stop_strike(&mut self) {
        self.current_style_mut().strike = false;
        self.out.push_str(self.special_render.no_strike);
    }

    fn reset(&mut self) {
        *self.current_style_mut() = RenderStyling::default();
        self.out.push_str(self.special_render.reset);
    }

    fn pause_style(&mut self) {
        self.out.push_str(self.special_render.reset);
        self.style_frames.push(RenderStyling::default());
    }

    fn resume_style(&mut self) {
        self.out.push_str(self.special_render.reset);
        self.style_frames.pop();
        if let Some(bg) = self.current_bg() {
            self.out.push_str(bg);
        }
        if self.is_bold() {
            self.out.push_str(self.special_render.bold);
        }
        if self.is_italic() {
            self.out.push_str(self.special_render.italic);
        }
        if self.is_strike() {
            self.out.push_str(self.special_render.strike);
        }
    }

    fn newline(&mut self) {
        if self.current_bg().is_some() {
            self.out
                .extend(std::iter::repeat(' ').take(self.max_line_len - self.cur_line_len));
        }
        self.pause_style();
        self.out.push('\n');
        self.prefix();
        for _ in 0..self.blockquote_depth {
            self.prefix();
        }
        self.resume_style();
        self.cur_line_len = self.blockquote_depth * 2;
    }

    fn prefix(&mut self) {
        self.out.push_str(self.special_render.dark_grey);
        self.out.push(self.special_render.body_prefix);
        self.out.push(' ');
    }

    fn current_style(&self) -> &RenderStyling {
        self.style_frames.last().expect("Ran out of style frames")
    }

    fn current_style_mut(&mut self) -> &mut RenderStyling {
        self.style_frames
            .last_mut()
            .expect("Ran out of style frames")
    }
}

impl std::fmt::Write for AnsiPrinter {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.text(s);
        Ok(())
    }
}

/// When formatted, display either the inner value if `Some`, or the fallback if `None`.
struct DisplayOptional<T: Display, F: Display>(Option<T>, F);

impl<T: Display, F: Display> Display for DisplayOptional<T, F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(x) = &self.0 {
            write!(f, "{x}")
        } else {
            write!(f, "{}", self.1)
        }
    }
}

/// When formatted, show the boolean with coloring.
struct DisplayBool(bool);

impl Display for DisplayBool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let SpecialRender {
            bright_red,
            bright_green,
            reset,
            ..
        } = *special_render();

        if self.0 {
            write!(f, "{bright_green}true{reset}")?;
        } else {
            write!(f, "{bright_red}false{reset}")?;
        }

        Ok(())
    }
}
