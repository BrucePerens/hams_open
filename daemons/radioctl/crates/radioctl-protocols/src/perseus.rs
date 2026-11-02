// SPDX-License-Identifier: AGPL-3.0-or-later
//! Microtelecom Perseus receiver board over USB: firmware loader, front-end control and a virtual
//! device for tests. UNVERIFIED on a board.
//!
//! **Sources (facts only, nothing copied).** The public `libperseus-sdr` repository of Microtelecom
//! (LGPL-3.0, commit c2c95daeaa08bf0daed0e8ada970ab17cc264e1b): `perseusfx2.h`, `perseusfx2.c`,
//! `perseus-sdr.h` and `perseus-sdr.c` (the open sequence, the tuning-word formula, the front-end
//! port bits, the preselector cut-offs). Copies are kept outside every repository in
//! `~/manuals/microtelecom/` with SHA256SUMS. The Cypress EZ-USB firmware-load request is a
//! standard mechanism. The Perseus CAT manual (EN03) belongs to the CI-V description
//! `microtelecom-perseus`, not to this module.
//!
//! **What the board is.** A Cypress FX2 microcontroller (USB 04B4:8613 while it has no firmware,
//! 04B4:325C once running) and an FPGA. The FX2 firmware is loaded into its RAM over vendor
//! control request 0xA0 (hold the CPU in reset through the CPUCS register at 0xE600, write the
//! image, release); the device then renumbers. The FPGA bitstream (one image per DDC sample rate)
//! goes over bulk endpoint 1 in 63-byte pieces and a check command reads its DONE line. Front-end
//! control: a Port E byte (attenuator in the high nibble, preselector filter in the low nibble)
//! and a five-byte serial-IO word (control bits and a 32-bit tuning word).
//!
//! **The firmware is the user's.** radioctl does not contain or redistribute the Microtelecom FX2
//! firmware or FPGA images, and no repository stores them. [`PerseusFiles`] says where the loader
//! looks: a directory holding `fx2.hex` (Intel HEX of the FX2 image) and `fpga.rbs` (a raw FPGA
//! bitstream for the wanted sample rate), the directory named by the environment variable
//! `RADIOCTL_PERSEUS_FIRMWARE_DIR` or else `$HOME/.config/radioctl/firmware/perseus`. Microtelecom
//! publishes the FPGA images (`*.rbs`) in its own `libperseus-sdr` repository; the FX2 image is
//! built from the HEX records of that library or taken from the vendor's installation. A missing
//! file is an error that names the exact path looked at.
//!
//! **Boundary.** Receive streaming (bulk endpoint 0x82, the FIFO-enable bit, the choice of sample
//! rate image) is the job of the shared-receiver service, which feeds hams.com members; radioctl
//! stops at loading, tuning, attenuator, preselector, ADC dither and preamplifier. The board is
//! exclusive: the vendor application and this module cannot both own it. This module is a
//! library, not a `ProtocolEngine`: the engine seam is one request per operation and this board
//! needs a multi-step load first, so wiring it into `radioctld` is future work.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::usb_device::{UsbBackend, UsbDevice, UsbFailure, UsbTarget};

/// Cypress vendor id of the board.
pub const VENDOR_ID: u16 = 0x04B4;
/// Product id while the FX2 has no firmware.
pub const PRODUCT_ID_BLANK: u16 = 0x8613;
/// Product id with the Perseus firmware running.
pub const PRODUCT_ID_RUNNING: u16 = 0x325C;
/// Bulk OUT endpoint for commands.
pub const EP_CMD: u8 = 0x01;
/// Bulk IN endpoint for command status.
pub const EP_STATUS: u8 = 0x81;
/// Product code of a production unit in the EEPROM record.
pub const PRODUCT_CODE: u16 = 0x8014;
/// Nominal ADC clock in hertz.
pub const ADC_CLOCK_HZ: f64 = 80_000_000.0;
/// Highest tuning frequency (half the ADC clock).
pub const MAX_FREQUENCY_HZ: f64 = ADC_CLOCK_HZ / 2.0;

const REQUEST_TYPE_VENDOR_OUT: u8 = 0x40;
const REQUEST_FIRMWARE_LOAD: u8 = 0xA0;
const ADDR_CPUCS: u16 = 0xE600;
const CMD_FPGA_CONFIG: u8 = 0x00;
const CMD_FPGA_RESET: u8 = 0x01;
const CMD_FPGA_CHECK: u8 = 0x02;
const CMD_FPGA_SIO: u8 = 0x03;
const CMD_PORT_E: u8 = 0x04;
const CMD_EEPROM_READ: u8 = 0x06;
const CMD_SHUTDOWN: u8 = 0x08;
const FPGA_PIECE: usize = 63;
const EEPROM_PRODUCT_ID_ADDR: u16 = 8;
const PRODUCT_ID_LEN: usize = 12;
/// ADC dither enable bit of the serial-IO control byte.
pub const SIO_DITHER: u8 = 0x02;
/// ADC preamplifier ("gain high") bit of the serial-IO control byte.
pub const SIO_GAIN_HIGH: u8 = 0x04;
/// Upper cut-off, in hertz, of each of the ten preselector filters; above the last is the
/// wide-band path (index 10).
pub const PRESELECTOR_CUTOFFS_HZ: [u32; 10] = [
    1_700_000, 2_100_000, 3_000_000, 4_200_000, 6_000_000, 8_400_000, 12_000_000, 17_000_000,
    24_000_000, 32_000_000,
];
/// The wide-band (no preselection) filter index.
pub const PRESELECTOR_WIDE: u8 = 10;
const TIMEOUT: Duration = Duration::from_secs(1);
/// How long the loader waits for the board to renumber after the FX2 firmware starts.
pub const RENUMBER_WAIT: Duration = Duration::from_secs(4);
const RENUMBER_ATTEMPTS: u32 = 3;

