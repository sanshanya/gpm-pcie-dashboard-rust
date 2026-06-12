use crate::app::{App, GpuHistory, ViewMode};
use ratatui::prelude::*;
use ratatui::symbols;
use ratatui::widgets::{
    Axis, Block, Borders, Chart, Dataset, GraphType, Paragraph, Row, Scrollbar,
    ScrollbarOrientation, Table,
};

pub fn render(app: &mut App, frame: &mut Frame) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(frame.area());

    match app.view_mode {
        ViewMode::Numbers => render_numbers(app, frame, root[0]),
        ViewMode::Chart => render_charts(app, frame, root[0]),
    }

    render_footer(app, frame, root[1]);
}

fn render_footer(app: &App, frame: &mut Frame, area: Rect) {
    let mode = match app.view_mode {
        ViewMode::Numbers => "v0.1 Numbers",
        ViewMode::Chart => "v0.3 Line Chart",
    };

    let uptime = app.started.elapsed().as_secs();
    let line = Line::from(vec![
        Span::styled(" GPM PCIe Dashboard ", Style::default().bold()),
        Span::raw(" | "),
        Span::styled(mode, Style::default().fg(Color::Cyan).bold()),
        Span::raw(format!(" | interval={}ms", app.interval_ms)),
        Span::raw(format!(" | history={}pts", app.history_points)),
        Span::raw(format!(" | uptime={}s", uptime)),
        Span::raw(" | "),
        Span::styled("[Tab/v/Space]", Style::default().fg(Color::Yellow).bold()),
        Span::raw(" switch "),
        Span::styled("[j/k]", Style::default().fg(Color::Yellow).bold()),
        Span::raw(" scroll "),
        Span::styled("[q/Esc]", Style::default().fg(Color::Red).bold()),
        Span::raw(" quit"),
    ]);

    frame.render_widget(Paragraph::new(line).alignment(Alignment::Center), area);
}

fn render_numbers(app: &mut App, frame: &mut Frame, area: Rect) {
    let data = snapshot_histories(app);

    let rows = data.iter().skip(app.vertical_scroll).map(|g| {
        let total = if g.tx_mib_s.is_finite() && g.rx_mib_s.is_finite() {
            g.tx_mib_s + g.rx_mib_s
        } else {
            f64::NAN
        };

        let style = if g.status == "OK" {
            Style::default()
        } else {
            Style::default().fg(Color::Red)
        };

        Row::new(vec![
            g.index.to_string(),
            g.name.clone(),
            g.pci_bus_id.clone(),
            fmt_bw(g.tx_mib_s),
            fmt_bw(g.rx_mib_s),
            fmt_bw(total),
            g.status.clone(),
        ])
        .style(style)
    });

    let table = Table::new(
        rows,
        [
            Constraint::Length(5),
            Constraint::Length(28),
            Constraint::Length(20),
            Constraint::Length(15),
            Constraint::Length(15),
            Constraint::Length(15),
            Constraint::Min(10),
        ],
    )
    .header(
        Row::new(vec![
            "GPU",
            "Name",
            "PCI Bus ID",
            "TX from GPU",
            "RX to GPU",
            "TX+RX",
            "Status",
        ])
        .style(Style::default().fg(Color::Yellow).bold()),
    )
    .block(
        Block::default()
            .title("v0.1 Numbers - NVML GPM 20/21, GPU-perspective aggregate PCIe traffic")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue)),
    );

    frame.render_widget(table, area);

    if data.len() > 1 {
        let scrollbar = Scrollbar::default().orientation(ScrollbarOrientation::VerticalRight);
        frame.render_stateful_widget(scrollbar, area, &mut app.scroll_state);
    }
}

fn render_charts(app: &mut App, frame: &mut Frame, area: Rect) {
    const CHART_HEIGHT: u16 = 12;

    let data = snapshot_histories(app);

    if data.is_empty() {
        frame.render_widget(
            Paragraph::new("waiting for samples...").block(Block::default().borders(Borders::ALL)),
            area,
        );
        return;
    }

    let items_per_screen = (area.height / CHART_HEIGHT).max(1) as usize;
    let start = app.vertical_scroll.min(data.len().saturating_sub(1));
    let end = (start + items_per_screen).min(data.len());

    let constraints = (start..end)
        .map(|_| Constraint::Length(CHART_HEIGHT))
        .collect::<Vec<_>>();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    for (slot, g) in data[start..end].iter().enumerate() {
        render_single_chart(frame, chunks[slot], g);
    }

    if data.len() > items_per_screen {
        let scrollbar = Scrollbar::default().orientation(ScrollbarOrientation::VerticalRight);
        frame.render_stateful_widget(scrollbar, area, &mut app.scroll_state);
    }
}

fn render_single_chart(frame: &mut Frame, area: Rect, g: &GpuHistory) {
    let tx: Vec<(f64, f64)> = g
        .history
        .iter()
        .filter(|p| p.tx_mib_s.is_finite())
        .map(|p| (p.t, p.tx_mib_s))
        .collect();

    let rx: Vec<(f64, f64)> = g
        .history
        .iter()
        .filter(|p| p.rx_mib_s.is_finite())
        .map(|p| (p.t, p.rx_mib_s))
        .collect();

    let x_min = g.history.front().map(|p| p.t).unwrap_or(0.0);
    let x_max = g.history.back().map(|p| p.t).unwrap_or(1.0).max(x_min + 1.0);

    let max_y = tx
        .iter()
        .chain(rx.iter())
        .map(|(_, y)| *y)
        .fold(0.0_f64, f64::max);
    let y_max = if max_y < 1024.0 { 1024.0 } else { max_y * 1.15 };

    let datasets = vec![
        Dataset::default()
            .name("TX from GPU")
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(Color::Cyan))
            .data(&tx),
        Dataset::default()
            .name("RX to GPU")
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(Color::Magenta))
            .data(&rx),
    ];

    let title = format!(
        "v0.3 Chart - GPU{} {} | {} | status={}",
        g.index, g.name, g.pci_bus_id, g.status
    );

    let chart = Chart::new(datasets)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green)),
        )
        .x_axis(
            Axis::default()
                .style(Style::default().fg(Color::DarkGray))
                .bounds([x_min, x_max])
                .labels(vec![
                    Span::raw(format!("{:.1}s", x_min)),
                    Span::raw(format!("{:.1}s", x_max)),
                ]),
        )
        .y_axis(
            Axis::default()
                .style(Style::default().fg(Color::DarkGray))
                .bounds([0.0, y_max])
                .labels(vec![
                    Span::raw("0"),
                    Span::styled(fmt_bw(y_max), Style::default().bold()),
                ]),
        );

    frame.render_widget(chart, area);
}

fn snapshot_histories(app: &App) -> Vec<GpuHistory> {
    app.histories.read().map(|h| h.clone()).unwrap_or_default()
}

fn fmt_bw(mib_s: f64) -> String {
    if !mib_s.is_finite() {
        return "NaN".to_string();
    }

    if mib_s >= 1024.0 {
        format!("{:.2} GiB/s", mib_s / 1024.0)
    } else {
        format!("{:.2} MiB/s", mib_s)
    }
}
