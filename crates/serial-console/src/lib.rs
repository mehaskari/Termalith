//! Serial console access: port discovery, validated line settings, and an
//! open port split into a reader (with a short timeout, so callers can stop
//! promptly) and a writer.

use std::io::{Read, Write};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Read timeout; bounds how long a reader takes to notice it should stop.
pub const READ_TIMEOUT: Duration = Duration::from_millis(100);

#[derive(Debug, thiserror::Error)]
pub enum SerialError {
    #[error("invalid {field}: {reason}")]
    Invalid { field: &'static str, reason: String },
    #[error("permission denied opening {path}{hint}")]
    PermissionDenied { path: String, hint: &'static str },
    #[error("{path} was not found or is not available")]
    NotFound { path: String },
    #[error("serial port error: {0}")]
    Other(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Parity {
    #[default]
    None,
    Odd,
    Even,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum FlowControl {
    #[default]
    None,
    Software,
    Hardware,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerialConfig {
    /// `/dev/tty.usbserial-…`, `/dev/ttyUSB0`, `COM3`, …
    pub path: String,
    pub baud_rate: u32,
    pub data_bits: u8,
    pub parity: Parity,
    pub stop_bits: u8,
    pub flow_control: FlowControl,
}

impl SerialConfig {
    /// The usual console default, 9600 8N1, no flow control.
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            baud_rate: 9600,
            data_bits: 8,
            parity: Parity::None,
            stop_bits: 1,
            flow_control: FlowControl::None,
        }
    }

    pub fn validate(&self) -> Result<(), SerialError> {
        let bad = |field, reason: &str| {
            Err(SerialError::Invalid {
                field,
                reason: reason.into(),
            })
        };
        let p = self.path.trim();
        if p.is_empty() || p.len() > 260 || p.chars().any(char::is_control) {
            return bad("port", "not a valid device name");
        }
        if !(50..=4_000_000).contains(&self.baud_rate) {
            return bad("baud rate", "must be 50-4000000");
        }
        if !(5..=8).contains(&self.data_bits) {
            return bad("data bits", "must be 5-8");
        }
        if !matches!(self.stop_bits, 1 | 2) {
            return bad("stop bits", "must be 1 or 2");
        }
        Ok(())
    }

    /// e.g. `115200 8N1`.
    pub fn summary(&self) -> String {
        let parity = match self.parity {
            Parity::None => 'N',
            Parity::Odd => 'O',
            Parity::Even => 'E',
        };
        format!(
            "{} {}{}{}",
            self.baud_rate, self.data_bits, parity, self.stop_bits
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortInfo {
    pub path: String,
    /// "USB (FTDI FT232R)", "PCI", "Bluetooth", or empty.
    pub description: String,
}

/// Serial ports the OS reports. On macOS, `/dev/cu.*` (call-out) devices are
/// listed before `/dev/tty.*` because they do not wait for carrier detect.
pub fn list_ports() -> Result<Vec<PortInfo>, SerialError> {
    let mut ports: Vec<PortInfo> = serialport::available_ports()
        .map_err(|e| SerialError::Other(e.to_string()))?
        .into_iter()
        .map(|p| PortInfo {
            description: match &p.port_type {
                serialport::SerialPortType::UsbPort(u) => {
                    let name = [u.manufacturer.as_deref(), u.product.as_deref()]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(" ");
                    if name.is_empty() {
                        "USB".into()
                    } else {
                        format!("USB ({name})")
                    }
                }
                serialport::SerialPortType::PciPort => "PCI".into(),
                serialport::SerialPortType::BluetoothPort => "Bluetooth".into(),
                serialport::SerialPortType::Unknown => String::new(),
            },
            path: p.port_name,
        })
        .collect();
    ports.sort_by_key(|p| (!p.path.starts_with("/dev/cu."), p.path.clone()));
    Ok(ports)
}

pub type SerialReader = Box<dyn Read + Send>;
pub type SerialWriter = Box<dyn Write + Send>;

/// Opens a port. Reads time out after [`READ_TIMEOUT`] (`TimedOut` errors
/// are expected and mean "no data yet").
pub fn open(cfg: &SerialConfig) -> Result<(SerialReader, SerialWriter), SerialError> {
    cfg.validate()?;
    let path = cfg.path.trim();
    let port = serialport::new(path, cfg.baud_rate)
        .data_bits(match cfg.data_bits {
            5 => serialport::DataBits::Five,
            6 => serialport::DataBits::Six,
            7 => serialport::DataBits::Seven,
            _ => serialport::DataBits::Eight,
        })
        .parity(match cfg.parity {
            Parity::None => serialport::Parity::None,
            Parity::Odd => serialport::Parity::Odd,
            Parity::Even => serialport::Parity::Even,
        })
        .stop_bits(if cfg.stop_bits == 2 {
            serialport::StopBits::Two
        } else {
            serialport::StopBits::One
        })
        .flow_control(match cfg.flow_control {
            FlowControl::None => serialport::FlowControl::None,
            FlowControl::Software => serialport::FlowControl::Software,
            FlowControl::Hardware => serialport::FlowControl::Hardware,
        })
        .timeout(READ_TIMEOUT)
        .open()
        .map_err(|e| map_open_error(path, e))?;
    let writer = port
        .try_clone()
        .map_err(|e| SerialError::Other(e.to_string()))?;
    Ok((Box::new(port), Box::new(writer)))
}

fn map_open_error(path: &str, e: serialport::Error) -> SerialError {
    let msg = e.to_string().to_lowercase();
    let io_kind = match e.kind() {
        serialport::ErrorKind::Io(k) => Some(k),
        _ => None,
    };
    if io_kind == Some(std::io::ErrorKind::PermissionDenied)
        || msg.contains("permission denied")
        || msg.contains("access is denied")
    {
        let hint = if cfg!(target_os = "linux") {
            " (add your user to the 'dialout' group, then log out and back in)"
        } else if cfg!(windows) {
            " (is another program using the port?)"
        } else {
            ""
        };
        return SerialError::PermissionDenied {
            path: path.into(),
            hint,
        };
    }
    if matches!(e.kind(), serialport::ErrorKind::NoDevice)
        || io_kind == Some(std::io::ErrorKind::NotFound)
    {
        return SerialError::NotFound { path: path.into() };
    }
    SerialError::Other(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_and_summary() {
        let mut c = SerialConfig::new("/dev/ttyUSB0");
        assert!(c.validate().is_ok());
        assert_eq!(c.summary(), "9600 8N1");
        c.baud_rate = 115_200;
        c.parity = Parity::Even;
        c.data_bits = 7;
        assert_eq!(c.summary(), "115200 7E1");
        for bad in [
            SerialConfig {
                path: String::new(),
                ..SerialConfig::new("x")
            },
            SerialConfig {
                baud_rate: 0,
                ..SerialConfig::new("x")
            },
            SerialConfig {
                data_bits: 9,
                ..SerialConfig::new("x")
            },
            SerialConfig {
                stop_bits: 3,
                ..SerialConfig::new("x")
            },
            SerialConfig {
                path: "a\nb".into(),
                ..SerialConfig::new("x")
            },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn missing_device_is_reported_as_not_found_or_error() {
        let r = open(&SerialConfig::new(if cfg!(windows) {
            "COM250"
        } else {
            "/dev/termalith-no-such-tty"
        }));
        assert!(
            matches!(
                r,
                Err(SerialError::NotFound { .. }) | Err(SerialError::Other(_))
            ),
            "{:?}",
            r.err()
        );
    }
}
