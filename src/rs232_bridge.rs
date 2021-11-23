use std::{
    process::exit,
    sync::{mpsc::{Receiver, RecvError, Sender, SyncSender, channel, sync_channel}},
    thread::{self, JoinHandle},
    time::Duration
};

use log::{debug, error, info};
use pjlink_bridge::{PjLinkCommand, PjLinkHandler, PjLinkRawPayload, PjLinkResponse};

use crate::rs232_bridge_def::{BridgeDefinition, BridgeDefinitionBehavior, BridgeDefinitionCommand, BridgeDefinitionCommandDefinition, BridgeDefinitionCommandDefinitionOutput, BridgeDefinitionCommandDefinitionOutputProjectorResponse, BridgeDefinitionCommandDefinitionOutputResponse, BridgeDefinitionCommandsMap};

#[derive(Clone)]
pub struct PjLinkRS232ProjectorState {
    power_on: u8,
    error_fan_status: u8,
    error_lamp_status: u8,
    error_temperature_status: u8,
    error_cover_open_status: u8,
    error_filter_status: u8,
    error_other_status: u8,
    lamp_hours: Vec<u8>,
    filter_hours: Vec<u8>,
    mute_status: [u8; 2],
    input_status: [u8; 2],
    available_inputs: Vec<u8>,
    freeze_status: u8,
}

pub struct PjLinkRS232ProjectorOptions {
    pub password: Option<String>,
    pub class_type: u8,
    pub manufacturer_name: Vec<u8>,
    pub product_name: Vec<u8>,
    pub projector_name: Vec<u8>,
    pub serial_number: Vec<u8>,
    pub software_version: Vec<u8>,
    pub screen_resolution: Vec<u8>,
    pub recommended_screen_resolution: Vec<u8>,
    pub commands: BridgeDefinitionCommandsMap,
    pub behavior: BridgeDefinitionBehavior,
    pub serial_port: String,
    pub baud_rate: u32,
    pub data_bits: u8,
    pub parity: char,
    pub stop_bits: u8,
    pub hardware_flow_control: bool,
    pub software_flow_control: bool,
}

impl PjLinkRS232ProjectorOptions {
    pub fn from_def(def: BridgeDefinition) -> PjLinkRS232ProjectorOptions {
        let manufacturer_name = Vec::from(def.general.manufacturer_name.as_bytes());
        let product_name = Vec::from(def.general.product_name.as_bytes());
        let software_version = Vec::from(def.general.software_version.as_bytes());

        let mut resolution_current = Vec::<u8>::new();
        let mut resolution_recommended = Vec::<u8>::new();

        if let Some(def_resolution) = def.resolution {
            if let Some(def_resolution_current) = def_resolution.current {
                resolution_current = Vec::from(format!("{}x{}", def_resolution_current[0], def_resolution_current[1]));
            }

            if let Some(def_resolution_recommended) = def_resolution.recommended {
                resolution_recommended = Vec::from(format!("{}x{}", def_resolution_recommended[0], def_resolution_recommended[1]));
            }
        }

        PjLinkRS232ProjectorOptions {
            class_type: def.general.class_type + 48, // fast converting to char
            manufacturer_name,
            product_name,
            software_version,
            recommended_screen_resolution: resolution_recommended,
            screen_resolution: resolution_current,
            password: Option::None,
            projector_name: Vec::<u8>::new(),
            serial_number: Vec::<u8>::new(),
            commands: def.commands,
            behavior: def.behavior.unwrap_or(BridgeDefinitionBehavior {
                send_on_start: None,
                wait_for_response: Some(50)
            }),
            serial_port: String::from(""),
            baud_rate: def.connection.baud_rate,
            data_bits: def.connection.data_bits.unwrap_or(8),
            parity: def.connection.parity.unwrap_or('N'),
            stop_bits: def.connection.stop_bits.unwrap_or(1),
            hardware_flow_control: def.connection.hardware_flow_control.unwrap_or(false),
            software_flow_control: def.connection.software_flow_control.unwrap_or(false),
        }
    }
}

struct PjLinkRS232MessageRequest {
    message: Vec<u8>,
    timeout: u32,
    stop_processing: bool
}

struct PjLinkRS232MessageResponse {
    response: Vec<u8>,
    elapsed_time: u32
}

struct PjLinkRS232Connector {
}

/// Minimum timeout allowed (in ms).
const CONNECTOR_THREAD_MIN_TIMEOUT: u32 = 50;

