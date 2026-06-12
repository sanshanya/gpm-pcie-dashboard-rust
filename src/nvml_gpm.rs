#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use anyhow::{anyhow, bail, Result};
use std::ffi::CStr;
use std::mem;
use std::os::raw::{c_char, c_uint};

pub mod ffi {
    include!(concat!(env!("OUT_DIR"), "/nvml.rs"));
}

const NVML_SUCCESS_CODE: ffi::nvmlReturn_t = 0;
const NVML_FEATURE_ENABLED_CODE: c_uint = 1;

// NVML GPM enum values. They are enum constants in nvml.h, not preprocessor macros.
const GPM_PCIE_TX_PER_SEC: c_uint = 20;
const GPM_PCIE_RX_PER_SEC: c_uint = 21;

pub fn nvml_error(code: ffi::nvmlReturn_t) -> String {
    unsafe {
        let p = ffi::nvmlErrorString(code);
        if p.is_null() {
            return format!("NVML error code {code}");
        }
        CStr::from_ptr(p).to_string_lossy().into_owned()
    }
}

fn check(code: ffi::nvmlReturn_t, what: &str) -> Result<()> {
    if code == NVML_SUCCESS_CODE {
        Ok(())
    } else {
        Err(anyhow!("{}: {}", what, nvml_error(code)))
    }
}

pub struct Nvml;

impl Nvml {
    pub fn init() -> Result<Self> {
        unsafe {
            check(ffi::nvmlInit_v2(), "nvmlInit_v2")?;
        }
        Ok(Self)
    }

    pub fn device_count(&self) -> Result<u32> {
        let mut count: c_uint = 0;
        unsafe {
            check(ffi::nvmlDeviceGetCount_v2(&mut count), "nvmlDeviceGetCount_v2")?;
        }
        Ok(count as u32)
    }
}

impl Drop for Nvml {
    fn drop(&mut self) {
        unsafe {
            let _ = ffi::nvmlShutdown();
        }
    }
}

#[derive(Debug, Clone)]
pub struct GpuMeta {
    pub index: u32,
    pub name: String,
    pub pci_bus_id: String,
}

#[derive(Debug, Clone)]
pub struct GpmReading {
    pub tx_mib_s: f64,
    pub rx_mib_s: f64,
    pub status: String,
}

pub struct GpmDevice {
    pub meta: GpuMeta,
    handle: ffi::nvmlDevice_t,
    sample_prev: ffi::nvmlGpmSample_t,
    sample_now: ffi::nvmlGpmSample_t,
}

impl GpmDevice {
    pub fn new(index: u32) -> Result<Self> {
        let mut handle: ffi::nvmlDevice_t = std::ptr::null_mut();

        unsafe {
            check(
                ffi::nvmlDeviceGetHandleByIndex_v2(index as c_uint, &mut handle),
                "nvmlDeviceGetHandleByIndex_v2",
            )?;
        }

        let name = get_device_name(handle).unwrap_or_else(|_| format!("GPU{index}"));
        let pci_bus_id = get_pci_bus_id(handle).unwrap_or_else(|_| "unknown".to_string());

        init_gpm(handle, index)?;

        let mut sample_prev: ffi::nvmlGpmSample_t = std::ptr::null_mut();
        let mut sample_now: ffi::nvmlGpmSample_t = std::ptr::null_mut();

        unsafe {
            check(ffi::nvmlGpmSampleAlloc(&mut sample_prev), "nvmlGpmSampleAlloc(prev)")?;
            check(ffi::nvmlGpmSampleAlloc(&mut sample_now), "nvmlGpmSampleAlloc(now)")?;
            check(ffi::nvmlGpmSampleGet(handle, sample_prev), "initial nvmlGpmSampleGet")?;
        }

        Ok(Self {
            meta: GpuMeta {
                index,
                name,
                pci_bus_id,
            },
            handle,
            sample_prev,
            sample_now,
        })
    }