fn err(kind: ErrorKind, detail: impl Into<String>) -> RigError {
    RigError::new(kind, detail.into())
}

fn usb_err(what: &str, failure: UsbFailure) -> RigError {
    let kind = match failure {
        UsbFailure::NoDevice => ErrorKind::LinkDown,
        UsbFailure::Timeout => ErrorKind::Timeout,
        UsbFailure::Stall | UsbFailure::Other(_) => ErrorKind::Rejected,
    };
    err(kind, format!("perseus: {what}: {failure:?}"))
}

/// Where the loader looks for the user-supplied firmware.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerseusFiles {
    /// The FX2 firmware, Intel HEX.
    pub fx2_hex: PathBuf,
    /// The FPGA bitstream, raw.
    pub fpga_image: PathBuf,
}

impl PerseusFiles {
    // [@ANCHOR: radioctl_perseus_files_in_dir]
    /// The two files in `dir`: `fx2.hex` and `fpga.rbs`.
    pub fn in_dir(dir: &Path) -> PerseusFiles {
        PerseusFiles {
            fx2_hex: dir.join("fx2.hex"),
            fpga_image: dir.join("fpga.rbs"),
        }
    }

    // [@ANCHOR: radioctl_perseus_files_default_dir]
    /// The directory in use: `env_dir` (the value of `RADIOCTL_PERSEUS_FIRMWARE_DIR`) when set and
    /// not empty, else `$HOME/.config/radioctl/firmware/perseus` (`home` is `$HOME`).
    pub fn default_dir(env_dir: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
        match (env_dir, home) {
            (Some(d), _) if !d.is_empty() => Some(PathBuf::from(d)),
            (_, Some(h)) if !h.is_empty() => {
                Some(Path::new(h).join(".config/radioctl/firmware/perseus"))
            }
            _ => None,
        }
    }
}

// [@ANCHOR: radioctl_perseus_read_file]
fn read_file(path: &Path, what: &str) -> Result<Vec<u8>, RigError> {
    std::fs::read(path).map_err(|e| {
        err(
            ErrorKind::LinkDown,
            format!(
                "perseus: the {what} is not available at {} ({e}); radioctl does not ship it, \
                 supply it there (see the perseus module documentation)",
                path.display()
            ),
        )
    })
}

/// One data record of an Intel HEX file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HexRecord {
    /// Load address in the FX2 RAM.
    pub address: u16,
    /// The bytes.
    pub data: Vec<u8>,
}

fn hex_byte(text: &str, at: usize) -> Option<u8> {
    u8::from_str_radix(text.get(at..at + 2)?, 16).ok()
}

// [@ANCHOR: radioctl_perseus_parse_intel_hex]
/// Parses Intel HEX text into data records, checking every record's checksum. Record types 00
/// (data) and 01 (end) are accepted; the FX2 image needs no extended addressing, so any other
/// type is refused.
pub fn parse_intel_hex(text: &str) -> Result<Vec<HexRecord>, RigError> {
    let bad = |n: usize, why: &str| err(ErrorKind::Malformed, format!("intel hex line {n}: {why}"));
    let mut records = Vec::new();
    let mut ended = false;
    for (index, line) in text.lines().enumerate() {
        let n = index + 1;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if ended {
            return Err(bad(n, "data after the end record"));
        }
        let body = line.strip_prefix(':').ok_or_else(|| bad(n, "no colon"))?;
        let length = usize::from(hex_byte(body, 0).ok_or_else(|| bad(n, "bad length"))?);
        if body.len() != (length + 5) * 2 {
            return Err(bad(n, "length does not match the record"));
        }
        let mut sum = 0u8;
        let mut bytes = Vec::with_capacity(length + 5);
        for i in 0..length + 5 {
            let b = hex_byte(body, i * 2).ok_or_else(|| bad(n, "not hexadecimal"))?;
            sum = sum.wrapping_add(b);
            bytes.push(b);
        }
        if sum != 0 {
            return Err(bad(n, "checksum"));
        }
        let hi = bytes.get(1).copied().unwrap_or(0);
        let lo = bytes.get(2).copied().unwrap_or(0);
        let kind = bytes.get(3).copied().unwrap_or(0xFF);
        let data = bytes.get(4..4 + length).unwrap_or(&[]).to_vec();
        match kind {
            0x00 => records.push(HexRecord {
                address: u16::from_be_bytes([hi, lo]),
                data,
            }),
            0x01 => ended = true,
            _ => return Err(bad(n, "unsupported record type")),
        }
    }
    if !ended {
        return Err(err(ErrorKind::Malformed, "intel hex: no end record"));
    }
    if records.is_empty() {
        return Err(err(ErrorKind::Malformed, "intel hex: no data records"));
    }
    Ok(records)
}

