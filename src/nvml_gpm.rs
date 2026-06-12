#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use anyhow::{anyhow, bail, Context, Result};
use libloading::{Library, Symbol};
use std::ffi::CStr;
use std::mem;
use std::os::raw::{c_char, c_uint};
use std::sync::Arc;

pub mod ffi {
    include!(concat!(env!("OUT_DIR"), "/nvml.rs"));
}

const NVML_SUCCESS_CODE: ffi::nvmlReturn_t = 0;
const NVML_FEATURE_ENABLED_CODE: c_uint = 1;

// NVML GPM enum values. They are enum constants in nvml.h, not preprocessor macros.
const GPM_PCIE_TX_PER_SEC: c_uint = 20;
const GPM_PCIE_RX_PER_SEC: c_uint = 21;

type NvmlInitV2 = unsafe extern "C" fn() -> ffi::nvmlReturn_t;
type NvmlShutdown = unsafe extern "C" fn() -> ffi::nvmlReturn_t;
type NvmlErrorString = unsafe extern "C" fn(ffi::nvmlReturn_t) -> *const c_char;
type NvmlDeviceGetCountV2 = unsafe extern "C" fn(*mut c_uint) -> ffi::nvmlReturn_t;
type NvmlDeviceGetHandleByIndexV2 =
    unsafe extern "C" fn(c_uint, *mut ffi::nvmlDevice_t) -> ffi::nvmlReturn_t;
type NvmlDeviceGetName =
    unsafe extern "C" fn(ffi::nvmlDevice_t, *mut c_char, c_uint) -> ffi::nvmlReturn_t;
type NvmlDeviceGetPciInfoV3 =
    unsafe extern "C" fn(ffi::nvmlDevice_t, *mut ffi::nvmlPciInfo_t) -> ffi::nvmlReturn_t;
type NvmlGpmQueryDeviceSupport =
    unsafe extern "C" fn(ffi::nvmlDevice_t, *mut ffi::nvmlGpmSupport_t) -> ffi::nvmlReturn_t;
type NvmlGpmQueryIfStreamingEnabled =
    unsafe extern "C" fn(ffi::nvmlDevice_t, *mut c_uint) -> ffi::nvmlReturn_t;
type NvmlGpmSetStreamingEnabled =
    unsafe extern "C" fn(ffi::nvmlDevice_t, c_uint) -> ffi::nvmlReturn_t;
type NvmlGpmSampleAlloc = unsafe extern "C" fn(*mut ffi::nvmlGpmSample_t) -> ffi::nvmlReturn_t;
type NvmlGpmSampleFree = unsafe extern "C" fn(ffi::nvmlGpmSample_t) -> ffi::nvmlReturn_t;
type NvmlGpmSampleGet =
    unsafe extern "C" fn(ffi::nvmlDevice_t, ffi::nvmlGpmSample_t) -> ffi::nvmlReturn_t;
type NvmlGpmMetricsGet =
    unsafe extern "C" fn(*mut ffi::nvmlGpmMetricsGet_t) -> ffi::nvmlReturn_t;

struct NvmlLib {
    _library: Library,
    init_v2: NvmlInitV2,
    shutdown: NvmlShutdown,
    error_string: NvmlErrorString,
    device_get_count_v2: NvmlDeviceGetCountV2,
    device_get_handle_by_index_v2: NvmlDeviceGetHandleByIndexV2,
    device_get_name: NvmlDeviceGetName,
    device_get_pci_info_v3: NvmlDeviceGetPciInfoV3,
    gpm_query_device_support: NvmlGpmQueryDeviceSupport,
    gpm_query_if_streaming_enabled: NvmlGpmQueryIfStreamingEnabled,
    gpm_set_streaming_enabled: NvmlGpmSetStreamingEnabled,
    gpm_sample_alloc: NvmlGpmSampleAlloc,
    gpm_sample_free: NvmlGpmSampleFree,
    gpm_sample_get: NvmlGpmSampleGet,
    gpm_metrics_get: NvmlGpmMetricsGet,
}

unsafe impl Send for NvmlLib {}
unsafe impl Sync for NvmlLib {}

