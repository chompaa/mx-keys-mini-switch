use anyhow::{Context, Result, anyhow};
use hidapi::HidApi;

const LOGITECH_VID: u16 = 0x046D;
const BT_MX_KEYS_MINI_PID: u16 = 0xB369;
const HIDPP_BT_USAGE_PAGE: u16 = 0xFF43;
const HIDPP_BT_USAGE: u16 = 0x0202;
const UNIFYING_PID: u16 = 0xC52B;
const BOLT_PID: u16 = 0xC547;
const CHANGE_HOST_FUNC: u8 = 0x1e;

pub(crate) enum Transport {
    Bluetooth,
    UsbReceiver,
}

/// Switch the MX Keys Mini to a given channel (1, 2, or 3) over Bluetooth.
///
/// # Example
///
/// ```no_run
/// mx_keys_switch::switch_channel(2).unwrap();
/// ```
pub fn switch_channel(channel: u8) -> Result<()> {
    if !(1..=3).contains(&channel) {
        return Err(anyhow!("channel must be 1, 2, or 3 (got {})", channel));
    }
    let api = HidApi::new().context("failed to initialize HID API")?;
    let (device, transport) = open_device(&api)?;

    let target = channel - 1; // HID++ is 0-indexed
    let buf = make_switch_msg(&transport, 1, 0x09, target);
    device.write(&buf).context("failed to write HID report")?;
    Ok(())
}

/// Probe the keyboard to find the `CHANGE_HOST` feature index.
///
/// Useful if the keyboard returns a different index than the default `0x09`.
pub fn probe_feature_index() -> Result<u8> {
    let api = HidApi::new().context("Failed to initialize HID API")?;
    let (device, transport) = open_device(&api)?;

    // HID++ 2.0: `GetFeature` request for `CHANGE_HOST` (0x1814)
    let buf = match transport {
        Transport::Bluetooth => {
            let mut b = build_hidpp_long(0xFF, 0x00, 0x0f, 0x00);
            b[4] = 0x18;
            b[5] = 0x14;
            b.to_vec()
        }
        Transport::UsbReceiver => vec![0x10, 0x01, 0x00, 0x0f, 0x18, 0x14, 0x00],
    };

    device.write(&buf).context("write failed")?;

    let mut response = [0u8; 20];
    let n = device
        .read_timeout(&mut response, 3000)
        .context("read timed out, press a key on the keyboard to wake it")?;

    if n < 5 {
        return Err(anyhow!("response too short ({} bytes)", n));
    }

    let feature_index = response[4];
    if feature_index == 0x00 {
        Err(anyhow!("CHANGE_HOST feature not found on this device"))
    } else {
        Ok(feature_index)
    }
}

/// Switch using an explicit feature index, if the device differs from the default `0x09`.
pub fn switch_channel_with_feature(channel: u8, feature_index: u8) -> Result<()> {
    if !(1..=3).contains(&channel) {
        return Err(anyhow!("Channel must be 1, 2, or 3 (got {})", channel));
    }
    let api = HidApi::new().context("Failed to initialize HID API")?;
    let (device, transport) = open_device(&api)?;

    let target = channel - 1;
    let buf = make_switch_msg(&transport, 1, feature_index, target);
    device.write(&buf).context("Failed to write HID report")?;
    Ok(())
}

fn open_device(api: &HidApi) -> Result<(hidapi::HidDevice, Transport)> {
    // 1. Prefer Bluetooth direct (macOS / Windows BT stack)
    for info in api.device_list() {
        if info.vendor_id() == LOGITECH_VID
            && info.product_id() == BT_MX_KEYS_MINI_PID
            && info.usage_page() == HIDPP_BT_USAGE_PAGE
            && info.usage() == HIDPP_BT_USAGE
        {
            let device = info.open_device(api).context(
                "Found MX Keys Mini but failed to open HID++ interface.\n\
                 On macOS: grant Input Monitoring to your terminal in System Settings.\n\
                 On Linux: add a udev rule for 046D:B369.\n\
                 On Windows: try running as Administrator.",
            )?;
            return Ok((device, Transport::Bluetooth));
        }
    }

    // 2. Fall back to Bolt/Unifying receiver (Windows with USB dongle)
    for info in api.device_list() {
        if info.vendor_id() == LOGITECH_VID
            && (info.product_id() == BOLT_PID || info.product_id() == UNIFYING_PID)
            && info.usage_page() == 0xFF00
            && info.usage() == 0x0001
        {
            let device = info
                .open_device(api)
                .context("Found Bolt/Unifying receiver but failed to open it.")?;
            return Ok((device, Transport::UsbReceiver));
        }
    }

    Err(anyhow!(
        "MX Keys Mini not found via Bluetooth ({:04X}:{:04X} usagePage={:04X} usage={:04X})\n\
         or USB receiver ({:04X}:{:04X} / {:04X}:{:04X} usagePage=FF00 usage=0001).\n\
         Is it on and paired?",
        LOGITECH_VID,
        BT_MX_KEYS_MINI_PID,
        HIDPP_BT_USAGE_PAGE,
        HIDPP_BT_USAGE,
        LOGITECH_VID,
        BOLT_PID,
        LOGITECH_VID,
        UNIFYING_PID,
    ))
}

fn make_switch_msg(
    transport: &Transport,
    device_index: u8,
    feature_index: u8,
    target: u8,
) -> Vec<u8> {
    match transport {
        Transport::Bluetooth => {
            build_hidpp_long(0xFF, feature_index, CHANGE_HOST_FUNC, target).to_vec()
        }
        Transport::UsbReceiver => vec![
            0x10,
            device_index,
            feature_index,
            CHANGE_HOST_FUNC,
            target,
            0x00,
            0x00,
        ],
    }
}

/// Build a 20-byte HID++ 2.0 long report.
fn build_hidpp_long(device_index: u8, feature_index: u8, func: u8, param0: u8) -> [u8; 20] {
    let mut buf = [0u8; 20];
    buf[0] = 0x11; // long report ID
    buf[1] = device_index;
    buf[2] = feature_index;
    buf[3] = func;
    buf[4] = param0;
    buf
}