// [@ANCHOR: radioctl_perseus_load_fx2]
/// Loads the FX2 firmware into the board's RAM: hold the CPU in reset, write every record at its
/// address, release the reset. The release is expected to be cut short by the board renumbering,
/// so its outcome is not an error.
pub fn load_fx2(dev: &mut dyn UsbDevice, records: &[HexRecord]) -> Result<(), RigError> {
    dev.control_out(
        REQUEST_TYPE_VENDOR_OUT,
        REQUEST_FIRMWARE_LOAD,
        ADDR_CPUCS,
        0,
        &[1],
        TIMEOUT,
    )
    .map_err(|e| usb_err("holding the FX2 in reset", e))?;
    for record in records {
        dev.control_out(
            REQUEST_TYPE_VENDOR_OUT,
            REQUEST_FIRMWARE_LOAD,
            record.address,
            0,
            &record.data,
            TIMEOUT,
        )
        .map_err(|e| usb_err("writing FX2 RAM", e))?;
    }
    let _ = dev.control_out(
        REQUEST_TYPE_VENDOR_OUT,
        REQUEST_FIRMWARE_LOAD,
        ADDR_CPUCS,
        0,
        &[0],
        TIMEOUT,
    );
    Ok(())
}

// [@ANCHOR: radioctl_perseus_configure_fpga]
/// Configures the FPGA: reset, the bitstream in 63-byte pieces, a check, a pause for `wait(50 ms)`,
/// then the status read whose second byte is the DONE line. Nothing more is sent after a failure.
pub fn configure_fpga(
    dev: &mut dyn UsbDevice,
    image: &[u8],
    wait: &mut dyn FnMut(Duration),
) -> Result<(), RigError> {
    if image.is_empty() {
        return Err(err(
            ErrorKind::Malformed,
            "perseus: the FPGA image is empty",
        ));
    }
    let write = |dev: &mut dyn UsbDevice, data: &[u8], what: &str| {
        dev.bulk_write(EP_CMD, data, TIMEOUT)
            .map(|_| ())
            .map_err(|e| usb_err(what, e))
    };
    write(dev, &[CMD_FPGA_RESET], "FPGA reset")?;
    for piece in image.chunks(FPGA_PIECE) {
        let mut frame = Vec::with_capacity(piece.len() + 1);
        frame.push(CMD_FPGA_CONFIG);
        frame.extend_from_slice(piece);
        write(dev, &frame, "writing the FPGA bitstream")?;
    }
    write(dev, &[CMD_FPGA_CHECK], "FPGA check")?;
    wait(Duration::from_millis(50));
    let status = dev
        .bulk_read(EP_STATUS, 2, TIMEOUT)
        .map_err(|e| usb_err("reading the FPGA status", e))?;
    if status.get(1).copied() != Some(1) {
        return Err(err(
            ErrorKind::Rejected,
            format!("perseus: the FPGA did not report DONE (status {status:02X?})"),
        ));
    }
    Ok(())
}

/// The product record the board keeps in its EEPROM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductId {
    /// Serial number.
    pub serial: u16,
    /// Microtelecom product code (production units: [`PRODUCT_CODE`]).
    pub product_code: u16,
    /// Hardware release.
    pub hardware_release: u8,
    /// Hardware version.
    pub hardware_version: u8,
}

impl ProductId {
    // [@ANCHOR: radioctl_perseus_product_id_is_pre_series]
    /// True for one of the few pre-production units (any other product code), which drive the
    /// attenuator with inverted sense.
    pub fn is_pre_series(&self) -> bool {
        self.product_code != PRODUCT_CODE
    }
}

// [@ANCHOR: radioctl_perseus_read_product_id]
/// Reads the product record from the EEPROM through the board's command endpoints.
pub fn read_product_id(dev: &mut dyn UsbDevice) -> Result<ProductId, RigError> {
    let addr = EEPROM_PRODUCT_ID_ADDR.to_le_bytes();
    let command = [CMD_EEPROM_READ, addr[0], addr[1], PRODUCT_ID_LEN as u8];
    dev.bulk_write(EP_CMD, &command, TIMEOUT)
        .map_err(|e| usb_err("EEPROM read command", e))?;
    let reply = dev
        .bulk_read(EP_STATUS, 2 + PRODUCT_ID_LEN, TIMEOUT)
        .map_err(|e| usb_err("EEPROM read status", e))?;
    if reply.get(1).copied() != Some(1) {
        return Err(err(ErrorKind::Rejected, "perseus: the EEPROM read failed"));
    }
    let field = |at: usize| reply.get(2 + at).copied();
    let word = |at: usize| Some(u16::from_le_bytes([field(at)?, field(at + 1)?]));
    let parsed = (|| {
        Some(ProductId {
            serial: word(0)?,
            product_code: word(2)?,
            hardware_release: field(4)?,
            hardware_version: field(5)?,
        })
    })();
    parsed.ok_or_else(|| err(ErrorKind::Malformed, "perseus: the EEPROM reply is short"))
}

