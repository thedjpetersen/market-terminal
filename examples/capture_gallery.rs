use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use ab_glyph::{point, Font, FontArc, PxScale, ScaleFont};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use image::{Rgba, RgbaImage};
use market_terminal::{bootstrap, runtime, App};
use ratatui::{
    backend::TestBackend,
    buffer::Buffer,
    style::{Color, Modifier},
    Terminal,
};

const COLUMNS: u16 = 160;
const ROWS: u16 = 48;
const CELL_WIDTH: u32 = 9;
const CELL_HEIGHT: u32 = 19;
const PADDING: u32 = 24;
const FONT_SIZE: f32 = 15.0;
const BASE_BACKGROUND: Rgba<u8> = Rgba([2, 3, 3, 255]);
const BASE_FOREGROUND: Rgba<u8> = Rgba([222, 221, 215, 255]);
const ASCIINEMA_TOURS: &[&str] = &["screening", "backtesting", "options", "fixed-income"];

fn main() -> Result<(), Box<dyn Error>> {
    let output = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("docs/screenshots"));
    let cast_output = std::env::args()
        .nth(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("docs/asciinema"));
    fs::create_dir_all(&output)?;
    fs::create_dir_all(&cast_output)?;
    let font = load_font()?;

    capture(&output, &cast_output, &font, "overview", |_| {})?;
    capture(&output, &cast_output, &font, "monitor", |app| {
        command(app, "MON MACRO")
    })?;
    capture(&output, &cast_output, &font, "desk", |app| {
        command(app, "DESK LAYOUT 60 65")
    })?;
    capture(&output, &cast_output, &font, "charting", |app| {
        command(app, "CHART MSFT COMPARE SPY,QQQ 6M SMA20 NORMALIZE");
        app.handle_key(key(KeyCode::Char(',')));
        app.handle_key(key(KeyCode::Char(',')));
    })?;
    capture(&output, &cast_output, &font, "chat", |app| {
        command(app, "CHAT")
    })?;
    capture(&output, &cast_output, &font, "spreadsheet", |app| {
        command(app, "SHEET");
        app.handle_key(key(KeyCode::Right));
        for _ in 0..11 {
            app.handle_key(key(KeyCode::Down));
        }
    })?;
    capture(&output, &cast_output, &font, "alerts", |app| {
        command(app, "ALERTS");
        app.handle_key(key(KeyCode::Char('r')));
        app.handle_key(key(KeyCode::Char('r')));
    })?;
    capture(&output, &cast_output, &font, "risk", |app| {
        command(app, "RISK")
    })?;
    capture(&output, &cast_output, &font, "assistant", |app| {
        command(app, "AI");
        app.handle_key(key(KeyCode::Enter));
        type_text(app, "Bring the monitor forward and compare AAPL with SPY");
    })?;
    capture(&output, &cast_output, &font, "launchpad", |app| {
        command(app, "LAUNCH")
    })?;
    capture(&output, &cast_output, &font, "discovery", |app| {
        command(app, "DISCOVER portfolio")
    })?;
    capture(&output, &cast_output, &font, "find", |app| {
        command(app, "FIND US")
    })?;
    capture(&output, &cast_output, &font, "security", |app| {
        command(app, "AAPL US")
    })?;
    capture(&output, &cast_output, &font, "news", |app| {
        command(app, "NEWS")
    })?;
    capture(&output, &cast_output, &font, "news-reader", |app| {
        command(app, "NEWS");
        app.handle_key(key(KeyCode::Enter));
    })?;
    capture(&output, &cast_output, &font, "screening", |app| {
        command(app, "SCREEN momentum")
    })?;
    capture(&output, &cast_output, &font, "backtesting", |app| {
        command(app, "BACKTEST AAPL FAST 10 SLOW 50 COST 5 COMMISSION 1.00");
        settle(app, 100);
    })?;
    capture(&output, &cast_output, &font, "options", |app| {
        command(app, "OPTIONS AAPL CALL 190 200 30 25 5 0 100")
    })?;
    capture(&output, &cast_output, &font, "fixed-income", |app| {
        command(app, "BOND UST-5Y-REFERENCE USD 100 4.5 4.25 5 SEMI 0")
    })?;
    Ok(())
}

