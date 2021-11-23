mod rs232_bridge;
mod rs232_bridge_def;

use pjlink_bridge::*;
use rs232_bridge_def::{BridgeDefinition};

use std::sync::{Arc, Mutex};
use clap::{AppSettings, Clap};
use log::{LevelFilter, error, info};
use simple_logger::{SimpleLogger};
use uuid::Uuid;

use crate::rs232_bridge::{PjLinkRS232Projector, PjLinkRS232ProjectorOptions};

#[derive(Clap)]
#[clap(version = "0.1.0", author = "Mateus Meyer Jiacomelli")]
#[clap(setting = AppSettings::ColoredHelp)]
struct Opts {
    #[clap(short, long, default_value = "0.0.0.0")]
    listen_address: String,
    #[clap(short, long, default_value = "4352")]
    port: String,
    #[clap(short, long, parse(from_occurrences))]
    verbose: i32,
    #[clap(long)]
    no_log: bool,
    #[clap(short, long)]
    udp: bool,
    #[clap(long, default_value = "0.0.0.0")]
    udp_listen_address: String,
    #[clap(long)]
    projector_name: String,
    #[clap(long)]
    serial_number: Option<String>,
    #[clap(long)]
    password: Option<String>,
    #[clap(required = true)]
    serial_port: String,
    #[clap(short, long)]
    baud_rate: Option<u32>,
    #[clap(default_value = "projector_info.toml")]
    projector_info_path: String,
}

#[inline(always)]
fn default_log_logging(selected_level: i32, default_level: LevelFilter) -> LevelFilter {
    match selected_level {
        1 => LevelFilter::Error,
        2 => LevelFilter::Warn,
        3 => LevelFilter::Info,
        4 => LevelFilter::Debug,
        5 => LevelFilter::Trace,
        _ => default_level
    }
}

pub fn main() {
    let cmd_opts = Opts::parse();

    if !cmd_opts.no_log {
        SimpleLogger::new()
            .with_level(default_log_logging(cmd_opts.verbose, LevelFilter::Warn))
            .with_module_level("pjlink_rs232_bridge", default_log_logging(cmd_opts.verbose, LevelFilter::Info))
            .with_module_level("pjlink_rs232_bridge::rs232_bridge", default_log_logging(cmd_opts.verbose, LevelFilter::Warn))
            .with_module_level("pjlink_bridge", default_log_logging(cmd_opts.verbose, LevelFilter::Info))
            .init()
            .unwrap();
    }

    let tcp_bind_address = cmd_opts.listen_address;
    let tcp_port = cmd_opts.port;
    let password = cmd_opts.password;

    match BridgeDefinition::from_file(cmd_opts.projector_info_path) {
        Ok(definition) => {
            let mut options = PjLinkRS232ProjectorOptions::from_def(definition);
            options.password = password;
            options.projector_name = Vec::from(cmd_opts.projector_name.as_bytes());
            options.serial_port = cmd_opts.serial_port;
            if let Some(baud_rate) = cmd_opts.baud_rate {
                options.baud_rate = baud_rate;
            }

            if let Some(serial_number) = cmd_opts.serial_number {
                options.serial_number = Vec::from(serial_number.as_bytes());
            } else {
                options.serial_number = Vec::from(Uuid::new_v4().to_simple().to_string().as_bytes());
            }

            info!("Projector Manufacturer: {}", String::from_utf8(options.manufacturer_name.clone()).unwrap_or_default());
            info!("Projector Model: {}", String::from_utf8(options.product_name.clone()).unwrap_or_default());
            info!("Projector Name: {}", String::from_utf8(options.projector_name.clone()).unwrap_or_default());
            info!("Projector Serial: {}", String::from_utf8(options.serial_number.clone()).unwrap_or_default());
            info!(
                "Serial Port: {}, {} baud, {}{}{}, {}",
                options.serial_port.clone(),
                options.baud_rate,
                options.data_bits,
                options.parity,
                options.stop_bits,
                if options.hardware_flow_control {"Hardware Flow Control"}
                else if options.software_flow_control {"Software Flow Control"}
                else {"No Flow Control"}
            );

            let handler = PjLinkRS232Projector::new(options);
            let shared_handler = Arc::new(Mutex::new(handler));

            if cmd_opts.udp {
                let udp_bind_address = cmd_opts.udp_listen_address;
                let (_, tcp_handle, _) = PjLinkServer::listen_tcp_udp(shared_handler, tcp_bind_address, udp_bind_address, tcp_port);

                tcp_handle.join().unwrap();
            } else {
                let (_, tcp_handle) = PjLinkServer::listen_tcp_only(shared_handler, tcp_bind_address, tcp_port);
                tcp_handle.join().unwrap();
            }
        },
        Err(err) => error!("{}", err.message)
    }
}