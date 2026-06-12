use crate::handler::handle_key_event;
use crate::nvml_gpm::Nvml;
use crate::tui::Tui;
use crate::ui;
use crate::Args;
use anyhow::{bail, Result};
use crossterm::event::{Event, EventStream};
use futures::StreamExt;
use ratatui::widgets::ScrollbarState;
use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, RwLock,
};
use std::time::{Duration, Instant};

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum ViewMode {
    Numbers,
    Chart,
}

#[derive(Clone, Debug)]
pub struct SamplePoint {
    pub t: f64,
    pub tx_mib_s: f64,
    pub rx_mib_s: f64,
}

#[derive(Clone, Debug)]
pub struct GpuHistory {
    pub index: u32,
    pub name: String,
    pub pci_bus_id: String,
    pub tx_mib_s: f64,
    pub rx_mib_s: f64,
    pub status: String,
    pub history: VecDeque<SamplePoint>,
}

impl GpuHistory {
    pub fn new(index: u32, name: String, pci_bus_id: String) -> Self {
        Self {
            index,
            name,
            pci_bus_id,
            tx_mib_s: 0.0,
            rx_mib_s: 0.0,
            status: "initializing".to_string(),
            history: VecDeque::new(),
        }
    }

    pub fn push(&mut self, t: f64, tx_mib_s: f64, rx_mib_s: f64, cap: usize, status: String) {
        self.tx_mib_s = tx_mib_s;
        self.rx_mib_s = rx_mib_s;
        self.status = status;

        if self.history.len() >= cap {
            self.history.pop_front();
        }

        self.history.push_back(SamplePoint {
            t,
            tx_mib_s,
            rx_mib_s,
        });
    }
}

pub struct App {
    pub should_quit: bool,
    pub view_mode: ViewMode,
    pub version: String,
    pub interval_ms: u64,
    pub history_points: usize,
    pub started: Instant,
    pub histories: Arc<RwLock<Vec<GpuHistory>>>,
    pub vertical_scroll: usize,
    pub scroll_state: ScrollbarState,
    running: Arc<AtomicBool>,
}

impl App {
    pub async fn try_new(args: Args) -> Result<Self> {
        if args.interval_ms <= 100 {
            bail!("--interval-ms must be >100; recommended values are 200 or 1000");
        }
        if args.history_points == 0 {
            bail!("--history-points must be >0");
        }

        let view_mode = parse_view_mode(&args.view)?;
        let histories = Arc::new(RwLock::new(Vec::new()));
        let running = Arc::new(AtomicBool::new(true));

        let selected_gpus = args.gpus.clone();
        let interval_ms = args.interval_ms;
        let history_points = args.history_points;
        let histories_for_task = histories.clone();
        let running_for_task = running.clone();

        tokio::task::spawn_blocking(move || {
            sampler_loop(
                selected_gpus,
                interval_ms,
                history_points,
                histories_for_task,
                running_for_task,
            );
        });

        Ok(Self {
            should_quit: false,
            view_mode,
            version: env!("CARGO_PKG_VERSION").to_string(),
            interval_ms: args.interval_ms,
            history_points: args.history_points,
            started: Instant::now(),
            histories,
            vertical_scroll: 0,
            scroll_state: ScrollbarState::new(0),
            running,
        })
    }

    pub async fn run(&mut self, tui: &mut Tui) -> Result<()> {
        let mut event_stream = EventStream::new();
        let mut ui_interval = tokio::time::interval(Duration::from_millis(100));

        while !self.should_quit {
            self.sync_scroll_state();
            tui.draw(|f| ui::render(self, f))?;

            tokio::select! {
                _ = ui_interval.tick() => {},
                Some(Ok(event)) = event_stream.next() => {
                    if let Event::Key(key) = event {
                        handle_key_event(key, self)?;
                    }
                },
                _ = tokio::signal::ctrl_c() => {
                    self.quit();
                },
            }
        }
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }

    pub fn toggle_view_mode(&mut self) {
        self.view_mode = match self.view_mode {
            ViewMode::Numbers => ViewMode::Chart,
            ViewMode::Chart => ViewMode::Numbers,
        };
    }

    pub fn quit(&mut self) {
        self.should_quit = true;
        self.running.store(false, Ordering::SeqCst);
    }

    pub fn on_up(&mut self) {
        if self.vertical_scroll > 0 {
            self.vertical_scroll = self.vertical_scroll.saturating_sub(1);
        }
    }

    pub fn on_down(&mut self) {
        let len = self.histories.read().map(|h| h.len()).unwrap_or(0);
        if self.vertical_scroll < len.saturating_sub(1) {
            self.vertical_scroll = self.vertical_scroll.saturating_add(1);
        }
    }

    fn sync_scroll_state(&mut self) {
        let len = self.histories.read().map(|h| h.len()).unwrap_or(0);
        self.vertical_scroll = self.vertical_scroll.min(len.saturating_sub(1));
        self.scroll_state = ScrollbarState::new(len).position(self.vertical_scroll);
    }
}

fn parse_view_mode(s: &str) -> Result<ViewMode> {
    match s {
        "numbers" | "number" | "table" | "v0.1" => Ok(ViewMode::Numbers),
        "chart" | "line" | "graph" | "v0.3" => Ok(ViewMode::Chart),
        _ => bail!("invalid --view {}; use numbers or chart", s),
    }
}

fn sampler_loop(
    selected_gpus: Vec<u32>,
    interval_ms: u64,
    history_points: usize,
    shared: Arc<RwLock<Vec<GpuHistory>>>,
    running: Arc<AtomicBool>,
) {
    let nvml = match Nvml::init() {
        Ok(n) => n,
        Err(e) => {
            set_error(&shared, format!("NVML init failed: {e}"));
            return;
        }
    };

    let count = match nvml.device_count() {
        Ok(c) => c,
        Err(e) => {
            set_error(&shared, format!("NVML device count failed: {e}"));
            return;
        }
    };

    let indices: Vec<u32> = if selected_gpus.is_empty() {
        (0..count).collect()
    } else {
        selected_gpus
    };

    let mut devices = Vec::new();
    let mut init_failures = Vec::new();

    for index in indices {
        match nvml.open_device(index) {
            Ok(dev) => devices.push(dev),
            Err(e) => init_failures.push(GpuHistory {
                index,
                name: format!("GPU{index}"),
                pci_bus_id: "unknown".to_string(),
                tx_mib_s: f64::NAN,
                rx_mib_s: f64::NAN,
                status: format!("init failed: {e}"),
                history: VecDeque::new(),
            }),
        }
    }

    {
        let mut data = shared.write().unwrap();
        data.clear();
        for dev in &devices {
            data.push(GpuHistory::new(
                dev.meta.index,
                dev.meta.name.clone(),
                dev.meta.pci_bus_id.clone(),
            ));
        }
        data.extend(init_failures);
    }

    let start = Instant::now();

    while running.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(interval_ms));
        if !running.load(Ordering::SeqCst) {
            break;
        }

        let t = start.elapsed().as_secs_f64();
        let mut data = match shared.write() {
            Ok(d) => d,
            Err(_) => return,
        };

        for (i, dev) in devices.iter_mut().enumerate() {
            let reading = dev.read();
            if let Some(series) = data.get_mut(i) {
                series.push(
                    t,
                    reading.tx_mib_s,
                    reading.rx_mib_s,
                    history_points,
                    reading.status,
                );
            }
        }
    }

    drop(devices);
    drop(nvml);
}

fn set_error(shared: &Arc<RwLock<Vec<GpuHistory>>>, status: String) {
    let mut data = shared.write().unwrap();
    data.clear();
    data.push(GpuHistory {
        index: 0,
        name: "NVML".to_string(),
        pci_bus_id: "-".to_string(),
        tx_mib_s: f64::NAN,
        rx_mib_s: f64::NAN,
        status,
        history: VecDeque::new(),
    });
}