impl NvmlLib {
    fn load() -> Result<Arc<Self>> {
        let library = unsafe { Library::new("libnvidia-ml.so.1") }
            .context("failed to load libnvidia-ml.so.1; is the NVIDIA driver installed?")?;

        unsafe {
            Ok(Arc::new(Self {
                init_v2: load_symbol(&library, b"nvmlInit_v2\0")?,
                shutdown: load_symbol(&library, b"nvmlShutdown\0")?,
                error_string: load_symbol(&library, b"nvmlErrorString\0")?,
                device_get_count_v2: load_symbol(&library, b"nvmlDeviceGetCount_v2\0")?,
                device_get_handle_by_index_v2: load_symbol(
                    &library,
                    b"nvmlDeviceGetHandleByIndex_v2\0",
                )?,
                device_get_name: load_symbol(&library, b"nvmlDeviceGetName\0")?,
                device_get_pci_info_v3: load_symbol(&library, b"nvmlDeviceGetPciInfo_v3\0")?,
                gpm_query_device_support: load_symbol(&library, b"nvmlGpmQueryDeviceSupport\0")?,
                gpm_query_if_streaming_enabled: load_symbol(
                    &library,
                    b"nvmlGpmQueryIfStreamingEnabled\0",
                )?,
                gpm_set_streaming_enabled: load_symbol(&library, b"nvmlGpmSetStreamingEnabled\0")?,
                gpm_sample_alloc: load_symbol(&library, b"nvmlGpmSampleAlloc\0")?,
                gpm_sample_free: load_symbol(&library, b"nvmlGpmSampleFree\0")?,
                gpm_sample_get: load_symbol(&library, b"nvmlGpmSampleGet\0")?,
                gpm_metrics_get: load_symbol(&library, b"nvmlGpmMetricsGet\0")?,
                _library: library,
            }))
        }
    }

    fn error(&self, code: ffi::nvmlReturn_t) -> String {
        unsafe {
            let p = (self.error_string)(code);
            if p.is_null() {
                return format!("NVML error code {code}");
            }
            CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    }

    fn check(&self, code: ffi::nvmlReturn_t, what: &str) -> Result<()> {
        if code == NVML_SUCCESS_CODE {
            Ok(())
        } else {
            Err(anyhow!("{}: {}", what, self.error(code)))
        }
    }
}

unsafe fn load_symbol<T: Copy>(library: &Library, name: &[u8]) -> Result<T> {
    let symbol: Symbol<T> = unsafe { library.get(name) }
        .with_context(|| format!("failed to load NVML symbol {}", String::from_utf8_lossy(name)))?;
    Ok(*symbol)
}

pub struct Nvml {
    lib: Arc<NvmlLib>,
}

impl Nvml {
    pub fn init() -> Result<Self> {
        let lib = NvmlLib::load()?;
        unsafe {
            lib.check((lib.init_v2)(), "nvmlInit_v2")?;
        }
        Ok(Self { lib })
    }

    pub fn device_count(&self) -> Result<u32> {
        let mut count: c_uint = 0;
        unsafe {
            self.lib.check(
                (self.lib.device_get_count_v2)(&mut count),
                "nvmlDeviceGetCount_v2",
            )?;
        }
        Ok(count as u32)
    }