    pub fn read(&mut self) -> GpmReading {
        match self.try_read() {
            Ok((tx_mib_s, rx_mib_s)) => GpmReading {
                tx_mib_s,
                rx_mib_s,
                status: "OK".to_string(),
            },
            Err(e) => GpmReading {
                tx_mib_s: f64::NAN,
                rx_mib_s: f64::NAN,
                status: e.to_string(),
            },
        }
    }

    fn try_read(&mut self) -> Result<(f64, f64)> {
        unsafe {
            check(ffi::nvmlGpmSampleGet(self.handle, self.sample_now), "nvmlGpmSampleGet")?;

            let mut get: ffi::nvmlGpmMetricsGet_t = mem::zeroed();
            get.version = ffi::NVML_GPM_METRICS_GET_VERSION;
            get.sample1 = self.sample_prev;
            get.sample2 = self.sample_now;
            get.numMetrics = 2;
            get.metrics[0].metricId = GPM_PCIE_TX_PER_SEC;
            get.metrics[1].metricId = GPM_PCIE_RX_PER_SEC;

            check(ffi::nvmlGpmMetricsGet(&mut get), "nvmlGpmMetricsGet")?;

            let tx_status = get.metrics[0].nvmlReturn;
            let rx_status = get.metrics[1].nvmlReturn;

            if tx_status != NVML_SUCCESS_CODE {
                bail!("GPM TX metric failed: {}", nvml_error(tx_status));
            }

            if rx_status != NVML_SUCCESS_CODE {
                bail!("GPM RX metric failed: {}", nvml_error(rx_status));
            }

            let tx = get.metrics[0].value;
            let rx = get.metrics[1].value;

            std::mem::swap(&mut self.sample_prev, &mut self.sample_now);
            Ok((tx, rx))
        }
    }
}

impl Drop for GpmDevice {
    fn drop(&mut self) {
        unsafe {
            if !self.sample_prev.is_null() {
                let _ = ffi::nvmlGpmSampleFree(self.sample_prev);
            }
            if !self.sample_now.is_null() {
                let _ = ffi::nvmlGpmSampleFree(self.sample_now);
            }
        }
    }
}

fn get_device_name(handle: ffi::nvmlDevice_t) -> Result<String> {
    let mut buf = [0 as c_char; 128];
    unsafe {
        check(
            ffi::nvmlDeviceGetName(handle, buf.as_mut_ptr(), buf.len() as c_uint),
            "nvmlDeviceGetName",
        )?;
        Ok(CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned())
    }
}

fn get_pci_bus_id(handle: ffi::nvmlDevice_t) -> Result<String> {
    unsafe {
        let mut pci: ffi::nvmlPciInfo_t = mem::zeroed();
        check(ffi::nvmlDeviceGetPciInfo_v3(handle, &mut pci), "nvmlDeviceGetPciInfo_v3")?;
        Ok(CStr::from_ptr(pci.busId.as_ptr()).to_string_lossy().into_owned())
    }
}

fn init_gpm(handle: ffi::nvmlDevice_t, index: u32) -> Result<()> {
    unsafe {
        let mut support: ffi::nvmlGpmSupport_t = mem::zeroed();
        support.version = ffi::NVML_GPM_SUPPORT_VERSION;

        check(
            ffi::nvmlGpmQueryDeviceSupport(handle, &mut support),
            "nvmlGpmQueryDeviceSupport",
        )?;

        if support.isSupportedDevice == 0 {
            bail!("GPU {index} does not support NVML GPM");
        }

        let mut state: c_uint = 0;
        check(
            ffi::nvmlGpmQueryIfStreamingEnabled(handle, &mut state),
            "nvmlGpmQueryIfStreamingEnabled",
        )?;

        if state != NVML_FEATURE_ENABLED_CODE {
            let r = ffi::nvmlGpmSetStreamingEnabled(handle, NVML_FEATURE_ENABLED_CODE);
            if r != NVML_SUCCESS_CODE {
                bail!(
                    "failed to enable GPM streaming on GPU {index}: {}; try `sudo nvidia-smi gpm -i {index} -s 1`",
                    nvml_error(r),
                );
            }
        }
    }

    Ok(())
}
