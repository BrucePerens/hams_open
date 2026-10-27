// SPDX-License-Identifier: AGPL-3.0-or-later
//! Software defined radios (a Nuand bladeRF 1 with an optional XB-200, a LimeSDR-USB) through SoapySDR. The library is loaded at
//! run time (`dlopen`), so the server builds and runs without it and answers "SoapySDR is not installed" for these tools. Only
//! receive is implemented: the tools tune, capture complex baseband, and analyse it (`dsp`). Nothing here transmits.
//! Levels are dB relative to the radio's full scale (dBFS), not calibrated power: they depend on the gain setting.

use crate::dsp::{self, Iq};
use serde_json::{json, Value};
use std::ffi::{c_char, c_int, c_long, c_void, CStr, CString};
use std::path::Path;
use std::sync::OnceLock;
use std::time::Instant;

/// SoapySDR direction constants.
const RX: c_int = 1;

/// Which radio, and what its tools may ask of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// "bladerf" or "limesdr" (the config file's driver name).
    pub driver: String,
    /// Hex serial (or any prefix of it) to pick one of several boards.
    pub serial: Option<String>,
    /// bladeRF only: the XB-200 transverter setting passed to SoapySDR ("50M", "144M", "222M", "auto", "auto1db", "auto3db", "custom").
    pub xb200: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            driver: "bladerf".into(),
            serial: None,
            xb200: None,
        }
    }
}

const XB200_SETTINGS: &[&str] = &[
    "50M", "144M", "222M", "auto", "auto1db", "auto3db", "custom",
];

/// The settings of a radio that are checked when the configuration is read.
pub fn check_config(c: &Config) -> Result<(), String> {
    if !matches!(c.driver.as_str(), "bladerf" | "limesdr") {
        return Err(format!("unknown SDR driver {:?}", c.driver));
    }
    if let Some(s) = &c.serial {
        if s.is_empty() || !s.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return Err(format!("serial {s:?} must be hexadecimal digits"));
        }
    }
    if let Some(x) = &c.xb200 {
        if c.driver != "bladerf" {
            return Err("xb200 applies to the bladeRF only".into());
        }
        if !XB200_SETTINGS.contains(&x.as_str()) {
            return Err(format!(
                "xb200 {x:?} must be one of {}",
                XB200_SETTINGS.join(", ")
            ));
        }
    }
    Ok(())
}

/// The SoapySDR argument string that opens this radio.
pub fn soapy_args(c: &Config) -> String {
    // [@ANCHOR: bench_mcp_sdr_args]
    let mut s = String::from(if c.driver == "limesdr" {
        "driver=lime"
    } else {
        "driver=bladerf"
    });
    if let Some(x) = &c.xb200 {
        s.push_str(&format!(",xb200={x}"));
    }
    if let Some(x) = &c.serial {
        s.push_str(&format!(",serial={x}"));
    }
    s
}

/// The tuning and rate limits the tools enforce before the radio is touched.
struct Limits {
    min_hz: f64,
    max_hz: f64,
    max_rate: f64,
}

fn limits(c: &Config) -> Limits {
    if c.driver == "limesdr" {
        Limits {
            min_hz: 30e6,
            max_hz: 3.8e9,
            max_rate: 30.72e6,
        }
    } else {
        Limits {
            min_hz: if c.xb200.is_some() { 9e3 } else { 237.5e6 },
            max_hz: 3.8e9,
            max_rate: 20e6,
        }
    }
}

fn number(args: &Value, key: &str) -> Result<Option<f64>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_f64()
            .filter(|x| x.is_finite())
            .map(Some)
            .ok_or(format!("{key} must be a number")),
    }
}

fn whole(args: &Value, key: &str, default: usize, min: usize, max: usize) -> Result<usize, String> {
    match number(args, key)? {
        None => Ok(default),
        Some(x) if x.fract() == 0.0 && x >= min as f64 && x <= max as f64 => Ok(x as usize),
        Some(x) => Err(format!(
            "{key} {x} must be a whole number from {min} to {max}"
        )),
    }
}

fn in_range(name: &str, x: f64, lo: f64, hi: f64) -> Result<f64, String> {
    if x >= lo && x <= hi {
        Ok(x)
    } else {
        Err(format!("{name} {x} is outside {lo} to {hi}"))
    }
}

/// A tuned capture: centre frequency, sample rate, gain, length and channels.
#[derive(Debug, Clone, PartialEq)]
pub struct Capture {
    pub freq_hz: f64,
    pub rate_hz: f64,
    pub gain_db: f64,
    pub samples: usize,
    pub channels: Vec<usize>,
    /// The LimeSDR receive input ("LNAH", "LNAL" or "LNAW"); `None` leaves the driver's default, which is LNAL.
    pub antenna: Option<String>,
}