    pub fn open_device(&self, index: u32) -> Result<GpmDevice> {
        GpmDevice::new(self.lib.clone(), index)
    }
}

impl Drop for Nvml {
    fn drop(&mut self) {
        unsafe {
            let _ = (self.lib.shutdown)();
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
    lib: Arc<NvmlLib>,
    pub meta: GpuMeta,
    handle: ffi::nvmlDevice_t,
    sample_prev: ffi::nvmlGpmSample_t,
    sample_now: ffi::nvmlGpmSample_t,
}

impl GpmDevice {
    fn new(lib: Arc<NvmlLib>, index: u32) -> Result<Self> {
        let mut handle: ffi::nvmlDevice_t = std::ptr::null_mut();

        unsafe {
            lib.check(
                (lib.device_get_handle_by_index_v2)(index as c_uint, &mut handle),
                "nvmlDeviceGetHandleByIndex_v2",
            )?;
        }

        let name = get_device_name(&lib, handle).unwrap_or_else(|_| format!("GPU{index}"));
        let pci_bus_id = get_pci_bus_id(&lib, handle).unwrap_or_else(|_| "unknown".to_string());

        init_gpm(&lib, handle, index)?;

        let mut sample_prev: ffi::nvmlGpmSample_t = std::ptr::null_mut();
        let mut sample_now: ffi::nvmlGpmSample_t = std::ptr::null_mut();

        unsafe {
            lib.check(
                (lib.gpm_sample_alloc)(&mut sample_prev),
                "nvmlGpmSampleAlloc(prev)",
            )?;
            lib.check(
                (lib.gpm_sample_alloc)(&mut sample_now),
                "nvmlGpmSampleAlloc(now)",
            )?;
            lib.check(
                (lib.gpm_sample_get)(handle, sample_prev),
                "initial nvmlGpmSampleGet",
            )?;
        }

        Ok(Self {
            lib,
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
            self.lib.check(
                (self.lib.gpm_sample_get)(self.handle, self.sample_now),
                "nvmlGpmSampleGet",
            )?;

            let mut get: ffi::nvmlGpmMetricsGet_t = mem::zeroed();
            get.version = ffi::NVML_GPM_METRICS_GET_VERSION;
            get.sample1 = self.sample_prev;
            get.sample2 = self.sample_now;
            get.numMetrics = 2;
            get.metrics[0].metricId = GPM_PCIE_TX_PER_SEC;
            get.metrics[1].metricId = GPM_PCIE_RX_PER_SEC;

            self.lib
                .check((self.lib.gpm_metrics_get)(&mut get), "nvmlGpmMetricsGet")?;

            let tx_status = get.metrics[0].nvmlReturn;
            let rx_status = get.metrics[1].nvmlReturn;

            if tx_status != NVML_SUCCESS_CODE {
                bail!("GPM TX metric failed: {}", self.lib.error(tx_status));
            }

            if rx_status != NVML_SUCCESS_CODE {
                bail!("GPM RX metric failed: {}", self.lib.error(rx_status));
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
                let _ = (self.lib.gpm_sample_free)(self.sample_prev);
            }
            if !self.sample_now.is_null() {
                let _ = (self.lib.gpm_sample_free)(self.sample_now);
            }
        }
    }
}

fn get_device_name(lib: &NvmlLib, handle: ffi::nvmlDevice_t) -> Result<String> {
    let mut buf = [0 as c_char; 128];
    unsafe {
        lib.check(
            (lib.device_get_name)(handle, buf.as_mut_ptr(), buf.len() as c_uint),
            "nvmlDeviceGetName",
        )?;
        Ok(CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned())
    }
}

fn get_pci_bus_id(lib: &NvmlLib, handle: ffi::nvmlDevice_t) -> Result<String> {
    unsafe {
        let mut pci: ffi::nvmlPciInfo_t = mem::zeroed();
        lib.check(
            (lib.device_get_pci_info_v3)(handle, &mut pci),
            "nvmlDeviceGetPciInfo_v3",
        )?;
        Ok(CStr::from_ptr(pci.busId.as_ptr()).to_string_lossy().into_owned())
    }
}

fn init_gpm(lib: &NvmlLib, handle: ffi::nvmlDevice_t, index: u32) -> Result<()> {
    unsafe {
        let mut support: ffi::nvmlGpmSupport_t = mem::zeroed();
        support.version = ffi::NVML_GPM_SUPPORT_VERSION;

        lib.check(
            (lib.gpm_query_device_support)(handle, &mut support),
            "nvmlGpmQueryDeviceSupport",
        )?;

        if support.isSupportedDevice == 0 {
            bail!("GPU {index} does not support NVML GPM");
        }

        let mut state: c_uint = 0;
        lib.check(
            (lib.gpm_query_if_streaming_enabled)(handle, &mut state),
            "nvmlGpmQueryIfStreamingEnabled",
        )?;

        if state != NVML_FEATURE_ENABLED_CODE {
            let r = (lib.gpm_set_streaming_enabled)(handle, NVML_FEATURE_ENABLED_CODE);
            if r != NVML_SUCCESS_CODE {
                bail!(
                    "failed to enable GPM streaming on GPU {index}: {}; try `sudo nvidia-smi gpm -i {index} -s 1`",
                    lib.error(r),
                );
            }
        }
    }

    Ok(())
}