struct PjLinkRS232ConnectorOptions {
    serial_port: String,
    baud_rate: u32,
    data_bits: u8,
    parity: char,
    stop_bits: u8,
    hardware_flow_control: bool,
    software_flow_control: bool,
}

impl PjLinkRS232Connector {
    fn spawn(
        options: PjLinkRS232ConnectorOptions,
        transmission: (Sender<PjLinkRS232MessageResponse>, Receiver<PjLinkRS232MessageRequest>)
    ) {
        let (tx, rx) = transmission;

        match Self::build_connection(options)
            .open() 
        {
            Ok(mut serial_conn_box) => {
                let serial_conn = serial_conn_box.as_mut(); 
                while let Ok(message) = rx.recv() {
                    let timeout = Duration::from_millis((
                        if message.timeout >= CONNECTOR_THREAD_MIN_TIMEOUT {message.timeout}
                        else {CONNECTOR_THREAD_MIN_TIMEOUT}
                    ) as u64);
                    let message_buffer = message.message;

                    if let Err(err) = serial_conn.write_all(&message_buffer[0..message_buffer.len()]) {
                        error!("Error when writing to serial connection. {}", err);
                    }

                    if let Err(err) = serial_conn.set_timeout(timeout) {
                        error!("Error when defining serial timeout. {}", err);
                    }

                    thread::sleep(timeout);
                    let buffer_size = serial_conn.bytes_to_read().unwrap_or_default() as usize;

                    let mut buffer = vec! [0;1];
                    buffer.resize(buffer_size, 0);

                    if let Err(err) = serial_conn.read(buffer.as_mut_slice()) {
                        error!("Error when reading from serial connection. {}", err);
                    }
                    
                    tx.send(PjLinkRS232MessageResponse {
                        response: buffer,
                        elapsed_time: 0
                    }).unwrap_or_default();
                }
            }
            Err(e) => {
                error!("Cannot start serial communication! {}", e);
                exit(1)
            }
        }
    }

    #[inline(always)]
    fn build_connection(
        options: PjLinkRS232ConnectorOptions
    ) -> serialport::SerialPortBuilder {
        let serial_data_bits = match options.data_bits {
            8 => serialport::DataBits::Eight,
            7 => serialport::DataBits::Seven,
            6 => serialport::DataBits::Six,
            5 => serialport::DataBits::Five,
            _ => {
                error!("Unsupported serial data bits: {}", options.data_bits);
                exit(1)
            } 
        };

        let serial_parity = match options.parity {
            'N' => serialport::Parity::None,
            'E' => serialport::Parity::Even,
            'O' => serialport::Parity::Odd,
            _ => {
                error!("Unsupported serial parity: {}", options.parity);
                exit(1)
            } 
        };

        let serial_stop_bits = match options.stop_bits {
            1 => serialport::StopBits::One,
            2 => serialport::StopBits::Two,
            _ => {
                error!("Unsupported serial stop bits: {}", options.stop_bits);
                exit(1)
            } 
        };

        let serial_flow_control
            = if options.hardware_flow_control {serialport::FlowControl::Hardware}
            else if options.software_flow_control {serialport::FlowControl::Software}
            else {serialport::FlowControl::None};

        serialport::new(options.serial_port, options.baud_rate)
            .parity(serial_parity)
            .data_bits(serial_data_bits)
            .stop_bits(serial_stop_bits)
            .flow_control(serial_flow_control)
    }
}

pub struct PjLinkRS232Projector {
    options: PjLinkRS232ProjectorOptions,
    tx: SyncSender<PjLinkRS232MessageRequest>,
    rx: Receiver<PjLinkRS232MessageResponse>,
//    state: PjLinkRS232ProjectorState
}

impl PjLinkRS232Projector {
    pub fn new(options: PjLinkRS232ProjectorOptions) -> Self {
        let (tx, rx ) = Self::open_rs232_connector(
            options.serial_port.clone(),
            options.baud_rate,
            options.data_bits,
            options.parity,
            options.stop_bits,
            options.hardware_flow_control,
            options.software_flow_control
        );

        PjLinkRS232Projector {
            options,
            tx,
            rx,
        }
    }

