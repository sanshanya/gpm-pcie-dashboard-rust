mod app;
mod handler;
mod nvml_gpm;
mod tui;
mod ui;

use anyhow::Result;
use app::App;
use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about = "TUI dashboard for NVML GPM PCIe TX/RX traffic", long_about = None)]
pub struct Args {
    /// Monitor one or more GPU indices. If omitted, monitor all GPUs.
    #[arg(long = "gpu", short = 'g')]
    pub gpus: Vec<u32>,

    /// Sampling interval in milliseconds. NVML GPM requires >100ms; 200ms is recommended for high resolution.
    #[arg(long, default_value_t = 200)]
    pub interval_ms: u64,

    /// History points retained per GPU for chart mode.
    #[arg(long, default_value_t = 600)]
    pub history_points: usize,

    /// Initial view: numbers/table/v0.1 or chart/line/v0.3.
    #[arg(long, default_value = "numbers")]
    pub view: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let mut tui = tui::Tui::new()?;
    let mut app = App::try_new(args).await?;
    app.run(&mut tui).await?;
    Ok(())
}
