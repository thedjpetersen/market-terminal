use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    symbols,
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Cell, Chart, Dataset, GraphType, List, ListItem, Paragraph, Row, Table, Wrap},
    Frame,
};

use crate::{
    app::{App, Screen},
    data::{AAPL, CURVE, HEADLINES, MARKETS, PERIODS, POSITIONS, RETURNS_A, RETURNS_B},
};

const BG: Color = Color::Rgb(2, 3, 3);
const INK: Color = Color::Rgb(222, 221, 215);
const MUTED: Color = Color::Rgb(124, 128, 128);
const AMBER: Color = Color::Rgb(242, 173, 55);
const YELLOW: Color = Color::Rgb(226, 217, 103);
const CYAN: Color = Color::Rgb(99, 212, 237);
const GREEN: Color = Color::Rgb(158, 229, 79);
const RED: Color = Color::Rgb(241, 69, 112);

pub fn render(frame: &mut Frame, app: &App) {
    frame.render_widget(Block::new().style(Style::new().bg(BG).fg(INK)), frame.area());
    let rows = Layout::vertical([Constraint::Length(3), Constraint::Length(2), Constraint::Min(12), Constraint::Length(1)]).split(frame.area());
    render_header(frame, rows[0], app);
    render_navigation(frame, rows[1], app.screen);
    match app.screen {
        Screen::Overview => overview(frame, rows[2], app),
        Screen::Markets => markets(frame, rows[2]),
        Screen::Security => security(frame, rows[2]),
        Screen::Portfolio => portfolio(frame, rows[2]),
        Screen::News => news(frame, rows[2], app.selected_news),
    }
    render_footer(frame, rows[3]);
}

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let cols = Layout::horizontal([Constraint::Length(31), Constraint::Min(35), Constraint::Length(25)]).split(area);
    frame.render_widget(Paragraph::new(Line::from(vec![Span::styled(" MT ", Style::new().bg(AMBER).fg(BG).bold()), Span::styled(" MARKET TERMINAL ", Style::new().fg(AMBER).bold()), Span::styled("RUST", Style::new().fg(MUTED))])).block(Block::new().borders(Borders::BOTTOM).border_style(MUTED)), cols[0]);
    let command = if app.command.is_empty() { "Enter security, function, or command" } else { app.command.as_str() };
    frame.render_widget(Paragraph::new(Line::from(vec![Span::styled(" ⌘ ", AMBER), Span::styled(command, if app.command.is_empty() { Style::new().fg(MUTED) } else { Style::new().fg(INK) }), Span::styled("  GO ", Style::new().bg(CYAN).fg(BG).bold())])).block(Block::new().borders(Borders::ALL).border_style(MUTED)), cols[1]);
    let seconds = (app.ticks / 5) % 60;
    frame.render_widget(Paragraph::new(Line::from(vec![Span::styled("● LIVE  ", GREEN), Span::styled(format!("10:42:{seconds:02}  "), INK), Span::styled("NYC", MUTED)])).alignment(Alignment::Right).block(Block::new().borders(Borders::BOTTOM).border_style(MUTED)), cols[2]);
}

fn render_navigation(frame: &mut Frame, area: Rect, active: Screen) {
    let mut spans = Vec::new();
    for (index, screen) in Screen::ALL.iter().enumerate() {
        let text = format!(" {} {} [{}] ", index + 1, screen.label(), screen.key().to_ascii_uppercase());
        spans.push(Span::styled(text, if *screen == active { Style::new().bg(CYAN).fg(BG).bold() } else { Style::new().fg(INK) }));
    }
    spans.push(Span::styled("  SPX 5,304.72 ", MUTED)); spans.push(Span::styled("+0.86%", GREEN)); spans.push(Span::styled("  NDX 18,658.32 ", MUTED)); spans.push(Span::styled("+1.00%", GREEN));
    frame.render_widget(Paragraph::new(Line::from(spans)).style(Style::new().bg(Color::Rgb(21, 32, 35))), area);
}

fn render_footer(frame: &mut Frame, area: Rect) {
    frame.render_widget(Paragraph::new(Line::from(vec![Span::styled(" Q/ESC ", AMBER), Span::raw("QUIT   "), Span::styled("G/M/S/P/N ", AMBER), Span::raw("SCREENS   "), Span::styled("↑↓/JK ", AMBER), Span::raw("MOVE   "), Span::styled("ENTER ", AMBER), Span::raw("COMMAND"), Span::styled("   DELAYED DEMO DATA · NOT INVESTMENT ADVICE", MUTED)])).style(Style::new().bg(Color::Rgb(40, 52, 54))), area);
}