const MAX_SAMPLES: usize = 1 << 24;

/// The LimeSDR receive inputs the tools may select: the H, L and W connectors of each channel.
const LIME_INPUTS: &[&str] = &["LNAH", "LNAL", "LNAW"];

/// The `antenna` argument: which receive input to use. Only the LimeSDR has a choice; the bladeRF has one receive input.
fn antenna_arg(args: &Value, c: &Config) -> Result<Option<String>, String> {
    match args.get("antenna") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let name = v
                .as_str()
                .ok_or("antenna must be a string")?
                .to_ascii_uppercase();
            if c.driver != "limesdr" {
                return Err("the bladeRF has one receive input; leave antenna out".into());
            }
            if !LIME_INPUTS.contains(&name.as_str()) {
                return Err(format!(
                    "antenna {name:?} must be one of {}",
                    LIME_INPUTS.join(", ")
                ));
            }
            Ok(Some(name))
        }
    }
}

/// The arguments of a capture call, checked against the radio's limits.
pub fn capture_args(args: &Value, c: &Config) -> Result<Capture, String> {
    // [@ANCHOR: bench_mcp_sdr_capture_args]
    let l = limits(c);
    let freq_hz = number(args, "freq_hz")?.ok_or("freq_hz is required")?;
    in_range("freq_hz", freq_hz, l.min_hz, l.max_hz)?;
    let rate_hz = number(args, "sample_rate_hz")?.unwrap_or(2e6);
    in_range("sample_rate_hz", rate_hz, 0.25e6, l.max_rate)?;
    let gain_db = number(args, "gain_db")?.unwrap_or(30.0);
    in_range("gain_db", gain_db, 0.0, 70.0)?;
    let samples = whole(args, "samples", 1 << 18, 4096, MAX_SAMPLES)?;
    let channel = whole(args, "channel", 0, 0, 1)?;
    if channel == 1 && c.driver != "limesdr" {
        return Err("this radio has one receive channel (0)".into());
    }
    Ok(Capture {
        freq_hz,
        rate_hz,
        gain_db,
        samples,
        channels: vec![channel],
        antenna: antenna_arg(args, c)?,
    })
}

/// A swept survey: every step is one capture, stitched into one spectrum.
#[derive(Debug, Clone, PartialEq)]
pub struct Survey {
    pub start_hz: f64,
    pub stop_hz: f64,
    pub rate_hz: f64,
    pub gain_db: f64,
    pub fft_len: usize,
    pub samples_per_step: usize,
    pub antenna: Option<String>,
}

/// The fraction of each capture's bandwidth that is kept (the band edges are rolled off by the radio's filters).
pub const USABLE: f64 = 0.7;
const MAX_STEPS: usize = 400;

impl Survey {
    pub fn steps(&self) -> usize {
        (((self.stop_hz - self.start_hz) / (self.rate_hz * USABLE)).ceil() as usize).max(1)
    }
}

pub fn survey_args(args: &Value, c: &Config) -> Result<Survey, String> {
    // [@ANCHOR: bench_mcp_sdr_survey_args]
    let l = limits(c);
    let start_hz = number(args, "start_hz")?.ok_or("start_hz is required")?;
    let stop_hz = number(args, "stop_hz")?.unwrap_or(start_hz + 1e6);
    in_range("start_hz", start_hz, l.min_hz, l.max_hz)?;
    in_range("stop_hz", stop_hz, l.min_hz, l.max_hz)?;
    if stop_hz <= start_hz {
        return Err("stop_hz must be above start_hz".into());
    }
    let rate_hz = number(args, "sample_rate_hz")?.unwrap_or(10e6);
    in_range("sample_rate_hz", rate_hz, 0.25e6, l.max_rate)?;
    let gain_db = number(args, "gain_db")?.unwrap_or(30.0);
    in_range("gain_db", gain_db, 0.0, 70.0)?;
    let fft_len = whole(args, "fft_len", 4096, 64, 1 << 16)?;
    if !fft_len.is_power_of_two() {
        return Err(format!("fft_len {fft_len} must be a power of two"));
    }
    let samples_per_step = whole(args, "samples_per_step", 1 << 18, fft_len * 4, MAX_SAMPLES)?;
    let s = Survey {
        start_hz,
        stop_hz,
        rate_hz,
        gain_db,
        fft_len,
        samples_per_step,
        antenna: antenna_arg(args, c)?,
    };
    if s.steps() > MAX_STEPS {
        return Err(format!(
            "{start_hz} to {stop_hz} Hz needs {} steps at {rate_hz} Hz; at most {MAX_STEPS} (raise sample_rate_hz or narrow the span)",
            s.steps()
        ));
    }
    Ok(s)
}