// [@ANCHOR: radioctl_perseus_target]
/// The USB target for the blank (`blank` true) or running board.
pub fn target(blank: bool) -> UsbTarget {
    UsbTarget {
        vendor_id: VENDOR_ID,
        product_id: if blank {
            PRODUCT_ID_BLANK
        } else {
            PRODUCT_ID_RUNNING
        },
        manufacturer: String::new(),
        product: String::new(),
        serial: None,
        interface: Some(0),
    }
}

// [@ANCHOR: radioctl_perseus_preselector_for]
/// The preselector filter index for a tuning frequency: the first filter whose upper cut-off is
/// above it, the wide-band path above the last or when preselection is off.
pub fn preselector_for(frequency_hz: f64, enabled: bool) -> u8 {
    if !enabled {
        return PRESELECTOR_WIDE;
    }
    PRESELECTOR_CUTOFFS_HZ
        .iter()
        .position(|cutoff| frequency_hz < f64::from(*cutoff))
        .map_or(PRESELECTOR_WIDE, |i| i as u8)
}

// [@ANCHOR: radioctl_perseus_tuning_word]
/// The 32-bit tuning word for a centre frequency: `f / 80 MHz * 2^32`, truncated.
pub fn tuning_word(frequency_hz: f64) -> u32 {
    (frequency_hz / ADC_CLOCK_HZ * 4_294_967_296.0) as u32
}

/// A board with firmware running and the FPGA configured, ready for control.
pub struct PerseusControl {
    dev: Box<dyn UsbDevice>,
    product: ProductId,
    frontend: u8,
    sio_control: u8,
    sio_word: u32,
    preselector: Option<u8>,
}

impl PerseusControl {
    // [@ANCHOR: radioctl_perseus_open_ready]
    /// Opens the board and makes it ready: if the running board is not on the bus the blank one
    /// is found, the user's FX2 firmware loaded and the renumbering waited for (`wait` is called
    /// with the pause; a real caller sleeps); then the EEPROM record is read and the user's FPGA
    /// image configured. Every missing file is an error naming its path.
    pub fn open_ready(
        backend: &dyn UsbBackend,
        files: &PerseusFiles,
        wait: &mut dyn FnMut(Duration),
    ) -> Result<PerseusControl, RigError> {
        let mut dev = match backend.open(&target(false)) {
            Ok(dev) => dev,
            Err(_) => {
                let mut blank = backend.open(&target(true))?;
                let hex = read_file(&files.fx2_hex, "FX2 firmware (Intel HEX)")?;
                let text = String::from_utf8(hex).map_err(|_| {
                    err(
                        ErrorKind::Malformed,
                        format!("perseus: {} is not text", files.fx2_hex.display()),
                    )
                })?;
                let records = parse_intel_hex(&text)?;
                load_fx2(blank.as_mut(), &records)?;
                drop(blank);
                let mut found = None;
                for _ in 0..RENUMBER_ATTEMPTS {
                    wait(RENUMBER_WAIT);
                    if let Ok(dev) = backend.open(&target(false)) {
                        found = Some(dev);
                        break;
                    }
                }
                found.ok_or_else(|| {
                    err(
                        ErrorKind::LinkDown,
                        "perseus: the board did not renumber after the firmware load",
                    )
                })?
            }
        };
        let product = read_product_id(dev.as_mut())?;
        let image = read_file(&files.fpga_image, "FPGA bitstream")?;
        configure_fpga(dev.as_mut(), &image, wait)?;
        Ok(PerseusControl {
            dev,
            product,
            frontend: 0,
            sio_control: 0,
            sio_word: 0,
            preselector: None,
        })
    }

    // [@ANCHOR: radioctl_perseus_product]
    /// The EEPROM product record read at open.
    pub fn product(&self) -> ProductId {
        self.product
    }

    // [@ANCHOR: radioctl_perseus_port_e]
    fn port_e(&mut self, value: u8) -> Result<(), RigError> {
        self.dev
            .bulk_write(EP_CMD, &[CMD_PORT_E, value], TIMEOUT)
            .map_err(|e| usb_err("front-end port write", e))?;
        self.frontend = value;
        Ok(())
    }

    // [@ANCHOR: radioctl_perseus_sio]
    fn sio(&mut self, control: u8, word: u32) -> Result<(), RigError> {
        let mut frame = vec![CMD_FPGA_SIO, control];
        frame.extend_from_slice(&word.to_le_bytes());
        self.dev
            .bulk_write(EP_CMD, &frame, TIMEOUT)
            .map_err(|e| usb_err("serial-IO write", e))?;
        let status = self
            .dev
            .bulk_read(EP_STATUS, frame.len(), TIMEOUT)
            .map_err(|e| usb_err("serial-IO status", e))?;
        if status.len() != frame.len() {
            return Err(err(ErrorKind::Malformed, "perseus: short serial-IO status"));
        }
        self.sio_control = control;
        self.sio_word = word;
        Ok(())
    }

    // [@ANCHOR: radioctl_perseus_set_attenuator_db]
    /// Sets the input attenuator to 0, 10, 20 or 30 dB (the high nibble of the front-end port);
    /// anything else is refused. A pre-series unit takes the inverted code.
    pub fn set_attenuator_db(&mut self, db: u8) -> Result<(), RigError> {
        let index = match db {
            0 => 0u8,
            10 => 1,
            20 => 2,
            30 => 3,
            _ => {
                return Err(err(
                    ErrorKind::Unsupported,
                    format!("perseus: no {db} dB attenuator step (0, 10, 20, 30)"),
                ))
            }
        };
        let code = if self.product.is_pre_series() {
            index ^ 3
        } else {
            index
        };
        self.port_e((code << 4) | (self.frontend & 0x0F))
    }