fn terminal_block(code: &'static str, title: &'static str) -> Block<'static> {
    Block::new().borders(Borders::ALL).border_style(AMBER).title(Line::from(vec![Span::styled(format!(" {code} "), Style::new().bg(AMBER).fg(BG).bold()), Span::styled(format!(" {title} "), AMBER)]))
}

fn overview(frame: &mut Frame, area: Rect, app: &App) {
    let rows = Layout::vertical([Constraint::Length(2), Constraint::Percentage(52), Constraint::Length(7), Constraint::Min(8)]).split(area);
    let mut periods = vec![Span::raw(" ")];
    for (i, period) in PERIODS.iter().enumerate() { periods.push(Span::styled(format!(" {} {} ", i + 1, period), if i == app.selected_period { Style::new().bg(CYAN).fg(BG).bold() } else { Style::new().fg(CYAN) })); }
    periods.push(Span::styled("   ● MARKET OPEN  ", GREEN)); periods.push(Span::styled("REGULAR SESSION", MUTED));
    frame.render_widget(Paragraph::new(Line::from(periods)), rows[0]);
    let datasets = vec![Dataset::default().name("001 +17.1%").marker(symbols::Marker::Braille).graph_type(GraphType::Line).style(YELLOW).data(&RETURNS_A), Dataset::default().name("002 +14.3%").marker(symbols::Marker::Braille).graph_type(GraphType::Line).style(CYAN).data(&RETURNS_B)];
    let chart = Chart::new(datasets).block(terminal_block("PERF", "RETURNS — YTD (%)")).x_axis(Axis::default().bounds([0., 100.]).labels(["02 JAN", "30 MAR", "25 JUN"]).style(MUTED)).y_axis(Axis::default().bounds([-3., 18.]).labels(["−3.0", "7.7", "17.1"]).style(AMBER)).legend_position(Some(ratatui::widgets::LegendPosition::TopLeft));
    frame.render_widget(chart, rows[1]);
    let mid = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(25), Constraint::Percentage(25)]).split(rows[2]);
    render_table(frame, mid[0], "RISK", "RISK & RETURN", ["PORTFOLIO","RETURN","MAX DD","SHARPE"], &[["001","+17.02%","−6.3%","2.79"],["002","+13.87%","−6.6%","2.28"]], [Constraint::Percentage(30),Constraint::Percentage(25),Constraint::Percentage(25),Constraint::Percentage(20)]);
    render_pairs(frame, mid[1], "ASST", "ASSET RETURNS", &[["SPYY","+13.97%"],["IS3R","+30.31%"],["AVWS","+22.05%"]]);
    render_pairs(frame, mid[2], "WATC", "WATCHLIST", &[["AVWC","+16.72%"],["DEGC","+13.15%"],["DEGT","+12.75%"]]);
    let bottom = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[3]);
    render_pairs(frame, bottom[0], "PORT", "TOP HOLDINGS", &[["NVIDIA CORPORATION","4.8%"],["APPLE INC.","4.3%"],["MICROSOFT CORPORATION","2.6%"],["AMAZON.COM INC.","2.3%"],["ALPHABET CLASS A","2.0%"]]);
    render_pairs(frame, bottom[1], "TOP", "NEWS & MOVERS", &[["ADVANTEST CORP.","+15.06%"],["KIOXIA HOLDINGS","+12.27%"],["MS&AD INSURANCE","−4.45%"],["KOMATSU LTD.","−3.56%"]]);
}