    fn handle_dynamic_content(&mut self, _command: PjLinkCommand, raw_command: &PjLinkRawPayload, connection_id: &u64) -> PjLinkResponse {
        let request_body = raw_command.command_body_with_class;
        let command_spec_result = self.options.commands.get(&request_body);

        if let Some(command_spec) = command_spec_result {
            let request_parameter = raw_command.transmission_parameter.clone();
            if let Some(command_input_definition) = command_spec.inputs.get(&request_parameter) {
                let timeout = self.get_timeout(&self.options.behavior, command_input_definition, command_spec);
                let message = command_input_definition.send.clone();
                let send_times = command_input_definition.send_times.unwrap_or(1);

                let recv_message = if send_times > 1 {
                    let mut result: Result<PjLinkRS232MessageResponse, RecvError> = Ok(PjLinkRS232MessageResponse {response: vec! [], elapsed_time: 0});

                    for _ in 0..send_times {
                        result = self.send_and_receive_message(message.clone(), timeout, connection_id);
                    }

                    result
                } else {
                    self.send_and_receive_message(message, timeout, connection_id)
                };

                match recv_message {
                    Ok(response) => self.handle_connector_response(
                        request_body,
                        request_parameter,
                        response,
                        command_input_definition,
                        connection_id
                    ),
                    Err(err) => {
                        error!("Can't receive message from connector thread! ConnectionId: {}, {}", *connection_id, err);
                        PjLinkResponse::UnavailableTime
                    }
                }
            } else {
                debug!(
                    "Projector specification doesn't contain a mapping for provided transmission parameter. ConnectionId: {}, Command: {} , Tx: {}",
                    *connection_id,
                    std::str::from_utf8(&request_body).unwrap_or_default(),
                    std::str::from_utf8(&request_parameter).unwrap_or_default(),
                );
                PjLinkResponse::OutOfParameter
            }
        } else {
            debug!(
                "Projector specification doesn't contain a mapping for provided command. ConnectionId: {}, Command: {}",
                *connection_id,
                std::str::from_utf8(&request_body).unwrap_or_default(),
            );
            PjLinkResponse::Undefined
        }
    }

    #[inline(always)]
    fn handle_connector_response(
        &self,
        request_body: [u8; 5],
        request_parameter: Vec<u8>,
        response: PjLinkRS232MessageResponse,
        command_input_definition: &BridgeDefinitionCommandDefinition,
        connection_id: &u64
    ) -> PjLinkResponse {
        let PjLinkRS232MessageResponse {response: projector_response, elapsed_time} = response;
        debug!(
            "Received from projector: ConnectionId: {}, Response: {:02x?}, ElapsedTime: {}",
            *connection_id,
            projector_response,
            elapsed_time
        );

        for BridgeDefinitionCommandDefinitionOutput {
            on_received: command_on_received,
            response: command_response,
        } in &command_input_definition.outputs {
            match command_on_received {
                BridgeDefinitionCommandDefinitionOutputProjectorResponse::Value(command_on_received_value) =>
                    if let Some(handler_response_value) = self.handle_connector_response_value(
                        &request_body,
                        &projector_response,
                        command_on_received_value,
                        command_response,
                        connection_id
                    ) {
                        return handler_response_value;
                    }
                BridgeDefinitionCommandDefinitionOutputProjectorResponse::RuleMap(_, _) => panic!("RuleMap not implemented")
            }
        }

        debug!(
            "Projector specification doesn't contain a mapping for provided projector response. ConnectionId: {}, Command: {} , Tx: {}, Rx: {}",
            *connection_id,
            std::str::from_utf8(&request_body).unwrap_or_default(),
            std::str::from_utf8(&request_parameter).unwrap_or_default(),
            std::str::from_utf8(&projector_response).unwrap_or_default(),
        );
        PjLinkResponse::OutOfParameter
    }

    #[inline(always)]
    fn handle_connector_response_value(
        &self,
        request_body: &[u8; 5],
        projector_response: &[u8],
        command_on_received: &[u8],
        command_response: &BridgeDefinitionCommandDefinitionOutputResponse,
        connection_id: &u64
    ) -> Option<PjLinkResponse> {
        // Output from projector is equal to output from projector spec
        if projector_response.eq(command_on_received) {
            match command_response {
                BridgeDefinitionCommandDefinitionOutputResponse::Value(command_response_value) => {
                    let handler_response_value: PjLinkResponse = command_response_value.clone().into();
                    debug!(
                        "Translated response: ConnectionId: {}, CmdBodyWithClass: {}, TxParam: {}",
                        *connection_id,
                        std::str::from_utf8(request_body).unwrap_or_default(),
                        command_response_value
                    );

                    Some(handler_response_value)
                },
                BridgeDefinitionCommandDefinitionOutputResponse::Default(command_response) => {
                    let handler_response_value: PjLinkResponse = command_response.clone().into();
                    debug!(
                        "Translated response: ConnectionId: {}, CmdBodyWithClass: {}, TxParam: {}",
                        *connection_id,
                        std::str::from_utf8(request_body).unwrap_or_default(),
                        command_response
                    );

                    Some(handler_response_value)
                }
            }
        } else {None}
    }