    // [@ANCHOR: radioctl_perseus_set_adc]
    /// Sets ADC dither and the ADC preamplifier (bits of the serial-IO control byte).
    pub fn set_adc(&mut self, dither: bool, preamp: bool) -> Result<(), RigError> {
        let mut control = self.sio_control & !(SIO_DITHER | SIO_GAIN_HIGH);
        if dither {
            control |= SIO_DITHER;
        }
        if preamp {
            control |= SIO_GAIN_HIGH;
        }
        self.sio(control, self.sio_word)
    }

    // [@ANCHOR: radioctl_perseus_set_frequency]
    /// Tunes the digital down-converter to `frequency_hz` (0 to 40 MHz) and, with `preselector`
    /// on, selects the matching preselector filter (the low nibble of the front-end port).
    pub fn set_frequency(&mut self, frequency_hz: f64, preselector: bool) -> Result<(), RigError> {
        if !(0.0..=MAX_FREQUENCY_HZ).contains(&frequency_hz) {
            return Err(err(
                ErrorKind::Unsupported,
                format!("perseus: {frequency_hz} Hz is outside 0 to 40 MHz"),
            ));
        }
        self.sio(self.sio_control, tuning_word(frequency_hz))?;
        let filter = preselector_for(frequency_hz, preselector);
        if self.preselector != Some(filter) {
            self.port_e((self.frontend & 0xF0) | (filter & 0x0F))?;
            self.preselector = Some(filter);
        }
        Ok(())
    }

    // [@ANCHOR: radioctl_perseus_shutdown]
    /// Resets the FPGA, disconnects the preselector filters and sets 30 dB attenuation.
    pub fn shutdown(&mut self) -> Result<(), RigError> {
        self.dev
            .bulk_write(EP_CMD, &[CMD_SHUTDOWN], TIMEOUT)
            .map(|_| ())
            .map_err(|e| usb_err("shutdown", e))
    }
}

/// What the virtual board has been told, for tests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VirtualPerseusState {
    /// True once the FX2 CPU was held in reset and released after RAM writes.
    pub firmware_running: bool,
    /// `(address, data)` of every FX2 RAM write.
    pub ram_writes: Vec<(u16, Vec<u8>)>,
    /// The FPGA bitstream received since the last reset.
    pub fpga: Vec<u8>,
    /// The last Port E byte.
    pub port_e: u8,
    /// The last serial-IO control byte and tuning word.
    pub sio: (u8, u32),
    /// True after the shutdown command.
    pub shut_down: bool,
    /// Every bulk command frame received on the command endpoint, in order.
    pub commands: Vec<Vec<u8>>,
}

struct VirtualInner {
    state: VirtualPerseusState,
    cpu_held: bool,
    status: Vec<Vec<u8>>,
    done: bool,
    product: [u8; PRODUCT_ID_LEN],
}

/// A virtual Perseus on a virtual USB bus: it appears as the blank board until the FX2 firmware
/// has been loaded, then as the running board. Clone it: all clones are the same board.
#[derive(Clone)]
pub struct VirtualPerseusBackend {
    inner: Arc<Mutex<VirtualInner>>,
}

fn lock(inner: &Mutex<VirtualInner>) -> MutexGuard<'_, VirtualInner> {
    inner.lock().unwrap_or_else(|p| p.into_inner())
}

impl VirtualPerseusBackend {
    // [@ANCHOR: radioctl_perseus_virtual_new]
    /// A board with no firmware (`running` false) or with it running, a production unit with
    /// serial number `serial`, whose FPGA reports DONE after any non-empty bitstream when
    /// `fpga_done` is true.
    pub fn new(running: bool, serial: u16, fpga_done: bool) -> VirtualPerseusBackend {
        let mut product = [0u8; PRODUCT_ID_LEN];
        product[..2].copy_from_slice(&serial.to_le_bytes());
        product[2..4].copy_from_slice(&PRODUCT_CODE.to_le_bytes());
        product[4] = 2;
        product[5] = 1;
        VirtualPerseusBackend {
            inner: Arc::new(Mutex::new(VirtualInner {
                state: VirtualPerseusState {
                    firmware_running: running,
                    ..VirtualPerseusState::default()
                },
                cpu_held: false,
                status: Vec::new(),
                done: fpga_done,
                product,
            })),
        }
    }

    // [@ANCHOR: radioctl_perseus_virtual_state]
    /// What the board has been told so far.
    pub fn state(&self) -> VirtualPerseusState {
        lock(&self.inner).state.clone()
    }
}

impl UsbBackend for VirtualPerseusBackend {
    // [@ANCHOR: radioctl_perseus_virtual_open]
    fn open(&self, target: &UsbTarget) -> Result<Box<dyn UsbDevice>, RigError> {
        let running = lock(&self.inner).state.firmware_running;
        let wanted_running = target.product_id == PRODUCT_ID_RUNNING;
        if target.vendor_id != VENDOR_ID
            || !(wanted_running || target.product_id == PRODUCT_ID_BLANK)
            || wanted_running != running
        {
            return Err(err(
                ErrorKind::LinkDown,
                format!("{} is not on the bus", target.label()),
            ));
        }
        Ok(Box::new(VirtualPerseusHandle {
            inner: Arc::clone(&self.inner),
        }))
    }
}

