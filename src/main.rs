use anyhow::{Context, Result, anyhow};
use clap::Parser;
use hidapi::{HidApi, HidDevice};

const LOGITECH_VID: u16 = 0x046D;

// USB receiver PIDs
const UNIFYING_PID: u16 = 0xC52B;
const BOLT_PID: u16 = 0xC547;

// Bluetooth direct PIDs
const BT_MX_KEYS_MINI_PID: u16 = 0xB369;

// HID++ usage page/usage combos
// Vendor-specific (HID++ control channel) - works for Bluetooth direct
const HIDPP_BT_USAGE_PAGE: u16 = 0xFF43;
const HIDPP_BT_USAGE: u16 = 0x0202;

const CHANGE_HOST_FUNC: u8 = 0x1e;

#[derive(Parser, Debug)]
#[command(about = "Switch MX Keys Mini channel programmatically")]
struct Args {
    /// Target channel (1, 2, or 3)
    #[arg(short, long, value_parser = clap::value_parser!(u8).range(1..=3),
          conflicts_with_all = ["list", "probe"])]
    channel: Option<u8>,

    /// Device index on receiver (1 for keyboard; ignored for Bluetooth direct)
    #[arg(short, long, default_value_t = 1)]
    device_index: u8,

    /// CHANGE_HOST feature index on the device (find with --probe; commonly 0x09)
    #[arg(short, long, default_value_t = 0x09)]
    feature_index: u8,

    /// List all connected Logitech HID devices and exit
    #[arg(long)]
    list: bool,

    /// Probe the device to find its CHANGE_HOST feature index
    #[arg(long)]
    probe: bool,
}

/// Describes how we're connected to the keyboard
enum Connection {
    /// Bluetooth direct — device_index is always 0xFF
    Bluetooth,
    /// Via USB receiver — device_index is the slot (1–6)
    UsbReceiver,
}

fn open_device(api: &HidApi) -> Result<(HidDevice, Connection)> {
    // Try Bluetooth direct first (B369 = MX Keys Mini BT)
    // Must open the vendor-specific HID++ interface (FF43/0202), not the keyboard one
    for info in api.device_list() {
        if info.vendor_id() == LOGITECH_VID
            && info.product_id() == BT_MX_KEYS_MINI_PID
            && info.usage_page() == HIDPP_BT_USAGE_PAGE
            && info.usage() == HIDPP_BT_USAGE
        {
            let device = info
                .open_device(api)
                .context("Found BT device but failed to open it")?;
            println!(
                "Opened via Bluetooth: {:04X}:{:04X} (usagePage={:04X}, usage={:04X})",
                info.vendor_id(),
                info.product_id(),
                info.usage_page(),
                info.usage()
            );
            return Ok((device, Connection::Bluetooth));
        }
    }

    // Fall back to USB receivers
    let device = api
        .open(LOGITECH_VID, BOLT_PID)
        .or_else(|_| api.open(LOGITECH_VID, UNIFYING_PID))
        .context("No Logitech receiver or Bluetooth device found")?;

    println!(
        "Opened via USB receiver: {}",
        device
            .get_product_string()
            .ok()
            .flatten()
            .unwrap_or_else(|| "(unknown)".to_string())
    );

    Ok((device, Connection::UsbReceiver))
}

fn switch_channel(
    device: &HidDevice,
    connection: &Connection,
    device_index: u8,
    feature_index: u8,
    channel: u8,
) -> Result<()> {
    let target = channel - 1; // 0-indexed

    // For Bluetooth direct, device index is 0xFF (broadcast/self)
    let idx = match connection {
        Connection::Bluetooth => 0xFF,
        Connection::UsbReceiver => device_index,
    };

    // HID++ 2.0 short report (7 bytes) for USB receiver
    // HID++ 2.0 long report  (20 bytes) for Bluetooth direct
    let buf: Vec<u8> = match connection {
        Connection::UsbReceiver => vec![
            0x10, // short report ID
            idx,  // device index
            feature_index,
            CHANGE_HOST_FUNC,
            target,
            0x00,
            0x00,
        ],
        Connection::Bluetooth => {
            let mut b = vec![0u8; 20];
            b[0] = 0x11; // long report ID
            b[1] = idx; // 0xFF for BT direct
            b[2] = feature_index;
            b[3] = CHANGE_HOST_FUNC;
            b[4] = target;
            b
        }
    };

    println!("Sending ({} bytes): {:02X?}", buf.len(), buf);
    device.write(&buf).context("Failed to write HID report")?;
    Ok(())
}

fn probe_change_host_feature(
    device: &HidDevice,
    connection: &Connection,
    device_index: u8,
) -> Result<()> {
    let idx = match connection {
        Connection::Bluetooth => 0xFF,
        Connection::UsbReceiver => device_index,
    };

    // GetFeature for CHANGE_HOST (0x1814)
    let buf: Vec<u8> = match connection {
        Connection::UsbReceiver => vec![0x10, idx, 0x00, 0x0f, 0x18, 0x14, 0x00],
        Connection::Bluetooth => {
            let mut b = vec![0u8; 20];
            b[0] = 0x11;
            b[1] = idx;
            b[2] = 0x00; // root feature
            b[3] = 0x0f; // GetFeature
            b[4] = 0x18; // feature ID hi
            b[5] = 0x14; // feature ID lo
            b
        }
    };

    println!(
        "Probing CHANGE_HOST (0x1814) on device index 0x{:02X}...",
        idx
    );
    println!("Sending: {:02X?}", buf);
    device.write(&buf).context("Write failed")?;

    let mut response = [0u8; 20];
    let n = device
        .read_timeout(&mut response, 3000)
        .context("Read timed out — press a key on the keyboard first to wake it")?;

    println!("Response ({} bytes): {:02X?}", n, &response[..n]);

    if n >= 5 {
        let feature_index = response[4];
        if feature_index == 0x00 {
            println!("CHANGE_HOST not found on this device/interface.");
        } else {
            println!(
                "✓ CHANGE_HOST feature index = 0x{:02X} ({})",
                feature_index, feature_index
            );
            println!("Use: --feature-index {}", feature_index);
        }
    }

    Ok(())
}

fn list_devices(api: &HidApi) {
    println!(
        "{:<10} {:<10} {:<12} {:<12} {:<35} {}",
        "VID", "PID", "UsagePage", "Usage", "Product", "Path"
    );
    println!("{}", "-".repeat(100));
    for info in api.device_list() {
        println!(
            "{:<10} {:<10} {:<12} {:<12} {:<35} {}",
            format!("{:04X}", info.vendor_id()),
            format!("{:04X}", info.product_id()),
            format!("{:04X}", info.usage_page()),
            format!("{:04X}", info.usage()),
            info.product_string().unwrap_or("(unknown)"),
            info.path().to_string_lossy()
        );
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    let api = HidApi::new().context("Failed to initialize HID API")?;

    if args.list {
        list_devices(&api);
        return Ok(());
    }

    let (device, connection) = open_device(&api)?;

    if args.probe {
        probe_change_host_feature(&device, &connection, args.device_index)?;
        return Ok(());
    }

    let channel = args
        .channel
        .ok_or_else(|| anyhow!("--channel is required unless using --list or --probe"))?;

    switch_channel(
        &device,
        &connection,
        args.device_index,
        args.feature_index,
        channel,
    )?;
    println!("✓ Switched to channel {}", channel);

    Ok(())
}