fn capture(
    output: &Path,
    cast_output: &Path,
    font: &FontArc,
    name: &str,
    prepare: impl FnOnce(&mut App),
) -> Result<(), Box<dyn Error>> {
    let mut app = bootstrap::demo_app();
    prepare(&mut app);
    app.advance_tick();

    let backend = TestBackend::new(COLUMNS, ROWS);
    let mut terminal = Terminal::new(backend)?;
    terminal.draw(|frame| runtime::render(frame, &app))?;
    let path = output.join(format!("{name}.png"));
    render_buffer(terminal.backend().buffer(), font).save(&path)?;
    println!("captured {}", path.display());
    if ASCIINEMA_TOURS.contains(&name) {
        let cast_path = cast_output.join(format!("{name}.cast"));
        write_cast(&cast_path, name, terminal.backend().buffer())?;
        println!("captured {}", cast_path.display());
    }
    Ok(())
}

fn write_cast(path: &Path, name: &str, buffer: &Buffer) -> Result<(), Box<dyn Error>> {
    let header = serde_json::json!({
        "version": 2,
        "width": COLUMNS,
        "height": ROWS,
        "timestamp": 0,
        "env": {"SHELL": "/bin/sh", "TERM": "xterm-256color"},
        "title": format!("market-terminal {name} tour")
    });
    let intro = format!(
        "\u{1b}[2J\u{1b}[Hmarket-terminal :: {}\r\n",
        name.replace('-', " ")
    );
    let frame = format!("\u{1b}[2J\u{1b}[H{}", buffer_to_ansi(buffer));
    let events = [
        serde_json::json!([0.0, "o", intro]),
        serde_json::json!([0.7, "o", frame]),
    ];
    let mut cast = serde_json::to_string(&header)?;
    for event in events {
        cast.push('\n');
        cast.push_str(&serde_json::to_string(&event)?);
    }
    cast.push('\n');
    fs::write(path, cast)?;
    Ok(())
}

fn buffer_to_ansi(buffer: &Buffer) -> String {
    let mut output = String::new();
    let mut previous = None;
    for row in 0..buffer.area.height {
        for column in 0..buffer.area.width {
            let cell = &buffer[(column, row)];
            let foreground = terminal_color(cell.fg, BASE_FOREGROUND).0;
            let background = terminal_color(cell.bg, BASE_BACKGROUND).0;
            let style = (
                foreground,
                background,
                cell.modifier.contains(Modifier::BOLD),
            );
            if previous != Some(style) {
                let bold = if style.2 { 1 } else { 22 };
                output.push_str(&format!(
                    "\u{1b}[{bold};38;2;{};{};{};48;2;{};{};{}m",
                    style.0[0], style.0[1], style.0[2], style.1[0], style.1[1], style.1[2]
                ));
                previous = Some(style);
            }
            output.push_str(cell.symbol());
        }
        output.push_str("\u{1b}[0m\r\n");
        previous = None;
    }
    output.push_str("\u{1b}[0m");
    output
}

fn command(app: &mut App, value: &str) {
    app.handle_key(key(KeyCode::Char('/')));
    for character in value.chars() {
        app.handle_key(key(KeyCode::Char(character)));
    }
    app.handle_key(key(KeyCode::Enter));
}

fn type_text(app: &mut App, value: &str) {
    for character in value.chars() {
        app.handle_key(key(KeyCode::Char(character)));
    }
}

