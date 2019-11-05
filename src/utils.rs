use bytes::{Buf, BufMut, BytesMut, IntoBuf};
#[cfg(feature = "derive")]
use serde::{Deserialize, Serialize};
use std::{
    error::Error as ErrorTrait,
    fmt,
    io::{Error as IoError, ErrorKind},
    num::NonZeroU16,
};

/// Errors returned by [`encode()`] and [`decode()`].
///
/// [`encode()`]: fn.encode.html
/// [`decode()`]: fn.decode.html
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Not enough space in the write buffer.
    ///
    /// It is the caller's responsiblity to pass a big enough buffer to `encode()`.
    WriteZero,
    /// Tried to encode or decode a ProcessIdentifier==0.
    InvalidPid,
    /// Tried to decode a QoS > 2.
    InvalidQos(u8),
    /// Tried to decode a ConnectReturnCode > 5.
    InvalidConnectReturnCode(u8),
    /// Tried to decode an unknown protocol.
    InvalidProtocol(String, u8),
    /// Tried to decode an invalid fixed header (packet type, flags, or remaining_length).
    InvalidHeader,
    /// Trying to encode/decode an invalid length.
    ///
    /// The difference with `WriteZero`/`UnexpectedEof` is that it refers to an invalid/corrupt
    /// length rather than a buffer size issue.
    InvalidLength,
    /// Trying to decode a non-utf8 string.
    InvalidString(std::str::Utf8Error),
    /// Catch-all error when converting from `std::io::Error`.
    ///
    /// You'll hopefully never see this.
    IoError(ErrorKind, String),
}
impl ErrorTrait for Error {}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}
impl From<Error> for IoError {
    fn from(err: Error) -> IoError {
        match err {
            Error::WriteZero => IoError::new(ErrorKind::WriteZero, err),
            _ => IoError::new(ErrorKind::InvalidData, err),
        }
    }
}
impl From<IoError> for Error {
    fn from(err: IoError) -> Error {
        match err.kind() {
            ErrorKind::WriteZero => Error::WriteZero,
            k => Error::IoError(k, format!("{}", err)),
        }
    }
}

/// Packet Identifier.
///
/// For packets with [`QoS::AtLeastOne` or `QoS::ExactlyOnce`] delivery.
///
/// ```rust
/// # use mqttrs::{Pid, Packet};
/// let pid = Pid::try_from(42).expect("illegal pid value");
/// let next_pid = pid + 1;
/// let pending_acks = std::collections::HashMap::<Pid, Packet>::new();
/// ```
///
/// The spec ([MQTT-2.3.1-1], [MQTT-2.2.1-3]) disallows a pid of 0.
///
/// [`QoS::AtLeastOne` or `QoS::ExactlyOnce`]: enum.QoS.html
/// [MQTT-2.3.1-1]: https://docs.oasis-open.org/mqtt/mqtt/v3.1.1/os/mqtt-v3.1.1-os.html#_Toc398718025
/// [MQTT-2.2.1-3]: https://docs.oasis-open.org/mqtt/mqtt/v5.0/os/mqtt-v5.0-os.html#_Toc3901026
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "derive", derive(Serialize, Deserialize))]
pub struct Pid(NonZeroU16);
impl Pid {
    /// Returns a new `Pid` with value `1`.
    pub fn new() -> Self {
        Pid(NonZeroU16::new(1).unwrap())
    }
    /// Returns a new `Pid` with specified value.
    // Not using std::convert::TryFrom so that don't have to depend on rust 1.34.
    pub fn try_from(u: u16) -> Result<Self, Error> {
        match NonZeroU16::new(u) {
            Some(nz) => Ok(Pid(nz)),
            None => Err(Error::InvalidPid),
        }
    }
    /// Get the `Pid` as a raw `u16`.
    pub fn get(self) -> u16 {
        self.0.get()
    }
    pub(crate) fn from_buffer(buf: &mut BytesMut) -> Result<Self, Error> {
        Self::try_from(buf.split_to(2).into_buf().get_u16_be())
    }
    pub(crate) fn to_buffer(self, buf: &mut BytesMut) -> Result<(), Error> {
        Ok(buf.put_u16_be(self.get()))
    }
}
impl std::ops::Add<u16> for Pid {
    type Output = Pid;
    fn add(self, u: u16) -> Pid {
        let n = match self.get().overflowing_add(u) {
            (n, false) => n,
            (n, true) => n + 1,
        };
        Pid(NonZeroU16::new(n).unwrap())
    }
}
impl std::ops::Sub<u16> for Pid {
    type Output = Pid;
    fn sub(self, u: u16) -> Pid {
        let n = match self.get().overflowing_sub(u) {
            (0, _) => std::u16::MAX,
            (n, false) => n,
            (n, true) => n - 1,
        };
        Pid(NonZeroU16::new(n).unwrap())
    }
}