struct VirtualPerseusHandle {
    inner: Arc<Mutex<VirtualInner>>,
}

impl UsbDevice for VirtualPerseusHandle {
    fn control_in(
        &mut self,
        _request_type: u8,
        _request: u8,
        _value: u16,
        _index: u16,
        _length: u16,
        _timeout: Duration,
    ) -> Result<Vec<u8>, UsbFailure> {
        Err(UsbFailure::Stall)
    }

    // [@ANCHOR: radioctl_perseus_virtual_control_out]
    fn control_out(
        &mut self,
        request_type: u8,
        request: u8,
        value: u16,
        _index: u16,
        data: &[u8],
        _timeout: Duration,
    ) -> Result<usize, UsbFailure> {
        let mut g = lock(&self.inner);
        if g.state.firmware_running {
            return Err(UsbFailure::NoDevice);
        }
        if request_type != REQUEST_TYPE_VENDOR_OUT || request != REQUEST_FIRMWARE_LOAD {
            return Err(UsbFailure::Stall);
        }
        if value == ADDR_CPUCS {
            match data {
                [1] => g.cpu_held = true,
                [0] if g.cpu_held && !g.state.ram_writes.is_empty() => {
                    g.cpu_held = false;
                    g.state.firmware_running = true;
                }
                _ => return Err(UsbFailure::Stall),
            }
        } else if g.cpu_held {
            g.state.ram_writes.push((value, data.to_vec()));
        } else {
            return Err(UsbFailure::Stall);
        }
        Ok(data.len())
    }

    // [@ANCHOR: radioctl_perseus_virtual_bulk_write]
    fn bulk_write(
        &mut self,
        endpoint: u8,
        data: &[u8],
        _timeout: Duration,
    ) -> Result<usize, UsbFailure> {
        let mut g = lock(&self.inner);
        if !g.state.firmware_running || endpoint != EP_CMD {
            return Err(UsbFailure::Stall);
        }
        g.state.commands.push(data.to_vec());
        let (command, rest) = data.split_first().ok_or(UsbFailure::Stall)?;
        match *command {
            CMD_FPGA_RESET => g.state.fpga.clear(),
            CMD_FPGA_CONFIG => g.state.fpga.extend_from_slice(rest),
            CMD_FPGA_CHECK => {
                let done = g.done && !g.state.fpga.is_empty();
                g.status.push(vec![CMD_FPGA_CHECK, u8::from(done)]);
            }
            CMD_FPGA_SIO => {
                let control = rest.first().copied().ok_or(UsbFailure::Stall)?;
                let word = rest
                    .get(1..5)
                    .and_then(|w| <[u8; 4]>::try_from(w).ok())
                    .map(u32::from_le_bytes)
                    .ok_or(UsbFailure::Stall)?;
                g.state.sio = (control, word);
                g.status.push(data.to_vec());
            }
            CMD_PORT_E => g.state.port_e = rest.first().copied().ok_or(UsbFailure::Stall)?,
            CMD_EEPROM_READ => {
                let count = usize::from(rest.get(2).copied().ok_or(UsbFailure::Stall)?);
                let mut reply = vec![CMD_EEPROM_READ, 1];
                reply.extend(g.product.iter().take(count));
                g.status.push(reply);
            }
            CMD_SHUTDOWN => g.state.shut_down = true,
            _ => return Err(UsbFailure::Stall),
        }
        Ok(data.len())
    }

    // [@ANCHOR: radioctl_perseus_virtual_bulk_read]
    fn bulk_read(
        &mut self,
        endpoint: u8,
        _length: usize,
        _timeout: Duration,
    ) -> Result<Vec<u8>, UsbFailure> {
        let mut g = lock(&self.inner);
        if endpoint != EP_STATUS || g.status.is_empty() {
            return Err(UsbFailure::Timeout);
        }
        Ok(g.status.remove(0))
    }

    fn interrupt_write(
        &mut self,
        _endpoint: u8,
        _data: &[u8],
        _timeout: Duration,
    ) -> Result<usize, UsbFailure> {
        Err(UsbFailure::Stall)
    }