    #[inline(always)]
    fn send_and_receive_message(
        &self,
        message: Vec<u8>,
        timeout: u32,
        connection_id: &u64
    ) -> Result<PjLinkRS232MessageResponse, RecvError> {
        debug!(
            "Will send to projector: ConnectionId: {}, Request: {:02x?}",
            *connection_id,
            message,
        );

        if let Err(err) = self.tx.send(PjLinkRS232MessageRequest {
            message,
            timeout,
            stop_processing: false
        }) {
            error!("Can't send message to connector thread! ConnectionId: {}, {}", *connection_id, err);
        }

        self.rx.recv()
    }

    #[inline(always)]
    fn get_timeout(&self,
        behavior: &BridgeDefinitionBehavior,
        command_input_definition: &BridgeDefinitionCommandDefinition,
        command_spec: &BridgeDefinitionCommand
    ) -> u32 {
        if let Some(cd_wait_for_response) = command_input_definition.wait_for_response {cd_wait_for_response}
        else if let Some(cs_wait_for_response) = command_spec.wait_for_response {cs_wait_for_response}
        else {behavior.wait_for_response.unwrap_or_default()}
    }

    fn open_rs232_connector(
        serial_port: String,
        baud_rate: u32,
        data_bits: u8,
        parity: char,
        stop_bits: u8,
        hardware_flow_control: bool,
        software_flow_control: bool,
    ) -> (SyncSender<PjLinkRS232MessageRequest>, Receiver<PjLinkRS232MessageResponse>) {
        let (to_connector_tx, to_connector_rx) = sync_channel::<PjLinkRS232MessageRequest>(0);
        let (from_connector_tx, from_connector_rx) = channel::<PjLinkRS232MessageResponse>();

        let connector_channel = (from_connector_tx, to_connector_rx);

        thread::spawn(move || {
            PjLinkRS232Connector::spawn(
                PjLinkRS232ConnectorOptions {
                    serial_port,
                    baud_rate, 
                    data_bits,
                    parity,
                    stop_bits,
                    hardware_flow_control,
                    software_flow_control,
                },
                connector_channel
            );
        }); 

        (to_connector_tx, from_connector_rx)
    }
}

impl PjLinkHandler for PjLinkRS232Projector {
    fn handle_command(&mut self, command: PjLinkCommand, raw_command: &PjLinkRawPayload, connection_id: &u64) -> PjLinkResponse {
        match command {
            // #region Class Information Query / CLSS
            PjLinkCommand::Class1 => {
                info!("Class Information Query");
                PjLinkResponse::Single(self.options.class_type)
            }
            // #endregion
            // #region Serial Number Query / SNUM
            PjLinkCommand::SerialNumber2 => {
                info!("Serial Number Query");
                PjLinkResponse::Multiple(self.options.serial_number.clone())
            }
            // #endregion
            // #region Software Version Query / SVER
            PjLinkCommand::SoftwareVersion2 => {
                info!("Software Version Query");
                PjLinkResponse::Multiple(self.options.software_version.clone())
            }
            // #endregion
            // #region Projector/Display Name Query / NAME
            PjLinkCommand::Name1 => {
                info!("Name Query");
                PjLinkResponse::Multiple(self.options.projector_name.clone())
            }
            // #endregion
            // #region Manufacture Name Information Query / INF1
            PjLinkCommand::InfoManufacturer1 => {
                info!("Info Manufacturer Query");
                PjLinkResponse::Multiple(self.options.manufacturer_name.clone())
            }
            // #endregion
            // #region Product Name Information Query / INF2
            PjLinkCommand::InfoProductName1 => {
                info!("Info Product Name Query");
                PjLinkResponse::Multiple(self.options.product_name.clone())
            }
            // #endregion
            // #region Input Resolution Query / IRES
            PjLinkCommand::InputResolution2 => {
                info!("Input Resolution Query");
                PjLinkResponse::Multiple(self.options.screen_resolution.clone())
            }
            // #endregion
            // #region Recommend Resolution Query / RRES
            PjLinkCommand::RecommendResolution2 => {
                info!("Recommend Resolution Query");
                PjLinkResponse::Multiple(self.options.recommended_screen_resolution.clone())
            }
            _ => self.handle_dynamic_content(command, raw_command, connection_id)
        }
    }

    fn get_password(&mut self, _connection_id: &u64) -> Option<String> {
        self.options.password.clone()
    }
}