/// Packet delivery [Quality of Service] level.
///
/// [Quality of Service]: http://docs.oasis-open.org/mqtt/mqtt/v3.1.1/os/mqtt-v3.1.1-os.html#_Toc398718099
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "derive", derive(Serialize, Deserialize))]
pub enum QoS {
    /// `QoS 0`. No ack needed.
    AtMostOnce,
    /// `QoS 1`. One ack needed.
    AtLeastOnce,
    /// `QoS 2`. Two acks needed.
    ExactlyOnce,
}
impl QoS {
    pub(crate) fn to_u8(&self) -> u8 {
        match *self {
            QoS::AtMostOnce => 0,
            QoS::AtLeastOnce => 1,
            QoS::ExactlyOnce => 2,
        }
    }
    pub(crate) fn from_u8(byte: u8) -> Result<QoS, Error> {
        match byte {
            0 => Ok(QoS::AtMostOnce),
            1 => Ok(QoS::AtLeastOnce),
            2 => Ok(QoS::ExactlyOnce),
            n => Err(Error::InvalidQos(n)),
        }
    }
}

/// Combined [`QoS`]/[`Pid`].
///
/// Used only in [`Publish`] packets.
///
/// [`Publish`]: struct.Publish.html
/// [`QoS`]: enum.QoS.html
/// [`Pid`]: struct.Pid.html
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "derive", derive(Serialize, Deserialize))]
pub enum QosPid {
    AtMostOnce,
    AtLeastOnce(Pid),
    ExactlyOnce(Pid),
}
impl QosPid {
    #[cfg(test)]
    pub(crate) fn from_u8u16(qos: u8, pid: u16) -> Self {
        match qos {
            0 => QosPid::AtMostOnce,
            1 => QosPid::AtLeastOnce(Pid::try_from(pid).expect("pid == 0")),
            2 => QosPid::ExactlyOnce(Pid::try_from(pid).expect("pid == 0")),
            _ => panic!("Qos > 2"),
        }
    }
    /// Extract the [`Pid`] from a `QosPid`, if any.
    ///
    /// [`Pid`]: struct.Pid.html
    pub fn pid(self) -> Option<Pid> {
        match self {
            QosPid::AtMostOnce => None,
            QosPid::AtLeastOnce(p) => Some(p),
            QosPid::ExactlyOnce(p) => Some(p),
        }
    }
    /// Extract the [`QoS`] from a `QosPid`.
    ///
    /// [`QoS`]: enum.QoS.html
    pub fn qos(self) -> QoS {
        match self {
            QosPid::AtMostOnce => QoS::AtMostOnce,
            QosPid::AtLeastOnce(_) => QoS::AtLeastOnce,
            QosPid::ExactlyOnce(_) => QoS::ExactlyOnce,
        }
    }
}

#[cfg(test)]
mod test {
    use crate::Pid;

    #[test]
    fn pid_add_sub() {
        let t: Vec<(u16, u16, u16, u16)> = vec![
            (2, 1, 1, 3),
            (100, 1, 99, 101),
            (1, 1, std::u16::MAX, 2),
            (1, 2, std::u16::MAX - 1, 3),
            (1, 3, std::u16::MAX - 2, 4),
            (std::u16::MAX, 1, std::u16::MAX - 1, 1),
            (std::u16::MAX, 2, std::u16::MAX - 2, 2),
            (10, std::u16::MAX, 10, 10),
            (10, 0, 10, 10),
            (1, 0, 1, 1),
            (std::u16::MAX, 0, std::u16::MAX, std::u16::MAX),
        ];
        for (cur, d, prev, next) in t {
            let sub = Pid::try_from(cur).unwrap() - d;
            let add = Pid::try_from(cur).unwrap() + d;
            assert_eq!(prev, sub.get(), "{} - {} should be {}", cur, d, prev);
            assert_eq!(next, add.get(), "{} + {} should be {}", cur, d, next);
        }
    }
}