// ---- SoapySDR, loaded at run time -------------------------------------------------------------------------------------

#[repr(C)]
struct Kwargs {
    size: usize,
    keys: *mut *mut c_char,
    vals: *mut *mut c_char,
}

type Dev = *mut c_void;
type Stream = *mut c_void;

struct Api {
    make: unsafe extern "C" fn(*const c_char) -> Dev,
    unmake: unsafe extern "C" fn(Dev) -> c_int,
    set_rate: unsafe extern "C" fn(Dev, c_int, usize, f64) -> c_int,
    set_freq: unsafe extern "C" fn(Dev, c_int, usize, f64, *const Kwargs) -> c_int,
    set_gain: unsafe extern "C" fn(Dev, c_int, usize, f64) -> c_int,
    set_antenna: unsafe extern "C" fn(Dev, c_int, usize, *const c_char) -> c_int,
    setup_stream: unsafe extern "C" fn(
        Dev,
        c_int,
        *const c_char,
        *const usize,
        usize,
        *const Kwargs,
    ) -> Stream,
    close_stream: unsafe extern "C" fn(Dev, Stream) -> c_int,
    activate: unsafe extern "C" fn(Dev, Stream, c_int, i64, usize) -> c_int,
    deactivate: unsafe extern "C" fn(Dev, Stream, c_int, i64) -> c_int,
    read: unsafe extern "C" fn(
        Dev,
        Stream,
        *const *mut c_void,
        usize,
        *mut c_int,
        *mut i64,
        c_long,
    ) -> c_int,
    hw_key: unsafe extern "C" fn(Dev) -> *mut c_char,
    driver_key: unsafe extern "C" fn(Dev) -> *mut c_char,
    hw_info: unsafe extern "C" fn(Dev) -> Kwargs,
    num_channels: unsafe extern "C" fn(Dev, c_int) -> usize,
    last_error: unsafe extern "C" fn() -> *const c_char,
    free: unsafe extern "C" fn(*mut c_void),
    kwargs_clear: unsafe extern "C" fn(*mut Kwargs),
}

fn load_api() -> Result<Api, String> {
    let mut h = std::ptr::null_mut();
    for name in ["libSoapySDR.so.0.8", "libSoapySDR.so.0.7", "libSoapySDR.so"] {
        let c = CString::new(name).unwrap();
        h = unsafe { libc::dlopen(c.as_ptr(), libc::RTLD_NOW) };
        if !h.is_null() {
            break;
        }
    }
    if h.is_null() {
        return Err("SoapySDR is not installed (libSoapySDR.so not found); install libsoapysdr0.8 and the module for the radio".into());
    }
    fn sym<T: Copy>(h: *mut c_void, name: &str) -> Result<T, String> {
        assert_eq!(std::mem::size_of::<T>(), std::mem::size_of::<*mut c_void>());
        let c = CString::new(name).unwrap();
        let p = unsafe { libc::dlsym(h, c.as_ptr()) };
        if p.is_null() {
            return Err(format!("libSoapySDR has no {name}"));
        }
        // SAFETY: T is one of the extern "C" function pointer types of `Api`, the same size as a data pointer, and the symbol
        // has the documented SoapySDR signature.
        Ok(unsafe { std::mem::transmute_copy::<*mut c_void, T>(&p) })
    }
    Ok(Api {
        make: sym(h, "SoapySDRDevice_makeStrArgs")?,
        unmake: sym(h, "SoapySDRDevice_unmake")?,
        set_rate: sym(h, "SoapySDRDevice_setSampleRate")?,
        set_freq: sym(h, "SoapySDRDevice_setFrequency")?,
        set_gain: sym(h, "SoapySDRDevice_setGain")?,
        set_antenna: sym(h, "SoapySDRDevice_setAntenna")?,
        setup_stream: sym(h, "SoapySDRDevice_setupStream")?,
        close_stream: sym(h, "SoapySDRDevice_closeStream")?,
        activate: sym(h, "SoapySDRDevice_activateStream")?,
        deactivate: sym(h, "SoapySDRDevice_deactivateStream")?,
        read: sym(h, "SoapySDRDevice_readStream")?,
        hw_key: sym(h, "SoapySDRDevice_getHardwareKey")?,
        driver_key: sym(h, "SoapySDRDevice_getDriverKey")?,
        hw_info: sym(h, "SoapySDRDevice_getHardwareInfo")?,
        num_channels: sym(h, "SoapySDRDevice_getNumChannels")?,
        last_error: sym(h, "SoapySDRDevice_lastError")?,
        free: sym(h, "SoapySDR_free")?,
        kwargs_clear: sym(h, "SoapySDRKwargs_clear")?,
    })
}