    fn interrupt_read(
        &mut self,
        _endpoint: u8,
        _length: usize,
        _timeout: Duration,
    ) -> Result<Vec<u8>, UsbFailure> {
        Err(UsbFailure::Stall)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;
    use std::time::Duration;

    const HEX: &str = ":0400000001020304F2\n:02001000AABB89\n:00000001FF\n";

    fn files(dir: &Path, hex: Option<&str>, fpga: Option<&[u8]>) -> PerseusFiles {
        let f = PerseusFiles::in_dir(dir);
        if let Some(h) = hex {
            std::fs::write(&f.fx2_hex, h).unwrap();
        }
        if let Some(b) = fpga {
            std::fs::write(&f.fpga_image, b).unwrap();
        }
        f
    }

    fn tmp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("radioctl-perseus-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // Tests [@ANCHOR: radioctl_perseus_parse_intel_hex]
    #[test]
    fn intel_hex_records_are_checked() {
        let records = parse_intel_hex(HEX).unwrap();
        assert_eq!(
            records,
            vec![
                HexRecord {
                    address: 0,
                    data: vec![1, 2, 3, 4]
                },
                HexRecord {
                    address: 0x10,
                    data: vec![0xAA, 0xBB]
                },
            ]
        );
        for (text, why) in [
            (":0400000001020304F3\n:00000001FF\n", "checksum"),
            ("0400000001020304F2\n:00000001FF\n", "no colon"),
            (":0400000001020304F2\n", "no end record"),
            (":00000001FF\n", "no data records"),
            (":020000040000FA\n:00000001FF\n", "unsupported"),
            (
                ":0400000001020304F2\n:00000001FF\n:0400000001020304F2\n",
                "after the end",
            ),
            (":05000000010203F2\n:00000001FF\n", "length"),
        ] {
            let e = parse_intel_hex(text).unwrap_err();
            assert_eq!(e.kind, ErrorKind::Malformed, "{why}");
            assert!(
                e.detail.contains(why.split(' ').next().unwrap_or(why)),
                "{why}: {}",
                e.detail
            );
        }
    }

    // Tests [@ANCHOR: radioctl_perseus_files_in_dir]
    // Tests [@ANCHOR: radioctl_perseus_files_default_dir]
    #[test]
    fn the_loader_looks_where_the_documentation_says() {
        let f = PerseusFiles::in_dir(Path::new("/x"));
        assert_eq!(f.fx2_hex, Path::new("/x/fx2.hex"));
        assert_eq!(f.fpga_image, Path::new("/x/fpga.rbs"));
        assert_eq!(
            PerseusFiles::default_dir(Some("/env"), Some("/home/u")),
            Some(PathBuf::from("/env"))
        );
        assert_eq!(
            PerseusFiles::default_dir(Some(""), Some("/home/u")),
            Some(PathBuf::from("/home/u/.config/radioctl/firmware/perseus"))
        );
        assert_eq!(PerseusFiles::default_dir(None, None), None);
    }

    // Tests [@ANCHOR: radioctl_perseus_open_ready]
    // Tests [@ANCHOR: radioctl_perseus_load_fx2]
    // Tests [@ANCHOR: radioctl_perseus_configure_fpga]
    // Tests [@ANCHOR: radioctl_perseus_read_product_id]
    // Tests [@ANCHOR: radioctl_perseus_target]
    // Tests [@ANCHOR: radioctl_perseus_virtual_new]
    // Tests [@ANCHOR: radioctl_perseus_virtual_state]
    // Tests [@ANCHOR: radioctl_perseus_virtual_open]
    // Tests [@ANCHOR: radioctl_perseus_virtual_control_out]
    // Tests [@ANCHOR: radioctl_perseus_virtual_bulk_write]
    // Tests [@ANCHOR: radioctl_perseus_virtual_bulk_read]
    // Tests [@ANCHOR: radioctl_perseus_read_file]
    // Tests [@ANCHOR: radioctl_perseus_product]
    #[test]
    fn a_blank_board_gets_firmware_then_the_fpga_then_reports_its_serial() {
        let dir = tmp("load");
        let bitstream: Vec<u8> = (0..150u32).map(|i| i as u8).collect();
        let f = files(&dir, Some(HEX), Some(&bitstream));
        let board = VirtualPerseusBackend::new(false, 4321, true);
        let mut waits = Vec::new();
        let ctl = PerseusControl::open_ready(&board, &f, &mut |d| waits.push(d)).unwrap();
        let s = board.state();
        assert!(s.firmware_running);
        assert_eq!(
            s.ram_writes,
            vec![(0, vec![1, 2, 3, 4]), (0x10, vec![0xAA, 0xBB])]
        );
        assert_eq!(s.fpga, bitstream);
        // EEPROM read, FPGA reset, 150 bytes in 63-byte pieces (63, 63, 24), check
        let cmds: Vec<usize> = s.commands.iter().map(Vec::len).collect();
        assert_eq!(cmds, vec![4, 1, 64, 64, 25, 1]);
        assert_eq!(s.commands.get(1), Some(&vec![CMD_FPGA_RESET]));
        assert_eq!(s.commands.get(5), Some(&vec![CMD_FPGA_CHECK]));
        assert_eq!(s.commands.first(), Some(&vec![CMD_EEPROM_READ, 8, 0, 12]));
        assert_eq!(waits, vec![RENUMBER_WAIT, Duration::from_millis(50)]);
        assert_eq!(ctl.product().serial, 4321);
        assert_eq!(ctl.product().product_code, PRODUCT_CODE);
        assert!(!ctl.product().is_pre_series());
        // a board that already runs is not given the firmware again
        let n = board.state().ram_writes.len();
        let _again = PerseusControl::open_ready(&board, &f, &mut |_| {}).unwrap();
        assert_eq!(board.state().ram_writes.len(), n);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_files_and_a_failed_fpga_stop_the_load_and_name_the_path() {
        let dir = tmp("missing");
        // no firmware file: the error names the path and nothing was sent to the board
        let f = files(&dir, None, Some(&[1, 2, 3]));
        let board = VirtualPerseusBackend::new(false, 1, true);
        let e = PerseusControl::open_ready(&board, &f, &mut |_| {})
            .err()
            .unwrap();
        assert_eq!(e.kind, ErrorKind::LinkDown);
        assert!(
            e.detail.contains(&f.fx2_hex.display().to_string()),
            "{}",
            e.detail
        );
        assert!(board.state().ram_writes.is_empty() && !board.state().firmware_running);
        // bad hex: refused before any byte is sent
        let f = files(&dir, Some(":0400000001020304F3\n:00000001FF\n"), None);
        let e = PerseusControl::open_ready(&board, &f, &mut |_| {})
            .err()
            .unwrap();
        assert_eq!(e.kind, ErrorKind::Malformed);
        assert!(board.state().ram_writes.is_empty());
        // firmware present, FPGA file missing: names it, sends no FPGA data
        std::fs::remove_file(&f.fpga_image).unwrap();
        let f = files(&dir, Some(HEX), None);
        let e = PerseusControl::open_ready(&board, &f, &mut |_| {})
            .err()
            .unwrap();
        assert!(
            e.detail.contains(&f.fpga_image.display().to_string()),
            "{}",
            e.detail
        );
        assert!(board.state().fpga.is_empty());
        // FPGA DONE low: an error, and nothing further is sent
        let f = files(&dir, Some(HEX), Some(&[9; 10]));
        let bad = VirtualPerseusBackend::new(true, 1, false);
        let e = PerseusControl::open_ready(&bad, &f, &mut |_| {})
            .err()
            .unwrap();
        assert_eq!(e.kind, ErrorKind::Rejected);
        assert!(e.detail.contains("DONE"));
        assert_eq!(bad.state().commands.last(), Some(&vec![CMD_FPGA_CHECK]));
        // an empty bitstream is refused before the FPGA is touched
        let f = files(&dir, Some(HEX), Some(&[]));
        let fresh = VirtualPerseusBackend::new(true, 1, true);
        assert!(PerseusControl::open_ready(&fresh, &f, &mut |_| {}).is_err());
        assert!(fresh.state().fpga.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Tests [@ANCHOR: radioctl_perseus_set_attenuator_db]
    // Tests [@ANCHOR: radioctl_perseus_set_adc]
    // Tests [@ANCHOR: radioctl_perseus_set_frequency]
    // Tests [@ANCHOR: radioctl_perseus_port_e]
    // Tests [@ANCHOR: radioctl_perseus_sio]
    // Tests [@ANCHOR: radioctl_perseus_shutdown]
    // Tests [@ANCHOR: radioctl_perseus_preselector_for]
    // Tests [@ANCHOR: radioctl_perseus_tuning_word]
    // Tests [@ANCHOR: radioctl_perseus_product_id_is_pre_series]
    #[test]
    fn the_front_end_is_driven_through_port_e_and_the_serial_io_word() {
        let dir = tmp("ctl");
        let f = files(&dir, Some(HEX), Some(&[7; 10]));
        let board = VirtualPerseusBackend::new(true, 1, true);
        let mut ctl = PerseusControl::open_ready(&board, &f, &mut |_| {}).unwrap();
        // 7.074 MHz: 7.074e6 / 80e6 * 2^32, preselector index 5 (6.0 to 8.4 MHz)
        ctl.set_frequency(7_074_000.0, true).unwrap();
        assert_eq!(board.state().sio, (0, 379782483));
        assert_eq!(board.state().port_e, 5);
        ctl.set_attenuator_db(20).unwrap();
        assert_eq!(board.state().port_e, 0x25);
        ctl.set_adc(true, true).unwrap();
        assert_eq!(board.state().sio, (SIO_DITHER | SIO_GAIN_HIGH, 379782483));
        ctl.set_adc(false, true).unwrap();
        assert_eq!(board.state().sio.0, SIO_GAIN_HIGH);
        // retuning inside the same filter sends no second Port E write
        let n = board.state().commands.len();
        ctl.set_frequency(7_100_000.0, true).unwrap();
        assert_eq!(board.state().commands.len(), n + 1);
        // preselector off: wide-band path, attenuator kept
        ctl.set_frequency(7_100_000.0, false).unwrap();
        assert_eq!(board.state().port_e, 0x2A);
        for bad in [5u8, 40] {
            assert_eq!(
                ctl.set_attenuator_db(bad).unwrap_err().kind,
                ErrorKind::Unsupported
            );
        }
        for bad in [-1.0, 40_000_001.0] {
            assert_eq!(
                ctl.set_frequency(bad, true).unwrap_err().kind,
                ErrorKind::Unsupported
            );
        }
        ctl.shutdown().unwrap();
        assert!(board.state().shut_down);
        assert_eq!(preselector_for(1_699_999.0, true), 0);
        assert_eq!(preselector_for(1_700_000.0, true), 1);
        assert_eq!(preselector_for(31_999_999.0, true), 9);
        assert_eq!(preselector_for(32_000_000.0, true), PRESELECTOR_WIDE);
        assert_eq!(tuning_word(40_000_000.0), 2_147_483_648);
        let pre = ProductId {
            serial: 1,
            product_code: 0,
            hardware_release: 0,
            hardware_version: 0,
        };
        assert!(pre.is_pre_series());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
