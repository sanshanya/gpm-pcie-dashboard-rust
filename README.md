# gpm-pcie-dashboard-rust

Rust TUI dashboard for per-GPU PCIe traffic using NVIDIA NVML GPM metrics.

This tool is intended for B200/B300/Hopper+ systems where legacy PCIe throughput paths such as `nvidia-smi dmon -s t` or `nvmlDeviceGetPcieThroughput()` may report zero or otherwise fail to represent the desired GPU traffic. It uses NVML GPM metrics:

- `20`: `NVML_GPM_METRIC_PCIE_TX_PER_SEC` — PCIe traffic from this GPU, MiB/s
- `21`: `NVML_GPM_METRIC_PCIE_RX_PER_SEC` — PCIe traffic to this GPU, MiB/s

The metric is GPU-perspective aggregate PCIe traffic. It is not a per-physical-link counter and does not split Gen5/Gen6 physical segments.

## Views

- `v0.1 Numbers`: table view of current TX/RX per GPU.
- `v0.3 Line Chart`: TX/RX bandwidth history line chart.

Keys:

- `Tab`, `v`, or `Space`: switch view
- `j/k` or arrow keys: scroll GPUs
- `q`, `Esc`, or `Ctrl-C`: quit

## Build

The build uses `bindgen` against your installed `nvml.h`, so it matches the actual NVML ABI on your node.

Install requirements on Ubuntu:

```bash
sudo apt update
sudo apt install -y build-essential clang libclang-dev pkg-config
```

Build on a CUDA/NVIDIA node:

```bash
export NVML_INCLUDE_DIR=/usr/local/cuda/include
export NVML_LIB_DIR=/usr/lib/x86_64-linux-gnu
cargo build --release
```

If your `nvml.h` is elsewhere, set `NVML_INCLUDE_DIR` to the directory containing `nvml.h`.

## Run

Enable GPM streaming first if needed:

```bash
sudo nvidia-smi gpm -i 0 -s 1
```

Monitor all GPUs:

```bash
./target/release/gpm-pcie-dashboard-rust
```

Monitor selected GPUs:

```bash
./target/release/gpm-pcie-dashboard-rust --gpu 0 --gpu 1
```

High-resolution sampling:

```bash
./target/release/gpm-pcie-dashboard-rust --gpu 0 --interval-ms 200
```

Stable 1s monitoring:

```bash
./target/release/gpm-pcie-dashboard-rust --gpu 0 --interval-ms 1000
```

Start in chart mode:

```bash
./target/release/gpm-pcie-dashboard-rust --view chart
```

## Sampling interval

NVML GPM requires two samples and a sample interval greater than 100ms. This tool enforces `--interval-ms > 100`.

Recommended values:

- `200ms`: high-resolution interactive monitoring
- `1000ms`: stable long-running monitoring

## Semantics

The main UI intentionally does not show PCIe Gen5/Gen6 endpoint link state. GPM 20/21 is not a per-link or per-port counter, so showing endpoint link state next to GPM traffic can falsely imply traffic-path attribution.

Use this tool for:

- Per-GPU aggregate PCIe TX/RX
- B200/B300 systems where `dmon -s t` is unreliable
- Sub-second PCIe traffic visualization

Do not use this tool for:

- Separating Gen5 vs Gen6 physical link traffic
- PCIe switch port-level attribution
- CPU root-port aggregate analysis