fn markets(frame: &mut Frame, area: Rect) {
    let cols = Layout::horizontal([Constraint::Percentage(66), Constraint::Percentage(34)]).split(area);
    let left = Layout::vertical([Constraint::Percentage(48), Constraint::Percentage(22), Constraint::Percentage(30)]).split(cols[0]);
    render_table(frame, left[0], "WEI", "WORLD EQUITY INDICES", ["INDEX","SYMBOL","LAST","NET CHG","% CHG"], &MARKETS, [Constraint::Percentage(27),Constraint::Percentage(14),Constraint::Percentage(23),Constraint::Percentage(19),Constraint::Percentage(17)]);
    render_pairs(frame, left[1], "XAM", "CROSS-ASSET MONITOR", &[["US 10Y","4.312  +3.2BP"],["DXY","104.72  −0.18%"],["EUR/USD","1.0837  +0.21%"],["WTI","78.42  +1.14%"],["GOLD","2,337.80  −0.36%"]]);
    let curve = Chart::new(vec![Dataset::default().marker(symbols::Marker::Braille).graph_type(GraphType::Line).style(YELLOW).data(&CURVE)]).block(terminal_block("GC", "U.S. TREASURY CURVE")).x_axis(Axis::default().bounds([0.,100.]).labels(["3M","5Y","10Y","30Y"]).style(MUTED)).y_axis(Axis::default().bounds([4.2,5.5]).labels(["4.2","4.8","5.5"]).style(AMBER));
    frame.render_widget(curve, left[2]);
    let right = Layout::vertical([Constraint::Percentage(45), Constraint::Percentage(27), Constraint::Percentage(28)]).split(cols[1]);
    render_pairs(frame, right[0], "IMAP", "SECTOR PERFORMANCE", &[["TECHNOLOGY","+1.56%"],["COMMUNICATION","+1.11%"],["CONS. DISC.","+0.69%"],["FINANCIALS","+0.42%"],["HEALTH CARE","−0.15%"],["UTILITIES","−0.67%"],["ENERGY","−1.21%"]]);
    render_pairs(frame, right[1], "MBR", "MARKET BREADTH", &[["NYSE ADV / DEC","2,181 / 812"],["NEW HIGHS / LOWS","224 / 31"],["UP / DOWN VOLUME","4.7X"],["ABOVE 200 DMA","62.8%"]]);
    render_pairs(frame, right[2], "ECO", "ECONOMIC CALENDAR", &[["08:30","US INITIAL CLAIMS"],["10:00","US EXISTING HOMES"],["14:00","FED BEIGE BOOK"]]);
}

fn security(frame: &mut Frame, area: Rect) {
    let rows = Layout::vertical([Constraint::Length(4), Constraint::Min(12)]).split(area);
    frame.render_widget(Paragraph::new(Line::from(vec![Span::styled(" AAPL US EQUITY · APPLE INC  ", AMBER),Span::styled("205.30  ", Style::new().fg(CYAN).bold()),Span::styled("+1.72  +0.84%  ", GREEN),Span::styled("OPEN 203.41  HIGH 205.64  LOW 202.72  VOLUME 41.82M", MUTED)])).block(Block::new().borders(Borders::ALL).border_style(AMBER)).alignment(Alignment::Center), rows[0]);
    let grid = Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(19), Constraint::Percentage(19)]).split(rows[1]);
    let left = Layout::vertical([Constraint::Percentage(58), Constraint::Percentage(42)]).split(grid[0]);
    let price = Chart::new(vec![Dataset::default().name("AAPL 205.30").marker(symbols::Marker::Braille).graph_type(GraphType::Line).style(CYAN).data(&AAPL)]).block(terminal_block("GP", "INTRADAY PRICE")).x_axis(Axis::default().bounds([0.,100.]).labels(["09:30","12:30","16:00"]).style(MUTED)).y_axis(Axis::default().bounds([195.,207.]).labels(["195","201","207"]).style(AMBER)); frame.render_widget(price,left[0]);
    render_table(frame,left[1],"FA","FINANCIAL SNAPSHOT",["USD BN","FY24","FY25E","FY26E"],&[["REVENUE","391.0","414.8","438.1"],["EBITDA","131.4","140.7","151.2"],["EPS","6.57","7.24","7.93"]],[Constraint::Percentage(34),Constraint::Percentage(22),Constraint::Percentage(22),Constraint::Percentage(22)]);
    render_pairs(frame,grid[1],"DES","KEY STATISTICS",&[["MARKET CAP","$3.15T"],["P/E (TTM)","31.92X"],["P/E (FY1)","29.44X"],["DIV YIELD","0.49%"],["52W RANGE","164—237"],["BETA","1.21"]]);
    render_pairs(frame,grid[2],"ANR","ANALYSTS",&[["BUY","32"],["HOLD","12"],["SELL","3"],["CONSENSUS","4.31 / 5"],["TARGET","$224.62"],["UPSIDE","+9.41%"]]);
}