fn api() -> Result<&'static Api, String> {
    static API: OnceLock<Result<Api, String>> = OnceLock::new();
    API.get_or_init(load_api).as_ref().map_err(|e| e.clone())
}

fn last_error(a: &Api) -> String {
    let p = unsafe { (a.last_error)() };
    if p.is_null() {
        "unknown SoapySDR error".into()
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

/// An open radio, closed when dropped.
struct Radio {
    a: &'static Api,
    dev: Dev,
}

impl Radio {
    fn open(c: &Config) -> Result<Radio, String> {
        let a = api()?;
        let args = CString::new(soapy_args(c)).map_err(|e| e.to_string())?;
        let dev = unsafe { (a.make)(args.as_ptr()) };
        if dev.is_null() {
            return Err(format!(
                "cannot open the {} ({}): {}",
                c.driver,
                soapy_args(c),
                last_error(a)
            ));
        }
        Ok(Radio { a, dev })
    }

    fn check(&self, what: &str, rc: c_int) -> Result<(), String> {
        if rc == 0 {
            Ok(())
        } else {
            Err(format!("{what}: {}", last_error(self.a)))
        }
    }

    fn rx_channels(&self) -> usize {
        unsafe { (self.a.num_channels)(self.dev, RX) }
    }

    fn string(&self, p: *mut c_char) -> String {
        if p.is_null() {
            return String::new();
        }
        let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
        unsafe { (self.a.free)(p.cast()) };
        s
    }

    /// Tune, then read `cap.samples` complex samples on each channel (the first samples after start are dropped while the
    /// front end settles). Returns the channels' samples and the number of overflows the driver reported.
    fn capture(&self, cap: &Capture) -> Result<(Vec<Vec<Iq>>, usize, f64), String> {
        let a = self.a;
        for &ch in &cap.channels {
            if let Some(name) = &cap.antenna {
                let c = CString::new(name.as_str()).map_err(|e| e.to_string())?;
                self.check("antenna", unsafe {
                    (a.set_antenna)(self.dev, RX, ch, c.as_ptr())
                })?;
            }
            self.check("sample rate", unsafe {
                (a.set_rate)(self.dev, RX, ch, cap.rate_hz)
            })?;
            self.check("gain", unsafe {
                (a.set_gain)(self.dev, RX, ch, cap.gain_db)
            })?;
            self.check("frequency", unsafe {
                (a.set_freq)(self.dev, RX, ch, cap.freq_hz, std::ptr::null())
            })?;
        }
        let fmt = CString::new("CF32").unwrap();
        let stream = unsafe {
            (a.setup_stream)(
                self.dev,
                RX,
                fmt.as_ptr(),
                cap.channels.as_ptr(),
                cap.channels.len(),
                std::ptr::null(),
            )
        };
        if stream.is_null() {
            return Err(format!("stream setup: {}", last_error(a)));
        }
        let result = self.read_stream(stream, cap);
        unsafe {
            (a.deactivate)(self.dev, stream, 0, 0);
            (a.close_stream)(self.dev, stream);
        }
        result
    }

    fn read_stream(
        &self,
        stream: Stream,
        cap: &Capture,
    ) -> Result<(Vec<Vec<Iq>>, usize, f64), String> {
        let a = self.a;
        self.check("activate", unsafe {
            (a.activate)(self.dev, stream, 0, 0, 0)
        })?;
        let nch = cap.channels.len();
        let settle = 8192usize.min(cap.samples);
        let total = cap.samples + settle;
        let mut bufs: Vec<Vec<f32>> = vec![vec![0.0; total * 2]; nch];
        let (mut got, mut overflows, mut timeouts) = (0usize, 0usize, 0u32);
        let started = Instant::now();
        while got < total {
            let want = (total - got).min(65536);
            let ptrs: Vec<*mut c_void> = bufs
                .iter_mut()
                .map(|b| b[got * 2..].as_mut_ptr().cast())
                .collect();
            let (mut flags, mut t) = (0 as c_int, 0i64);
            let n = unsafe {
                (a.read)(
                    self.dev,
                    stream,
                    ptrs.as_ptr(),
                    want,
                    &mut flags,
                    &mut t,
                    1_000_000,
                )
            };
            match n {
                n if n > 0 => got += n as usize,
                -1 => {
                    timeouts += 1;
                    if timeouts > 5 {
                        return Err(format!(
                            "the radio delivered no samples (5 read timeouts after {got} samples)"
                        ));
                    }
                }
                -4 => overflows += 1,
                e => return Err(format!("read error {e}: {}", last_error(a))),
            }
            if started.elapsed().as_secs() > 120 {
                return Err("capture took longer than 120 s".into());
            }
        }
        let out = bufs
            .iter()
            .map(|b| (settle..total).map(|i| (b[2 * i], b[2 * i + 1])).collect())
            .collect();
        Ok((out, overflows, started.elapsed().as_secs_f64()))
    }
}

impl Drop for Radio {
    fn drop(&mut self) {
        unsafe { (self.a.unmake)(self.dev) };
    }
}

// ---- the tools -----------------------------------------------------------------------------------------------------

fn db10(x: f64) -> f64 {
    10.0 * x.max(1e-30).log10()
}

fn round(x: f64, places: i32) -> f64 {
    let p = 10f64.powi(places);
    (x * p).round() / p
}

/// Identity and state of the radio: the driver's hardware key and info (firmware and gateware versions), channel counts.
pub fn identify(c: &Config) -> Result<Value, String> {
    let r = Radio::open(c)?;
    let a = r.a;
    let key = r.string(unsafe { (a.hw_key)(r.dev) });
    let drv = r.string(unsafe { (a.driver_key)(r.dev) });
    let mut info = unsafe { (a.hw_info)(r.dev) };
    let mut m = serde_json::Map::new();
    for i in 0..info.size {
        let (k, v) = unsafe {
            (
                CStr::from_ptr(*info.keys.add(i))
                    .to_string_lossy()
                    .into_owned(),
                CStr::from_ptr(*info.vals.add(i))
                    .to_string_lossy()
                    .into_owned(),
            )
        };
        m.insert(k, Value::String(v));
    }
    unsafe { (a.kwargs_clear)(&mut info) };
    Ok(json!({
        "driver": c.driver,
        "soapy_args": soapy_args(c),
        "soapy_driver": drv,
        "hardware": key,
        "info": m,
        "rx_channels": r.rx_channels(),
    }))
}

/// Whether samples arrived at the sample rate, so none were dropped: losing samples makes the streaming take longer than the signal
/// lasts (the time to tune, which the LimeSDR spends over a second on, is not counted).
/// The bladeRF with a newer FPGA than libbladeRF knows reports an overrun about every 504 samples even when none are lost, so the
/// reported overflow count alone is not trusted; this timing is.
pub fn continuous(capture_s: f64, real_s: f64) -> bool {
    // [@ANCHOR: bench_mcp_sdr_continuous]
    capture_s <= real_s * 1.04 + 0.1
}

fn write_iq(path: &Path, samples: &[Iq]) -> Result<(), String> {
    let mut bytes = Vec::with_capacity(samples.len() * 8);
    for &(i, q) in samples {
        bytes.extend_from_slice(&i.to_le_bytes());
        bytes.extend_from_slice(&q.to_le_bytes());
    }
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn stamp() -> String {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    t.to_string()
}

/// Capture one block, save the raw I/Q (little-endian f32 pairs) and return its statistics, floor and strongest peaks.
pub fn capture(c: &Config, args: &Value, out: &Path) -> Result<Value, String> {
    let cap = capture_args(args, c)?;
    let r = Radio::open(c)?;
    let t0 = Instant::now();
    let (chans, overflows, capture_s) = r.capture(&cap)?;
    let real_s = (cap.samples + 8192) as f64 / cap.rate_hz;
    let x = &chans[0];
    let (rms, peak, dc, clip) = dsp::stats(x);
    let fft_len = 4096.min(x.len().next_power_of_two() / 2).max(64);
    let spec = dsp::welch(x, fft_len, cap.rate_hz)?;
    let peaks: Vec<Value> = dsp::peaks(&spec, 8, 15.0, 8, 3)
        .iter()
        .map(|p| json!({"freq_hz": round(cap.freq_hz + p.offset_hz, 1), "power_dbfs": round(p.power_db, 1)}))
        .collect();
    let file = out.join(format!(
        "sdr_{}_{}_{}.cf32",
        c.driver,
        cap.freq_hz as u64,
        stamp()
    ));
    write_iq(&file, x)?;
    Ok(json!({
        "freq_hz": cap.freq_hz,
        "sample_rate_hz": cap.rate_hz,
        "gain_db": cap.gain_db,
        "samples": x.len(),
        "channel": cap.channels[0],
        "antenna": cap.antenna.clone().unwrap_or_else(|| "driver default (LNAL on the LimeSDR)".into()),
        "rms_dbfs": round(db10(rms * rms), 1),
        "peak_amplitude": round(peak, 4),
        "dc_offset": [round(dc.0, 5), round(dc.1, 5)],
        "clipped_samples": clip,
        "overflows_reported_by_driver": overflows,
        "noise_floor_dbfs": round(dsp::noise_floor_db(&spec), 1),
        "fft_bin_hz": round(spec.bin_hz, 1),
        "peaks": peaks,
        "iq_file": file.display().to_string(),
        "iq_format": "little-endian f32 I,Q pairs, full scale 1.0",
        "stream_seconds": round(capture_s, 3),
        "real_time_seconds": round(real_s, 3),
        "continuous": continuous(capture_s, real_s),
        "seconds": round(t0.elapsed().as_secs_f64(), 2),
        "note": "levels are dBFS at the set gain, not calibrated power; the DC spike at the centre frequency is the radio's own and is ignored in peaks",
    }))
}

/// A swept power survey from `start_hz` to `stop_hz`, saved as CSV, with the floor and the strongest peaks.
pub fn survey(c: &Config, args: &Value, out: &Path) -> Result<Value, String> {
    let sv = survey_args(args, c)?;
    let r = Radio::open(c)?;
    let t0 = Instant::now();
    let step_hz = sv.rate_hz * USABLE;
    let steps = sv.steps();
    let mut freqs: Vec<f64> = Vec::new();
    let mut power: Vec<f64> = Vec::new();
    let mut overflows = 0usize;
    for i in 0..steps {
        let centre = sv.start_hz + step_hz * (i as f64 + 0.5);
        let cap = Capture {
            freq_hz: centre,
            rate_hz: sv.rate_hz,
            gain_db: sv.gain_db,
            samples: sv.samples_per_step,
            channels: vec![0],
            antenna: sv.antenna.clone(),
        };
        let (chans, o, _) = r.capture(&cap)?;
        overflows += o;
        let spec = dsp::welch(&chans[0], sv.fft_len, sv.rate_hz)?;
        let mid = sv.fft_len / 2;
        let half = (step_hz / spec.bin_hz / 2.0) as usize;
        // The radio's own DC spike at the middle of every step is replaced by its neighbours so it is not read as a signal.
        let mut p: Vec<f64> = spec.power_db.clone();
        let (lo, hi) = (p[mid - 4], p[mid + 4]);
        for (k, v) in p.iter_mut().enumerate().take(mid + 4).skip(mid - 3) {
            let t = (k + 4 - mid) as f64 / 8.0;
            *v = lo + (hi - lo) * t;
        }
        for (k, &v) in p.iter().enumerate().take(mid + half).skip(mid - half) {
            freqs.push(centre + spec.offset_hz(k));
            power.push(v);
        }
    }
    let spec = dsp::Spectrum {
        bin_hz: sv.rate_hz / sv.fft_len as f64,
        power_db: power.clone(),
    };
    let floor = dsp::noise_floor_db(&spec);
    let peaks: Vec<Value> = dsp::peaks(&spec, 10, 10.0, 4, 0)
        .iter()
        .map(|pk| {
            let i = (pk.offset_hz / spec.bin_hz + (power.len() / 2) as f64).round() as usize;
            json!({"freq_hz": round(freqs[i.min(freqs.len() - 1)], 1), "power_dbfs": round(pk.power_db, 1)})
        })
        .collect();
    let mut csv = String::from("freq_hz,power_dbfs\n");
    for (f, p) in freqs.iter().zip(&power) {
        csv.push_str(&format!("{:.1},{:.2}\n", f, p));
    }
    let file = out.join(format!("sdr_survey_{}_{}.csv", c.driver, stamp()));
    std::fs::write(&file, csv).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(json!({
        "start_hz": sv.start_hz,
        "stop_hz": sv.stop_hz,
        "steps": steps,
        "sample_rate_hz": sv.rate_hz,
        "gain_db": sv.gain_db,
        "antenna": sv.antenna.clone().unwrap_or_else(|| "driver default (LNAL on the LimeSDR)".into()),
        "bin_hz": round(sv.rate_hz / sv.fft_len as f64, 1),
        "points": power.len(),
        "median_floor_dbfs": round(floor, 1),
        "peaks": peaks,
        "overflows": overflows,
        "csv_file": file.display().to_string(),
        "seconds": round(t0.elapsed().as_secs_f64(), 1),
        "note": "levels are dBFS at the set gain, not calibrated power; steps are stitched, so a level step at a step boundary (every rate*0.7 Hz) is the radio's response edge",
    }))
}

/// Phase and gain between the two receive channels of a radio that has them, after a fresh tune each repeat.
pub fn coherence(c: &Config, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_sdr_coherence_args]
    let l = limits(c);
    let freq_hz = number(args, "freq_hz")?.ok_or("freq_hz is required")?;
    in_range("freq_hz", freq_hz, l.min_hz, l.max_hz)?;
    let rate_hz = number(args, "sample_rate_hz")?.unwrap_or(2e6);
    in_range("sample_rate_hz", rate_hz, 0.25e6, l.max_rate)?;
    let gain_db = number(args, "gain_db")?.unwrap_or(30.0);
    in_range("gain_db", gain_db, 0.0, 70.0)?;
    let repeats = whole(args, "repeats", 4, 1, 10)?;
    let r = Radio::open(c)?;
    if r.rx_channels() < 2 {
        return Err(format!(
            "the {} has {} receive channel; coherence needs two",
            c.driver,
            r.rx_channels()
        ));
    }
    let cap = Capture {
        freq_hz,
        rate_hz,
        gain_db,
        samples: 1 << 18,
        channels: vec![0, 1],
        antenna: antenna_arg(args, c)?,
    };
    let mut runs = Vec::new();
    let mut phases = Vec::new();
    for _ in 0..repeats {
        let (ch, _, _) = r.capture(&cap)?;
        let rep = dsp::phase_between(&ch[0], &ch[1], 4096, rate_hz, 3)?;
        phases.push(rep.phase_deg);
        runs.push(json!({
            "tone_offset_hz": round(rep.tone_offset_hz, 1),
            "phase_deg": round(rep.phase_deg, 2),
            "phase_std_within_capture_deg": round(rep.phase_std_deg, 2),
            "gain_db": round(rep.gain_db, 2),
            "coherence": round(rep.coherence, 4),
            "blocks": rep.blocks,
        }));
    }
    let (sx, sy) = phases.iter().fold((0.0, 0.0), |a, p| {
        (a.0 + p.to_radians().cos(), a.1 + p.to_radians().sin())
    });
    let rr = (sx * sx + sy * sy).sqrt() / phases.len() as f64;
    Ok(json!({
        "freq_hz": freq_hz,
        "runs": runs,
        "mean_phase_deg": round(sy.atan2(sx).to_degrees(), 2),
        "phase_std_across_retunes_deg": round((-2.0 * rr.max(1e-12).ln()).max(0.0).sqrt().to_degrees(), 2),
        "note": "needs one signal on both inputs (a splitter); with different or no signals the numbers are meaningless, check coherence near 1",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lime() -> Config {
        Config {
            driver: "limesdr".into(),
            ..Config::default()
        }
    }

    fn blade(xb: Option<&str>) -> Config {
        Config {
            xb200: xb.map(String::from),
            ..Config::default()
        }
    }

    // Tests [@ANCHOR: bench_mcp_sdr_args]
    #[test]
    fn the_soapy_argument_string_and_config_checks() {
        assert_eq!(soapy_args(&lime()), "driver=lime");
        assert_eq!(
            soapy_args(&Config {
                serial: Some("ca3e".into()),
                xb200: Some("auto1db".into()),
                ..Config::default()
            }),
            "driver=bladerf,xb200=auto1db,serial=ca3e"
        );
        assert!(check_config(&blade(Some("auto"))).is_ok());
        assert!(check_config(&blade(Some("enabled"))).is_err());
        assert!(check_config(&Config {
            xb200: Some("auto".into()),
            ..lime()
        })
        .is_err());
        // A serial may not smuggle another SoapySDR argument.
        let bad = Config {
            serial: Some("ab,driver=audio".into()),
            ..Config::default()
        };
        assert!(check_config(&bad).is_err());
        assert!(check_config(&Config {
            driver: "hackrf".into(),
            ..Config::default()
        })
        .is_err());
    }

    // Tests [@ANCHOR: bench_mcp_sdr_capture_args]
    #[test]
    fn capture_arguments_are_checked_against_each_radios_limits() {
        let ok = capture_args(&json!({"freq_hz": 433.92e6}), &blade(None)).unwrap();
        assert_eq!((ok.rate_hz, ok.gain_db, ok.samples), (2e6, 30.0, 1 << 18));
        // Below 237.5 MHz the bladeRF needs its XB-200.
        assert!(capture_args(&json!({"freq_hz": 7.1e6}), &blade(None)).is_err());
        assert!(capture_args(&json!({"freq_hz": 7.1e6}), &blade(Some("auto"))).is_ok());
        assert!(capture_args(&json!({"freq_hz": 3.9e9}), &lime()).is_err());
        assert!(capture_args(&json!({"freq_hz": 100e6, "sample_rate_hz": 40e6}), &lime()).is_err());
        assert!(capture_args(&json!({"freq_hz": 100e6, "gain_db": 90}), &lime()).is_err());
        assert!(capture_args(&json!({"freq_hz": 100e6, "samples": 10}), &lime()).is_err());
        assert!(capture_args(&json!({"freq_hz": 100e6, "samples": 5000.5}), &lime()).is_err());
        assert!(capture_args(&json!({}), &lime()).is_err());
        assert!(capture_args(&json!({"freq_hz": "100e6"}), &lime()).is_err());
        // The second channel exists on the LimeSDR only.
        assert!(capture_args(&json!({"freq_hz": 100e6, "channel": 1}), &blade(None)).is_err());
        assert_eq!(
            capture_args(&json!({"freq_hz": 100e6, "channel": 1}), &lime())
                .unwrap()
                .channels,
            vec![1]
        );
    }

    // Tests [@ANCHOR: bench_mcp_sdr_continuous]
    #[test]
    fn a_capture_that_takes_longer_than_its_signal_lasts_lost_samples() {
        assert!(continuous(2.147, 2.099));
        assert!(!continuous(2.36, 2.099), "an 11 % loss must be flagged");
        assert!(
            continuous(0.17, 0.13),
            "short captures include the tuning time"
        );
    }

    #[test]
    fn the_receive_input_can_be_chosen_on_the_lime_only() {
        let a = |driver: &Config, v: &str| {
            capture_args(&json!({"freq_hz": 2.4e9, "antenna": v}), driver)
        };
        assert_eq!(a(&lime(), "lnah").unwrap().antenna.as_deref(), Some("LNAH"));
        assert_eq!(a(&lime(), "LNAW").unwrap().antenna.as_deref(), Some("LNAW"));
        assert!(
            a(&lime(), "LB1").is_err(),
            "the loopback inputs are not offered"
        );
        assert!(a(&lime(), "RX1_H").is_err());
        assert!(a(&blade(None), "LNAL").is_err());
        assert_eq!(
            capture_args(&json!({"freq_hz": 2.4e9}), &lime())
                .unwrap()
                .antenna,
            None
        );
        let s = survey_args(&json!({"start_hz": 2.4e9, "antenna": "LNAH"}), &lime()).unwrap();
        assert_eq!(s.antenna.as_deref(), Some("LNAH"));
        assert!(survey_args(&json!({"start_hz": 2.4e9, "antenna": 3}), &lime()).is_err());
    }

    // Tests [@ANCHOR: bench_mcp_sdr_survey_args]
    #[test]
    fn survey_arguments_bound_the_number_of_steps() {
        let s = survey_args(&json!({"start_hz": 88e6, "stop_hz": 108e6}), &lime()).unwrap();
        assert_eq!(s.steps(), 3);
        assert!(survey_args(&json!({"start_hz": 108e6, "stop_hz": 88e6}), &lime()).is_err());
        // 30 MHz to 3.8 GHz at 10 MS/s is 540 steps: refused.
        assert!(survey_args(&json!({"start_hz": 30e6, "stop_hz": 3.8e9}), &lime()).is_err());
        assert!(survey_args(
            &json!({"start_hz": 30e6, "stop_hz": 3.8e9, "sample_rate_hz": 30e6}),
            &lime()
        )
        .is_ok());
        assert!(survey_args(
            &json!({"start_hz": 100e6, "stop_hz": 200e6, "fft_len": 1000}),
            &lime()
        )
        .is_err());
        assert!(survey_args(&json!({"start_hz": 5e6, "stop_hz": 6e6}), &blade(None)).is_err());
        assert_eq!(
            survey_args(&json!({"start_hz": 5e6}), &blade(Some("auto")))
                .unwrap()
                .steps(),
            1
        );
    }

    // Tests [@ANCHOR: bench_mcp_sdr_coherence_args]
    #[test]
    fn coherence_checks_its_arguments_before_touching_hardware() {
        let e = coherence(&lime(), &json!({})).unwrap_err();
        assert!(e.contains("freq_hz"), "{e}");
        let e = coherence(&lime(), &json!({"freq_hz": 1e6})).unwrap_err();
        assert!(e.contains("freq_hz"), "{e}");
        let e = coherence(&lime(), &json!({"freq_hz": 100e6, "repeats": 50})).unwrap_err();
        assert!(e.contains("repeats"), "{e}");
    }
}
