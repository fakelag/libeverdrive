mod edos;

#[derive(Debug)]
pub struct Everdrive {
    port: Box<dyn serialport::SerialPort>,
}

impl Everdrive {
    /// Creates a new Everdrive instance and returns an error if the device is not found
    /// or if there is an error opening the USB serial port.
    ///
    /// `timeout` is configured for the serial port for future reads and writes
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use libeverdrive::Everdrive;
    ///
    /// let mut ed = match Everdrive::new(std::time::Duration::from_millis(100)) {
    ///     Ok(ed) => ed,
    ///     Err(err) => {
    ///         eprintln!("Failed to find Everdrive: {:?}", err);
    ///         return;
    ///     }
    /// };
    /// ```
    pub fn new(timeout: std::time::Duration) -> std::io::Result<Self> {
        let ports = serialport::available_ports().expect("No available USB ports found");

        let usb_port = match ports.iter().find(|p| match &p.port_type {
            serialport::SerialPortType::UsbPort(info) => info.vid == 0x0403 && info.pid == 0x6001,
            _ => false,
        }) {
            Some(port) => port,
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Everdrive USB device not found",
                ));
            }
        };

        let mut port = match serialport::new(&usb_port.port_name, 115_200).open() {
            Ok(port) => port,
            Err(err) => {
                return Err(err.into());
            }
        };

        match port.set_timeout(timeout) {
            Ok(_) => (),
            Err(err) => {
                return Err(err.into());
            }
        };

        let mut ed = Self { port };

        ed.ed_status()?;

        Ok(ed)
    }

    /// Directly write a buffer to the serial port
    pub fn write(&mut self, buf: &[u8]) -> std::io::Result<()> {
        self.port.write_all(buf)
    }

    /// Directly read a buffer from the serial port
    pub fn read(&mut self, buf: &mut [u8]) -> std::io::Result<()> {
        self.port.read_exact(buf)
    }
}