fn portfolio(frame: &mut Frame, area: Rect) {
    let rows = Layout::vertical([Constraint::Length(4), Constraint::Min(10)]).split(area);
    let kpis = Layout::horizontal([Constraint::Ratio(1,4);4]).split(rows[0]);
    for (i,(label,value)) in [["NET ASSET VALUE","$1,045,228"],["YTD RETURN","+17.02%"],["AVAILABLE CASH","$127,834"],["SHARPE","2.79"]].iter().enumerate() { frame.render_widget(Paragraph::new(vec![Line::styled(*label,MUTED),Line::styled(*value,if i==1 {GREEN}else{CYAN})]).block(Block::new().borders(Borders::ALL).border_style(AMBER)).alignment(Alignment::Center),kpis[i]); }
    let cols = Layout::horizontal([Constraint::Percentage(62),Constraint::Percentage(20),Constraint::Percentage(18)]).split(rows[1]);
    render_table(frame,cols[0],"PORT","POSITIONS",["SYMBOL","QTY","AVG COST","MKT VALUE","P&L","WEIGHT"],&POSITIONS,[Constraint::Percentage(13),Constraint::Percentage(13),Constraint::Percentage(18),Constraint::Percentage(22),Constraint::Percentage(18),Constraint::Percentage(16)]);
    render_pairs(frame,cols[1],"PMAP","ALLOCATION",&[["TECHNOLOGY","59.2%"],["BROAD MARKET","10.6%"],["CASH","12.2%"],["OTHER","18.0%"]]);
    let right=Layout::vertical([Constraint::Percentage(50),Constraint::Percentage(50)]).split(cols[2]);
    render_pairs(frame,right[0],"ATTR","ATTRIBUTION",&[["NVDA","+5.48%"],["META","+2.14%"],["MSFT","+1.83%"],["AMZN","+1.62%"]]);
    render_pairs(frame,right[1],"MARS","RISK SCENARIOS",&[["SPX −10%","−$83,441"],["NASDAQ −20%","−$194,702"],["RATES +100BP","−$18,821"]]);
}

fn news(frame: &mut Frame, area: Rect, selected: usize) {
    let cols=Layout::horizontal([Constraint::Percentage(39),Constraint::Percentage(43),Constraint::Percentage(18)]).split(area);
    let items=HEADLINES.iter().enumerate().map(|(i,h)| ListItem::new(Line::from(vec![Span::styled(format!("{} ",h[0]),MUTED),Span::styled(format!("{:<4}",h[1]),AMBER),Span::styled(h[2],if i==selected{Style::new().bg(CYAN).fg(BG)}else{Style::new().fg(INK)})]))).collect::<Vec<_>>();
    frame.render_widget(List::new(items).block(terminal_block("TOP","TOP NEWS")),cols[0]);
    let headline=HEADLINES[selected][2];
    let story=vec![Line::styled("TOP · 16:00 ET · AUG 25, 2026",AMBER),Line::raw(""),Line::styled(headline,Style::new().fg(INK).bold()),Line::raw(""),Line::raw("Markets moved decisively into positive territory as investors weighed resilient corporate earnings against a shifting interest-rate outlook."),Line::raw(""),Line::raw("Technology shares led the advance while market breadth remained constructive."),Line::raw(""),Line::styled("“The move is broader than a single theme.”",YELLOW),Line::raw(""),Line::raw("Attention now turns to inflation data and central-bank guidance.")];
    frame.render_widget(Paragraph::new(story).wrap(Wrap{trim:true}).block(terminal_block("READ","STORY")),cols[1]);
    let right=Layout::vertical([Constraint::Percentage(52),Constraint::Percentage(48)]).split(cols[2]);
    render_pairs(frame,right[0],"MOST","MOST READ",&[["1","CHIP RALLY"],["2","FED PATH"],["3","OIL OUTLOOK"],["4","DOLLAR FALLS"]]);
    render_pairs(frame,right[1],"MOV","LIVE MOVERS",&[["NVDA","+4.21%"],["META","+2.85%"],["AMZN","+2.31%"],["MRNA","−4.32%"]]);
}

fn render_pairs<const N: usize>(frame: &mut Frame, area: Rect, code: &'static str, title: &'static str, data: &[[&str; N]]) {
    let lines=data.iter().map(|row| { let mut spans=Vec::new(); for (i,value) in row.iter().enumerate(){ let style=if i+1==N {value_style(value)} else {Style::new().fg(INK)}; spans.push(Span::styled(format!("{:<width$}",value,width=if i+1==N{1}else{22}),style)); } Line::from(spans) }).collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines).block(terminal_block(code,title)),area);
}

fn render_table<const N: usize>(frame:&mut Frame,area:Rect,code:&'static str,title:&'static str,header:[&'static str;N],data:&[[&str;N]],widths:[Constraint;N]){
    let rows=data.iter().map(|row|Row::new(row.iter().map(|value|Cell::from(*value).style(value_style(value)))));
    let table=Table::new(rows,widths).header(Row::new(header).style(Style::new().fg(AMBER).add_modifier(Modifier::BOLD)).bottom_margin(1)).column_spacing(1).block(terminal_block(code,title));
    frame.render_widget(table,area);
}

fn value_style(value:&str)->Style{if value.starts_with('+'){Style::new().fg(GREEN)}else if value.starts_with('−')||value.starts_with('-'){Style::new().fg(RED)}else{Style::new().fg(INK)}}
