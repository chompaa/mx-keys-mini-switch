use anyhow::{Result, anyhow};
use clap::Parser;
use hidapi::HidApi;
use mx_keys_switch::{probe_feature_index, switch_channel};

#[derive(Parser, Debug)]
#[command(about = "Switch MX Keys Mini Bluetooth channel")]
struct Args {
    /// Target channel (1, 2, or 3)
    #[arg(short, long, value_parser = clap::value_parser!(u8).range(1..=3),
          conflicts_with_all = ["list", "probe"])]
    channel: Option<u8>,

    /// List Logitech HID devices (for debugging)
    #[arg(long)]
    list: bool,

    /// Probe to find the CHANGE_HOST feature index
    #[arg(long)]
    probe: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    if args.list {
        list_devices()?;
        return Ok(());
    }

    if args.probe {
        let idx = probe_feature_index()?;
        println!("✓ CHANGE_HOST feature index = 0x{:02X} ({})", idx, idx);
        return Ok(());
    }

    let channel = args
        .channel
        .ok_or_else(|| anyhow!("--channel is required unless using --list or --probe"))?;

    switch_channel(channel)?;
    println!("✓ Switched to channel {}", channel);
    Ok(())
}

fn list_devices() -> Result<()> {
    let api = HidApi::new()?;
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
    Ok(())
}