fn settle(app: &mut App, attempts: usize) {
    for _ in 0..attempts {
        thread::sleep(Duration::from_millis(1));
        app.advance_tick();
    }
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn load_font() -> Result<FontArc, Box<dyn Error>> {
    let configured = std::env::var_os("MARKET_TERMINAL_FONT").map(PathBuf::from);
    let candidates = configured.into_iter().chain([
        PathBuf::from("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"),
        PathBuf::from("/usr/share/fonts/truetype/liberation2/LiberationMono-Regular.ttf"),
    ]);
    for path in candidates {
        if let Ok(bytes) = fs::read(&path) {
            if let Ok(font) = FontArc::try_from_vec(bytes) {
                return Ok(font);
            }
        }
    }
    Err("set MARKET_TERMINAL_FONT to a monospaced TrueType font".into())
}

fn render_buffer(buffer: &Buffer, font: &FontArc) -> RgbaImage {
    let width = u32::from(buffer.area.width) * CELL_WIDTH + PADDING * 2;
    let height = u32::from(buffer.area.height) * CELL_HEIGHT + PADDING * 2;
    let mut image = RgbaImage::from_pixel(width, height, BASE_BACKGROUND);

    for (index, cell) in buffer.content.iter().enumerate() {
        let column = index as u32 % u32::from(buffer.area.width);
        let row = index as u32 / u32::from(buffer.area.width);
        let x = PADDING + column * CELL_WIDTH;
        let y = PADDING + row * CELL_HEIGHT;
        let background = terminal_color(cell.bg, BASE_BACKGROUND);
        fill_rect(&mut image, x, y, CELL_WIDTH, CELL_HEIGHT, background);
    }

    for (index, cell) in buffer.content.iter().enumerate() {
        if cell.symbol().trim().is_empty() {
            continue;
        }
        let column = index as u32 % u32::from(buffer.area.width);
        let row = index as u32 / u32::from(buffer.area.width);
        let x = PADDING + column * CELL_WIDTH;
        let y = PADDING + row * CELL_HEIGHT;
        let foreground = terminal_color(cell.fg, BASE_FOREGROUND);
        let bold = cell.modifier.contains(Modifier::BOLD);
        draw_symbol(&mut image, font, cell.symbol(), x, y, foreground, bold);
    }
    image
}

fn fill_rect(image: &mut RgbaImage, x: u32, y: u32, width: u32, height: u32, color: Rgba<u8>) {
    for pixel_y in y..y + height {
        for pixel_x in x..x + width {
            image.put_pixel(pixel_x, pixel_y, color);
        }
    }
}

fn draw_symbol(
    image: &mut RgbaImage,
    font: &FontArc,
    symbol: &str,
    x: u32,
    y: u32,
    color: Rgba<u8>,
    bold: bool,
) {
    if draw_braille(image, symbol, x, y, color) {
        return;
    }
    let scale = PxScale::from(FONT_SIZE);
    let scaled = font.as_scaled(scale);
    let baseline = y as f32 + (CELL_HEIGHT as f32 - scaled.height()) / 2.0 + scaled.ascent();
    let mut cursor = x as f32;
    for character in symbol.chars() {
        let glyph_id = scaled.glyph_id(character);
        let advance = scaled.h_advance(glyph_id);
        let glyph = glyph_id.with_scale_and_position(scale, point(cursor, baseline));
        if let Some(outlined) = font.outline_glyph(glyph) {
            draw_glyph(image, &outlined, color, 0);
            if bold {
                draw_glyph(image, &outlined, color, 1);
            }
        }
        cursor += advance;
    }
}

/// Rasterizes Unicode Braille directly so generated captures do not depend on
/// whether the selected text font happens to contain the Braille block.
fn draw_braille(image: &mut RgbaImage, symbol: &str, x: u32, y: u32, color: Rgba<u8>) -> bool {
    let mut characters = symbol.chars();
    let Some(character) = characters.next() else {
        return false;
    };
    if characters.next().is_some() || !(('\u{2800}'..='\u{28ff}').contains(&character)) {
        return false;
    }

    let pattern = character as u32 - 0x2800;
    let dots = [
        (0_u32, 0_u32, 0_u32),
        (1, 0, 1),
        (2, 0, 2),
        (3, 1, 0),
        (4, 1, 1),
        (5, 1, 2),
        (6, 0, 3),
        (7, 1, 3),
    ];
    for (bit, column, row) in dots {
        if pattern & (1 << bit) == 0 {
            continue;
        }
        let dot_x = x + 2 + column * 4;
        let dot_y = y + 2 + row * 4;
        fill_rect(image, dot_x, dot_y, 2, 2, color);
    }
    true
}

fn draw_glyph(
    image: &mut RgbaImage,
    glyph: &ab_glyph::OutlinedGlyph,
    color: Rgba<u8>,
    x_offset: i32,
) {
    let bounds = glyph.px_bounds();
    glyph.draw(|x, y, coverage| {
        let pixel_x = bounds.min.x.floor() as i32 + x as i32 + x_offset;
        let pixel_y = bounds.min.y.floor() as i32 + y as i32;
        if pixel_x < 0 || pixel_y < 0 {
            return;
        }
        let (pixel_x, pixel_y) = (pixel_x as u32, pixel_y as u32);
        if pixel_x >= image.width() || pixel_y >= image.height() {
            return;
        }
        blend(image.get_pixel_mut(pixel_x, pixel_y), color, coverage);
    });
}

fn blend(destination: &mut Rgba<u8>, source: Rgba<u8>, coverage: f32) {
    let alpha = coverage.clamp(0.0, 1.0);
    for channel in 0..3 {
        destination[channel] = (f32::from(source[channel]) * alpha
            + f32::from(destination[channel]) * (1.0 - alpha))
            .round() as u8;
    }
}

fn terminal_color(color: Color, fallback: Rgba<u8>) -> Rgba<u8> {
    let rgb = match color {
        Color::Reset => return fallback,
        Color::Black => [0, 0, 0],
        Color::Red => [205, 49, 49],
        Color::Green => [13, 188, 121],
        Color::Yellow => [229, 229, 16],
        Color::Blue => [36, 114, 200],
        Color::Magenta => [188, 63, 188],
        Color::Cyan => [17, 168, 205],
        Color::Gray => [204, 204, 204],
        Color::DarkGray => [102, 102, 102],
        Color::LightRed => [241, 76, 76],
        Color::LightGreen => [35, 209, 139],
        Color::LightYellow => [245, 245, 67],
        Color::LightBlue => [59, 142, 234],
        Color::LightMagenta => [214, 112, 214],
        Color::LightCyan => [41, 184, 219],
        Color::White => [242, 242, 242],
        Color::Rgb(red, green, blue) => [red, green, blue],
        Color::Indexed(index) => indexed_color(index),
    };
    Rgba([rgb[0], rgb[1], rgb[2], 255])
}

fn indexed_color(index: u8) -> [u8; 3] {
    if index < 16 {
        const ANSI: [[u8; 3]; 16] = [
            [0, 0, 0],
            [205, 49, 49],
            [13, 188, 121],
            [229, 229, 16],
            [36, 114, 200],
            [188, 63, 188],
            [17, 168, 205],
            [229, 229, 229],
            [102, 102, 102],
            [241, 76, 76],
            [35, 209, 139],
            [245, 245, 67],
            [59, 142, 234],
            [214, 112, 214],
            [41, 184, 219],
            [255, 255, 255],
        ];
        return ANSI[usize::from(index)];
    }
    if index < 232 {
        let value = index - 16;
        let component = |part: u8| if part == 0 { 0 } else { 55 + part * 40 };
        return [
            component(value / 36),
            component((value % 36) / 6),
            component(value % 6),
        ];
    }
    let gray = 8 + (index - 232) * 10;
    [gray, gray, gray]
}
