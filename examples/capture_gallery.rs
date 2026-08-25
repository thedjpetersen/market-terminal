use std::{error::Error, fs, path::{Path, PathBuf}};

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

fn main() -> Result<(), Box<dyn Error>> {
    let output = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("docs/screenshots"));
    fs::create_dir_all(&output)?;
    let font = load_font()?;

    capture(&output, &font, "overview", |_| {})?;
    capture(&output, &font, "monitor", |app| command(app, "MON MACRO"))?;
    capture(&output, &font, "charting", |app| {
        command(app, "CHART MSFT COMPARE SPY,QQQ 6M SMA20 NORMALIZE");
        app.handle_key(key(KeyCode::Char(',')));
        app.handle_key(key(KeyCode::Char(',')));
    })?;
    capture(&output, &font, "chat", |app| command(app, "CHAT"))?;
    capture(&output, &font, "spreadsheet", |app| command(app, "SHEET"))?;
    capture(&output, &font, "alerts", |app| {
        command(app, "ALERTS");
        app.handle_key(key(KeyCode::Char('r')));
        app.handle_key(key(KeyCode::Char('r')));
    })?;
    capture(&output, &font, "assistant", |app| {
        command(app, "AI");
        type_text(app, "Bring the monitor forward and compare AAPL with SPY");
    })?;
    capture(&output, &font, "find", |app| command(app, "FIND US"))?;
    capture(&output, &font, "security", |app| command(app, "AAPL US"))?;
    Ok(())
}

fn capture(
    output: &Path,
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
    Ok(())
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
            [0, 0, 0], [205, 49, 49], [13, 188, 121], [229, 229, 16],
            [36, 114, 200], [188, 63, 188], [17, 168, 205], [229, 229, 229],
            [102, 102, 102], [241, 76, 76], [35, 209, 139], [245, 245, 67],
            [59, 142, 234], [214, 112, 214], [41, 184, 219], [255, 255, 255],
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
